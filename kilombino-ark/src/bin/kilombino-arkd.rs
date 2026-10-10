//! The Ark engine of the Kilombino wallet as its own process, for Kilowallet on a home node
//! (kilowallet-server, StartOS). Same engine and REST API as the Android library: the
//! paperclip-walletd daemon and its API on 127.0.0.1 only, behind a fresh random bearer token.
//!
//! Usage: `kilombino-arkd --datadir DIR --port PORT`
//!
//! - stdin, first two lines: the mnemonic (empty line: no Ark wallet yet) and the BIP-39
//!   passphrase (empty for none). They stay in memory; nothing is written to disk.
//! - stdout: one line, `TOKEN <token>` once the API is up, or `ERR <message>` (exit 2).
//! - It runs until stdin closes (the parent went away) or SIGTERM/SIGINT, then shuts the
//!   wallet down cleanly.

use std::io::{BufRead, Write};
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
	args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn fail(msg: &str) -> ! {
	println!("ERR {}", msg.replace('\n', " "));
	let _ = std::io::stdout().flush();
	std::process::exit(2);
}

fn main() {
	let args: Vec<String> = std::env::args().collect();
	let datadir = arg(&args, "--datadir").map(PathBuf::from).unwrap_or_else(|| fail("--datadir is required"));
	let port: u16 = arg(&args, "--port").and_then(|p| p.parse().ok()).unwrap_or_else(|| fail("--port is required"));

	// Read both lines, then let go of stdin: the watcher below locks it again.
	let (mut words, mut passphrase) = {
		let stdin = std::io::stdin();
		let mut lines = stdin.lock().lines();
		let mut next = |what: &str| -> String {
			match lines.next() {
				Some(Ok(l)) => l.trim_end_matches(['\r', '\n']).to_owned(),
				Some(Err(e)) => fail(&format!("could not read the {what}: {e}")),
				None => fail(&format!("stdin closed before the {what}")),
			}
		};
		let w = next("words");
		let p = next("passphrase");
		(w, p)
	};
	let mnemonic = if words.trim().is_empty() { None } else { Some(words.trim().to_owned()) };
	// Drop our copies of the secrets as soon as the engine holds its own.
	let started = kilombino_ark::start(&datadir, port, mnemonic.as_deref(), &passphrase);
	words.replace_range(.., &"\0".repeat(words.len()));
	passphrase.replace_range(.., &"\0".repeat(passphrase.len()));
	drop(mnemonic);
	let token = match started {
		Ok(t) => t,
		Err(e) => fail(&format!("{:#}", e)),
	};
	println!("TOKEN {token}");
	let _ = std::io::stdout().flush();

	// Stop on SIGTERM/SIGINT, or when stdin closes: a parent that dies takes the engine with it.
	let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
	{
		let tx = tx.clone();
		std::thread::spawn(move || {
			let mut sink = String::new();
			let stdin = std::io::stdin();
			let mut lock = stdin.lock();
			loop {
				sink.clear();
				match lock.read_line(&mut sink) {
					Ok(0) | Err(_) => break,
					Ok(_) => continue,
				}
			}
			let _ = tx.send("stdin closed");
		});
	}
	std::thread::spawn(move || {
		let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("signal runtime");
		rt.block_on(async {
			use tokio::signal::unix::{SignalKind, signal};
			let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
			let mut int = signal(SignalKind::interrupt()).expect("SIGINT handler");
			tokio::select! {
				_ = term.recv() => { let _ = tx.send("SIGTERM"); },
				_ = int.recv() => { let _ = tx.send("SIGINT"); },
			}
		});
	});
	let why = rx.recv().unwrap_or("channel closed");
	log::info!("kilombino-arkd stopping: {why}");
	kilombino_ark::stop();
}
