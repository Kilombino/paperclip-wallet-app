//! Ark inside Kilombino wallet (Android).
//!
//! Instead of binding the whole bark API to Kotlin, this runs the same wallet daemon and
//! REST API that paperclip-walletd serves to its web UI, inside the app process:
//!
//! - bound to 127.0.0.1 only, on a port chosen by the app;
//! - protected by a fresh random bearer token generated on every start and handed to
//!   the app in memory (never written to disk), so other apps on the phone that find
//!   the port cannot use it;
//! - chain data over Esplora (mempool.kilombino.com/api), because a phone cannot run a
//!   Knots node;
//! - the mnemonic never touches disk: the app keeps it encrypted by the Android Keystore
//!   and hands it over in memory on every start and on wallet creation.
//!
//! JNI surface: `start(datadir, port, mnemonic, passphrase) -> token` and `stop()`. Everything else is HTTP.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, Once, OnceLock};

use anyhow::Context;
use bitcoin::secp256k1::rand::{self, RngCore};
use log::{info, warn};

use bark_cli::connection;
use bark_cli::wallet::{ConfigOpts, CreateOpts, create_wallet, open_wallet_with_mnemonic_and_passphrase};
use bark_json::web::{BarkNetwork, BitcoindAuth, ChainSourceConfig, CreateWalletRequest};
use bark_rest::auth::AuthToken;
use bark_rest::{Config, OnWalletCreate, OnWalletDelete, RestServer, ServerState};
use tokio_util::sync::CancellationToken;

const USER_AGENT: &str = "kilombino-wallet-ark/0.1";

struct Running {
	server: RestServer,
	shutdown: CancellationToken,
	wallet: Option<bark::Wallet>,
	// Held for as long as the daemon runs: stops a second instance on the same datadir.
	_lock: Box<dyn std::any::Any + Send>,
}

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
// The global logger can be set only once per process; the engine restarts (for a backup)
// without the process ending.
static LOGGING: Once = Once::new();

fn runtime() -> &'static tokio::runtime::Runtime {
	RUNTIME.get_or_init(|| {
		tokio::runtime::Builder::new_multi_thread()
			// A phone does not need many workers, and the wallet mostly waits on network.
			.worker_threads(2)
			.enable_all()
			.thread_name("kilombino-ark")
			.build()
			.expect("tokio runtime")
	})
}

fn create_opts(req: CreateWalletRequest) -> anyhow::Result<CreateOpts> {
	// The app always supplies the words (new or shared with its XBT wallet), so no
	// wallet is ever created with a seed only the datadir knows.
	let words = req.mnemonic.context("the app must supply the mnemonic")?;
	let mnemonic = Some(bip39::Mnemonic::from_str(&words).context("invalid mnemonic")?);
	let passphrase = req.passphrase.unwrap_or_default();
	#[allow(deprecated)]
	let mut config = ConfigOpts {
		ark: req.ark_server,
		access_token: req.ark_server_access_token,
		esplora: None,
		bitcoind: None,
		bitcoind_cookie: None,
		bitcoind_user: None,
		bitcoind_pass: None,
		socks5_proxy: None,
		gap_limit: req.gap_limit,
	};
	match req.chain_source {
		Some(ChainSourceConfig::Esplora { url }) => config.esplora = Some(url),
		Some(ChainSourceConfig::Bitcoind { bitcoind, bitcoind_auth }) => {
			config.bitcoind = Some(bitcoind);
			match bitcoind_auth {
				BitcoindAuth::Cookie { cookie } => config.bitcoind_cookie = Some(cookie),
				BitcoindAuth::UserPass { user, pass } => {
					config.bitcoind_user = Some(user);
					config.bitcoind_pass = Some(pass);
				},
			}
		},
		None => {},
	}
	Ok(CreateOpts {
		force: req.force,
		use_filestore: false,
		mainnet: req.network == BarkNetwork::Mainnet,
		regtest: req.network == BarkNetwork::Regtest,
		signet: req.network == BarkNetwork::Signet,
		mutinynet: req.network == BarkNetwork::Mutinynet,
		mnemonic,
		birthday_height: req.birthday_height.map(Into::into),
		passphrase,
		config,
		write_mnemonic_file: false,
	})
}

/// Starts the wallet daemon and its REST API. Returns the bearer token to use.
/// `mnemonic` is `None` while the app has no Ark wallet yet; `passphrase` is its optional
/// BIP-39 passphrase ("" for none). Starting twice returns an error rather than a second daemon.
pub fn start(datadir: &Path, port: u16, mnemonic: Option<&str>, passphrase: &str) -> anyhow::Result<String> {
	let mut running = RUNNING.lock().unwrap();
	anyhow::ensure!(running.is_none(), "already running");

	// Paperclip only enables XBT mainnet when asked to explicitly.
	// SAFETY: set once, before the runtime spawns wallet threads that read it.
	unsafe { std::env::set_var("PAPERCLIP_XBT_MAINNET", "1"); }

	std::fs::create_dir_all(datadir).with_context(|| format!("create {}", datadir.display()))?;
	LOGGING.call_once(|| bark_cli::log::init_logging(false, true, datadir, None, false));

	let mut secret = [0u8; 32];
	rand::thread_rng().fill_bytes(&mut secret);
	let token = AuthToken::new(secret);
	let encoded = token.encode();

	let mnemonic = match mnemonic {
		Some(m) => Some(bip39::Mnemonic::from_str(m).context("invalid mnemonic")?),
		None => None,
	};

	let datadir = datadir.to_path_buf();
	let passphrase = passphrase.to_owned();
	let r = runtime().block_on(async move {
		let lock = connection::acquire_barkd_lock(&datadir)?;
		let shutdown = CancellationToken::new();

		let wallet = match mnemonic {
			Some(m) => open_wallet_with_mnemonic_and_passphrase(&datadir, USER_AGENT, m, &passphrase).await?,
			None => None,
		};
		if let Some(w) = &wallet {
			w.start_daemon()?;
			info!("Ark wallet loaded, daemon started");
		} else {
			warn!("No Ark wallet yet; REST API up so the app can create one");
		}

		let on_create: Box<OnWalletCreate> = Box::new({
			let datadir = datadir.clone();
			move |req: CreateWalletRequest| {
				let datadir = datadir.clone();
				Box::pin(async move {
					let opts = create_opts(req)?;
					let mnemonic = opts.mnemonic.clone().expect("checked in create_opts");
					let passphrase = opts.passphrase.clone();
					create_wallet(&datadir, USER_AGENT, opts).await?;
					let wallet = open_wallet_with_mnemonic_and_passphrase(&datadir, USER_AGENT, mnemonic, &passphrase).await?
						.context("wallet just created")?;
					if let Err(e) = wallet.refresh_server().await {
						warn!("Ark server handshake failed on wallet creation: {:#}", e);
					}
					wallet.start_daemon()?;
					Ok::<_, anyhow::Error>(wallet)
				})
			}
		});
		let on_delete: Box<OnWalletDelete> = Box::new({
			let datadir = datadir.clone();
			move || {
				let datadir = datadir.clone();
				Box::pin(async move {
					connection::wipe_datadir_except_barkd_files(&datadir)?;
					Ok(())
				})
			}
		});

		let state = ServerState::builder()
			.wallet(wallet.clone())
			.auth_token(Some(token))
			.on_wallet_create(on_create)
			.on_wallet_delete(on_delete)
			.on_get_mnemonic(None)
			.build(shutdown.clone());

		let mut config = Config::default();
		config.host = "127.0.0.1".parse().unwrap();
		config.port = port;
		let server = RestServer::start(&config, Arc::new(state), shutdown.clone()).await?;
		Ok::<_, anyhow::Error>(Running { server, shutdown, wallet, _lock: Box::new(lock) })
	})?;
	*running = Some(r);
	Ok(encoded)
}

/// Stops the daemon and the REST API (waits for a clean shutdown).
pub fn stop() {
	let taken = RUNNING.lock().unwrap().take();
	if let Some(r) = taken {
		r.shutdown.cancel();
		if let Some(w) = &r.wallet {
			w.stop_daemon();
		}
		let _ = runtime().block_on(r.server.stop_wait());
	}
}

// ---------------------------------------------------------------------------- JNI

mod jni_api {
	use super::*;
	use jni::JNIEnv;
	use jni::objects::{JClass, JString};
	use jni::sys::{jint, jstring};

	/// `ArkNative.start(datadir, port, mnemonic, passphrase)`: the bearer token, or
	/// "ERR:<message>". `mnemonic` is null while the app has no Ark wallet yet;
	/// `passphrase` is "" (or null) for none.
	#[unsafe(no_mangle)]
	pub extern "system" fn Java_com_kilombino_pyblockwatch_ark_ArkNative_start<'l>(
		mut env: JNIEnv<'l>, _class: JClass<'l>, datadir: JString<'l>, port: jint,
		mnemonic: JString<'l>, passphrase: JString<'l>,
	) -> jstring {
		// A string that cannot be read is an error, never "no words" or "no passphrase": either
		// would silently open a different (empty-looking) wallet.
		let err = |env: &mut JNIEnv<'l>, m: &str| {
			env.new_string(format!("ERR:{m}")).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
		};
		let words: Option<String> = if mnemonic.is_null() {
			None
		} else {
			match env.get_string(&mnemonic) {
				Ok(w) => Some(w.into()),
				Err(e) => return err(&mut env, &format!("could not read the words: {e}")),
			}
		};
		let passphrase: String = if passphrase.is_null() {
			String::new()
		} else {
			match env.get_string(&passphrase) {
				Ok(p) => p.into(),
				Err(e) => return err(&mut env, &format!("could not read the passphrase: {e}")),
			}
		};
		let out = match env.get_string(&datadir) {
			Ok(d) => {
				let d: String = d.into();
				match start(&PathBuf::from(d), port as u16, words.as_deref(), &passphrase) {
					Ok(token) => token,
					Err(e) => format!("ERR:{:#}", e),
				}
			},
			Err(e) => format!("ERR:bad datadir: {e}"),
		};
		env.new_string(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
	}

	/// `ArkNative.stop()`.
	#[unsafe(no_mangle)]
	pub extern "system" fn Java_com_kilombino_pyblockwatch_ark_ArkNative_stop<'l>(
		_env: JNIEnv<'l>, _class: JClass<'l>,
	) {
		stop();
	}
}
