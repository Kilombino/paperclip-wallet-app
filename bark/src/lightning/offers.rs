//! One persistent wallet-owned offer, reusable while the wallet daemon is online.

use std::str::FromStr;
use std::sync::Weak;
use std::time::Duration;

use anyhow::Context;
use bitcoin::{Amount, secp256k1::PublicKey};
use bitcoin::hashes::{sha256, Hash};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use server_rpc::protos;
use ark::{bolt12_receive, ProtocolEncoding};
use ark::lightning::{Invoice, Offer};
use crate::{Wallet, WalletInner};
use crate::actions::WalletActionCheckpoint;
use crate::actions::lightning::receive::{LightningReceive, Progress, ln_recv_action_id};

pub(crate) const OFFER_CHECKPOINT: &str = "ln_offer.default";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightningOffer {
	pub offer: String,
	pub description: String,
	pub amount_sat: Option<u64>,
	pub active: bool,
	pub key_index: u32,
	pub relay_pubkey: PublicKey,
}

impl Wallet {
	pub async fn lightning_offer(&self) -> anyhow::Result<Option<LightningOffer>> {
		Ok(match self.inner.db.get_wallet_action_checkpoint(&OFFER_CHECKPOINT.into()).await? {
			Some(WalletActionCheckpoint::LightningOffer(offer)) => Some(offer),
			_ => None,
		})
	}

	pub async fn create_lightning_offer(&self, description: String, amount_sat: Option<u64>) -> anyhow::Result<LightningOffer> {
		ensure!(!self.config().daemon_manual_sync, "reusable offers require background sync");
		let _guard = self.inner.lock_manager.try_lock(OFFER_CHECKPOINT).await.context("offer update in progress")?;
		if let Some(existing) = self.lightning_offer().await? {
			if existing.active {
				ensure!(existing.description == description && existing.amount_sat == amount_sat, "disable the existing offer before changing its details");
				return Ok(existing);
			}
		}
		let (mut srv, info) = self.require_server().await?;
		ensure!(info.funded_lightning, "server has not enabled funded Lightning");
		let relay = srv.client.get_lightning_offer_info(protos::Empty {}).await?.into_inner();
		if let Some(amount) = amount_sat { ensure!(amount > 0 && amount <= relay.maximum_sat, "amount outside server limits"); }
		let relay_pubkey = PublicKey::from_slice(&relay.relay_pubkey)?;
		let (key, key_index) = self.derive_store_next_keypair().await?;
		let offer = bolt12_receive::create_offer(key.public_key(), relay_pubkey, self.network().await?, description.clone(), amount_sat)?;
		let state = LightningOffer { offer: offer.to_string(), description, amount_sat, active: true, key_index, relay_pubkey };
		self.inner.db.upsert_wallet_action_checkpoint(&OFFER_CHECKPOINT.into(), &WalletActionCheckpoint::LightningOffer(state.clone())).await?;
		Ok(state)
	}

	pub async fn disable_lightning_offer(&self) -> anyhow::Result<()> {
		let _guard = self.inner.lock_manager.try_lock(OFFER_CHECKPOINT).await.context("offer update in progress")?;
		let mut offer = self.lightning_offer().await?.context("no reusable offer")?;
		offer.active = false;
		self.inner.db.upsert_wallet_action_checkpoint(&OFFER_CHECKPOINT.into(), &WalletActionCheckpoint::LightningOffer(offer)).await?;
		Ok(())
	}

	async fn answer_offer(&self, state: &LightningOffer, bytes: &[u8]) -> anyhow::Result<String> {
		let _offer_guard = self.inner.lock_manager.try_lock(OFFER_CHECKPOINT).await.context("offer update in progress")?;
		ensure!(self.lightning_offer().await?.as_ref() == Some(state) && state.active, "offer disabled");
		let (mut srv, info) = self.require_server().await?;
		let relay = srv.client.get_lightning_offer_info(protos::Empty {}).await?.into_inner();
		ensure!(relay.relay_pubkey == state.relay_pubkey.serialize(), "offer relay changed; create a new offer");
		let offer = Offer::from_str(&state.offer).map_err(|e| anyhow!("invalid stored offer: {e:?}"))?;
		let (request, amount_sat) = bolt12_receive::parse_request(bytes, &offer, self.network().await?, relay.maximum_sat)?;
		let amount = Amount::from_sat(amount_sat);
		let fee = info.fees.lightning_receive.calculate(amount).context("fee overflow")?;
		ark::fees::validate_and_subtract_fee(amount, fee)?;
		let key = self.peek_keypair(state.key_index).await?;
		let preimage = bolt12_receive::request_preimage(&key, bytes)?;
		let hash = preimage.compute_payment_hash();
		let id = ln_recv_action_id(hash);
		let _guard = self.inner.lock_manager.try_lock(&id).await.context("receive already being processed")?;
		ensure!(self.inner.db.get_settled_lightning_receive(hash).await?.is_none(), "request already paid");
		let receive = if let Some(existing) = self.lightning_receive_checkpoint(hash).await? {
			ensure!(matches!(existing.progress, Progress::AwaitingPayment), "request already accepted");
			ensure!(!existing.invoice.expired_at(std::time::UNIX_EPOCH.elapsed()?), "request expired; request a new invoice");
			existing
		} else {
			ensure!(self.pending_lightning_receives().await?.len() < 128, "too many pending receives");
			let config = self.config();
			let delta = info.vtxo_exit_delta.checked_add(info.htlc_expiry_delta)
				.and_then(|v| v.checked_add(config.vtxo_exit_margin))
				.and_then(|v| v.checked_add(config.htlc_recv_claim_delta))
				.and_then(|v| v.checked_add(2u16.into())).context("CLTV overflow")?;
			ensure!(delta <= info.max_user_invoice_cltv_delta, "server CLTV limit too low for recovery");
			let lightning_delta = delta.checked_add(info.htlc_expiry_delta).context("CLTV overflow")?;
			let maximum_height = relay.block_height.checked_add(u32::from(lightning_delta)).and_then(|v| v.checked_add(144)).context("height overflow")?;
			let invoice = bolt12_receive::create_invoice(&request, &offer, &key, state.relay_pubkey, preimage, lightning_delta.into(), maximum_height)?;
			let (_, key_index) = self.derive_store_next_keypair().await?;
			let receive = LightningReceive {
				invoice: Invoice::Bolt12(invoice), payment_hash: hash, payment_preimage: preimage,
				htlc_recv_cltv_delta: delta, anti_dos_token: None, claim_destination: None,
				key_index, progress: Progress::AwaitingPayment,
			};
			// Durable before either registration or publication. A timeout or restart
			// reuses this exact invoice and preimage, never an untracked payment.
			self.inner.db.upsert_wallet_action_checkpoint(&id, &receive.clone().into()).await?;
			receive
		};
		let mailbox = ark::mailbox::MailboxIdentifier::from_pubkey(self.inner.seed.to_mailbox_keypair().public_key());
		srv.client.register_bolt12_receive(protos::RegisterBolt12ReceiveRequest {
			invoice: receive.invoice.to_string(), min_cltv_delta: receive.htlc_recv_cltv_delta.into(), mailbox_id: Some(mailbox.serialize()),
		}).await?;
		Ok(receive.invoice.to_string())
	}

	pub(crate) async fn serve_offer_weak(weak: Weak<WalletInner>, shutdown: CancellationToken) -> anyhow::Result<()> {
		let (state, key, mut srv) = {
			let wallet = Wallet { inner: weak.upgrade().context("wallet closed")? };
			let Some(state) = wallet.lightning_offer().await?.filter(|o| o.active) else { return Ok(()); };
			let key = wallet.peek_keypair(state.key_index).await?;
			let (srv, _) = wallet.require_server().await?;
			(state, key, srv)
		};
		let (tx, rx) = mpsc::channel(8);
		let mut stream = srv.client.serve_lightning_offers(ReceiverStream::new(rx)).await?.into_inner();
		let greeting = tokio::time::timeout(Duration::from_secs(10), stream.message()).await??.context("relay closed")?;
		let offer = Offer::from_str(&state.offer).map_err(|e| anyhow!("invalid stored offer: {e:?}"))?;
		let message = bolt12_receive::session_challenge(&offer, &greeting.challenge)?;
		let signature = ark::SECP.sign_schnorr_no_aux_rand(&message, &key);
		tx.send(protos::LightningOfferClient { offer: state.offer.clone(), challenge_signature: signature.as_ref().to_vec(), ..Default::default() }).await?;
		let mut timer = tokio::time::interval(Duration::from_secs(5));
		loop {
			let event = tokio::select! {
				_ = shutdown.cancelled() => return Ok(()),
				_ = timer.tick() => None,
				r = stream.message() => Some(r),
			};
			let wallet = Wallet { inner: weak.upgrade().context("wallet closed")? };
			if wallet.lightning_offer().await?.as_ref() != Some(&state) { return Ok(()); }
			if let Some(event) = event {
				let request = event?.context("relay disconnected")?;
				ensure!(request.request_id == sha256::Hash::hash(&request.invoice_request).to_byte_array(), "relay request id mismatch");
				let answer = tokio::time::timeout(Duration::from_secs(15), wallet.answer_offer(&state, &request.invoice_request)).await;
				let mut response = protos::LightningOfferClient { request_id: request.request_id, ..Default::default() };
				match answer {
					Ok(Ok(invoice)) => response.invoice = invoice,
					Ok(Err(error)) => { log::warn!("BOLT12 request refused: {error:#}"); response.error = "request unavailable".into(); },
					Err(_) => response.error = "request timed out".into(),
				}
				tx.send(response).await?;
			}
		}
	}
}
