use std::collections::HashMap;

use anyhow::Context;
use bitcoin::{Amount, NetworkKind};
use bitcoin::hex::DisplayHex;
use bitcoin::secp256k1::{Keypair, PublicKey};
use bitcoin_ext::BlockHeight;
use log::{error, info, warn};

use ark::{ProtocolEncoding, VtxoPolicy};
use ark::arkoor::{ArkoorDestination, ArkoorConstructionError};
use ark::arkoor::package::{ArkoorPackageBuilder, ArkoorPackageCosignResponse};
use ark::vtxo::{Full, Vtxo, VtxoId};
use server_rpc::{protos, ServerConnection};

use crate::{VtxoDelivery, Wallet, WalletVtxo};
use crate::actions::DriveMode;
use crate::actions::arkoor_send::{ArkoorSend, start_arkoor_send};

/// The result of creating an arkoor transaction
pub struct ArkoorCreateResult {
	pub recovery_reserve: Amount,
	pub inputs: Vec<VtxoId>,
	pub created: Vec<Vtxo<Full>>,
	pub change: Vec<Vtxo<Full>>,
}

/// Error returned by [`Wallet::create_checkpointed_arkoor_with_vtxos`].
///
/// The cosign RPC failure is kept as a typed [`tonic::Status`] rather
/// than flattened into `anyhow`, so a caller driving this as a wallet
/// action can route a genuine server rejection to its `on_rejection`
/// path (via `AdvanceError::is_server_rejection`) instead of retrying a
/// doomed request forever. Every other failure is opaque `Other`.
#[derive(Debug, thiserror::Error)]
pub enum ArkoorCreateError {
	/// The `request_arkoor_cosign` RPC failed. May be a rejection
	/// (`InvalidArgument`/`NotFound`) or a transient error; the caller
	/// classifies it via the status code.
	#[error("server failed to cosign arkoor: {0}")]
	Cosign(#[source] tonic::Status),
	#[error(transparent)]
	Other(#[from] anyhow::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArkoorAddressError {
	#[error("Ark address is for different network")]
	NetworkMismatch,
	#[error("Ark address is for different server")]
	ServerMismatch,
	#[error("VTXO policy in address cannot be used for arkoor payment: {0:?}")]
	PolicyNotSupported(VtxoPolicy),
	#[error("Unknown delivery mechanism: {0}")]
	UnknownDeliveryMechanism(String),
	#[error("Other error: {0}")]
	Other(String),
}

/// Split a change amount into the piece amounts of the change destinations.
///
/// Change exceeding the payment is split in `split_factor` pieces
/// (see [crate::Config::change_vtxo_split_factor]) so that repeated payments
/// build a tree of change VTXOs rather than a chain. Pieces below the dust
/// threshold are fine here: [ark::arkoor::ArkoorBuilder] isolates them.
///
/// Retries reuse the pieces stored on the action, so this policy can
/// change between versions.
pub(crate) fn split_change_amount(change: Amount, pay: Amount, split_factor: u8) -> Vec<Amount> {
	if change == Amount::ZERO {
		return Vec::new();
	}
	let pieces = if change > pay { u64::from(split_factor.max(1)) } else { 1 };
	let base = change / pieces;
	let mut ret = vec![base; pieces as usize];
	*ret.last_mut().unwrap() = change - base * (pieces - 1);
	ret
}

/// Resolve the change outputs of an arkoor package from the pieces stored
/// on the action. `None` means the action was persisted by a pre-split
/// bark, which built a single whole change output.
pub(crate) fn resolve_change_pieces(
	stored: Option<Vec<Amount>>,
	change: Amount,
) -> anyhow::Result<Vec<Amount>> {
	match stored {
		Some(pieces) => {
			let sum = pieces.iter().copied().sum::<Amount>();
			ensure!(sum == change, "stored change pieces sum to {}, expected {}", sum, change);
			Ok(pieces)
		},
		None if change == Amount::ZERO => Ok(Vec::new()),
		None => Ok(vec![change]),
	}
}

/// Outcome of one [`post_arkoor_to_mailboxes`] pass.
pub(crate) enum DeliveryOutcome {
	/// At least one mailbox accepted the post.
	AnySucceeded,
	/// No mailbox accepted the post. `summary` describes why and is meant to
	/// be captured in a caller's park error for observability.
	AllFailed { summary: String },
}

/// Posts `vtxos` to every [`VtxoDelivery::ServerMailbox`] method found in
/// `delivery`, in order, skipping any other delivery variant. Mailbox posts
/// are idempotent on the server.
///
/// Any-success semantics: one accepted post is enough, since the recipient
/// only needs the signed chain to arrive once.
pub(crate) async fn post_arkoor_to_mailboxes(
	srv: &mut ServerConnection,
	delivery: &[VtxoDelivery],
	vtxos: impl IntoIterator<Item = impl AsRef<Vtxo<Full>>>,
) -> DeliveryOutcome {
	let serialized = vtxos.into_iter()
		.map(|v| v.as_ref().serialize().to_vec())
		.collect::<Vec<_>>();

	let mut any_succeeded = false;
	let mut failures: Vec<String> = Vec::new();
	for method in delivery {
		let VtxoDelivery::ServerMailbox { blinded_id } = method else { continue };
		let req = protos::mailbox_server::PostArkoorMessageRequest {
			blinded_id: blinded_id.as_ref().to_vec(),
			vtxos: serialized.clone(),
		};
		match srv.mailbox_client.post_arkoor_message(req).await {
			Ok(_) => any_succeeded = true,
			Err(e) => {
				let reason = format!("{:#}", e);
				error!("failed to post arkoor vtxos to mailbox: {}", reason);
				failures.push(reason);
			},
		}
	}

	if any_succeeded {
		return DeliveryOutcome::AnySucceeded;
	}
	let summary = if failures.is_empty() {
		"no mailbox delivery mechanism configured on destination".to_string()
	} else {
		format!("no delivery mechanism accepted the arkoor vtxos: {}", failures.join("; "))
	};
	DeliveryOutcome::AllFailed { summary }
}

/// Checks that the address lists a useable delivery mechanism.
///
/// If the address doesn't specify a delivery mechanism that is clearly
/// the receiver choice and this is allowed.
///
/// If all delivery methods are unknown we will error and not initiate
/// a payment.
fn check_delivery(delivery: &[VtxoDelivery]) -> Result<(), ArkoorAddressError> {
	// The receiver explicitly wants no delivery. We should honour it
	if delivery.is_empty() {
		return Ok(());
	}

	if delivery.iter().any(|d| matches!(d, VtxoDelivery::ServerMailbox { .. })) {
		return Ok(());
	}

	let listed = delivery.iter()
		.map(|d| match d {
			VtxoDelivery::Unknown { delivery_type, data } => {
				format!("type={:#x}, data={}", delivery_type, data.as_hex())
			},
			other => format!("{:?}", other),
		})
		.collect::<Vec<_>>()
		.join("; ");
	Err(ArkoorAddressError::UnknownDeliveryMechanism(listed))
}

impl Wallet {
	/// Validate if we can send arkoor payments to the given [ark::Address], for example an error
	/// will be returned if the given [ark::Address] belongs to a different server (see
	/// [ark::address::ArkId]).
	pub async fn validate_arkoor_address(&self, address: &ark::Address) -> Result<(), ArkoorAddressError> {
		let network = self.network().await
			.map_err(|e| ArkoorAddressError::Other(e.to_string()))?;
		let (_, ark_info) = self.require_server().await
			.map_err(|e| ArkoorAddressError::Other(e.to_string()))?;

		let network_kind = NetworkKind::from(network);
		if address.is_testnet() == network_kind.is_mainnet() {
			return Err(ArkoorAddressError::NetworkMismatch);
		}

		if !address.ark_id().is_for_server(ark_info.server_pubkey) {
			return Err(ArkoorAddressError::ServerMismatch);
		}

		// Not all policies are supported for sending arkoor
		match address.policy() {
			VtxoPolicy::Pubkey(_) => {},
			VtxoPolicy::ServerHtlcRecv_v0(_) | VtxoPolicy::ServerHtlcSend_v0(_)
				| VtxoPolicy::ServerHtlcRecv(_) | VtxoPolicy::ServerHtlcSend(_) =>
			{
				return Err(ArkoorAddressError::PolicyNotSupported(address.policy().clone()));
			}
		}

		check_delivery(address.delivery())?;

		Ok(())
	}

	/// Build, cosign and split an arkoor package using a caller-provided
	/// change keypair.
	///
	/// Reusing the same change keypair on a retry keeps the implied
	/// `spending_txid` stable, so the server's `check_spendable_for_oor`
	/// idempotency check accepts the retry rather than rejecting it as a
	/// conflicting double-spend.
	pub(crate) async fn create_checkpointed_arkoor_with_vtxos(
		&self,
		arkoor_dest: ArkoorDestination,
		inputs: impl IntoIterator<Item = WalletVtxo>,
		change_keypair: Keypair,
		change_pieces: Option<Vec<Amount>>,
	) -> Result<ArkoorCreateResult, ArkoorCreateError> {
		let (mut srv, _) = self.require_server().await?;
		let input_ids = inputs.into_iter().map(|v| v.id()).collect::<Vec<_>>();

		// Hydrate the inputs to their full form: the arkoor builder needs
		// the genesis chain and the server registration call sends the
		// full bytes over the wire.
		let inputs = self.inner.db.get_full_vtxos(&input_ids).await
			.context("failed to hydrate arkoor input vtxos")?;

		// Pre-register the input chains so the post-cosign register call
		// for the outputs finds a signed chain anchor:
		// register_vtxo_transactions validates a vtxo against its anchor's
		// signed_tx in the DB, and boarded inputs sit unsigned in
		// virtual_transaction (see register_board) until a
		// register_vtxo_transactions call backfills them.
		self.register_vtxo_transactions_with_server(&inputs).await
			.context("failed to register arkoor input vtxo transactions with server")?;

		for input in &inputs {
			self.validate_funded_admission(input).await?;
			let tip = self.inner.chain.tip().await?.to_u32();
			let required = u32::try_from(input.exit_depth()).context("exit depth overflow")?
				.checked_add(u32::from(input.exit_delta().to_u16()) + 14)
				.and_then(|v| v.checked_add(tip)).context("exit deadline overflow")?;
			if required >= input.expiry_height().to_u32() {
				return Err(anyhow!("refresh required before another transfer").into());
			}
		}
		let change_pubkey = change_keypair.public_key();
		if arkoor_dest.policy.user_pubkey() == change_pubkey {
			return Err(anyhow!("Cannot create arkoor to same address as change").into());
		}

		let mut user_keypairs = vec![];
		for vtxo in &inputs {
			user_keypairs.push(self.get_vtxo_key(vtxo).await?);
		}

		// The profile fixes the output allocation. Older split-piece hints cannot
		// change recipient amounts or create dust recovery outputs.
		let _ = change_pieces;
		let (builder, recovery_reserve) = ArkoorPackageBuilder::new_funded_payment(
			inputs, arkoor_dest.clone(), VtxoPolicy::new_pubkey(change_pubkey),
		).context("insufficient funded recovery reserves; refresh or select more inputs")?;
		let builder = builder.generate_user_nonces(&user_keypairs)
			.context("invalid nb of keypairs")?;

		let cosign_request = protos::ArkoorPackageCosignRequest::from(
			builder.cosign_request(),
		);

		let response = srv.client.request_arkoor_cosign(cosign_request).await
			.map_err(ArkoorCreateError::Cosign)?
			.into_inner();

		let cosign_responses = ArkoorPackageCosignResponse::try_from(response)
			.context("Failed to parse cosign response from server")?;

		let vtxos = builder
			.user_cosign(&user_keypairs, cosign_responses)
			.context("Failed to cosign vtxos")?
			.build_signed_vtxos();

		// divide between change and destination
		let (dest, change) = vtxos.into_iter()
			.partition::<Vec<_>, _>(|v| *v.policy() == arkoor_dest.policy);

		Ok(ArkoorCreateResult {
			recovery_reserve,
			inputs: input_ids,
			created: dest,
			change,
		})
	}

	/// Makes an out-of-round payment to the given [ark::Address]. This does not require waiting for
	/// a round, so it should be relatively instantaneous.
	///
	/// If the [Wallet] doesn't contain a VTXO larger than the given [Amount], multiple payments
	/// will be chained together, resulting in the recipient receiving multiple VTXOs.
	///
	/// Note that a change [Vtxo] may be created as a result of this call. With each payment these
	/// will become more uneconomical to unilaterally exit, so you should eventually refresh them
	/// with [Wallet::refresh_vtxos] or periodically call [Wallet::maintenance_refresh].
	pub async fn send_arkoor_payment(
		&self,
		destination: &ark::Address,
		amount: Amount,
	) -> anyhow::Result<()> {
		self.send_arkoor_payment_with_max_cost(destination, amount, None).await
	}

	/// Send with an optional upper bound on the amount plus recovery reserve.
	pub async fn send_arkoor_payment_with_max_cost(
		&self, destination: &ark::Address, amount: Amount, max_total: Option<Amount>,
	) -> anyhow::Result<()> {
		self.send_arkoor_payment_from(destination, amount, max_total, None).await
	}

	/// Send spending exactly the VTXOs in `inputs` (coin control), or let the wallet choose
	/// when it is `None`.
	pub async fn send_arkoor_payment_from(
		&self, destination: &ark::Address, amount: Amount, max_total: Option<Amount>,
		inputs: Option<Vec<VtxoId>>,
	) -> anyhow::Result<()> {
		let action = start_arkoor_send(self, destination.clone(), amount, max_total, inputs).await?;

		// Persist the action together with the input locks so the executor has
		// something to drive on restart; otherwise a crash between this point and
		// `drive_action` leaves vtxos locked under an action id that has no
		// checkpoint row.
		self.inner.db.upsert_wallet_action_checkpoint(&action.id, &action.clone().into()).await?;

		self.drive_action(action, DriveMode::UntilDone).await
	}

	/// Read-only plan shared by estimates and sends. No keys, locks or signatures are created.
	pub(crate) async fn plan_arkoor_payment(
		&self, amount: Amount, policy: VtxoPolicy, change: PublicKey, chosen: Option<&[VtxoId]>,
	) -> anyhow::Result<(Vec<WalletVtxo>, Amount)> {
		self.inner.chain.require_funded_policy().await?;
		let _ = self.require_server().await?;
		ensure!(amount > Amount::ZERO, "payment amount must be positive");
		let tip = self.inner.chain.tip().await?;
		let candidates = self.spendable_vtxos().await?;
		let reserve = ark::exit_policy::paperclip_funding().per_transaction();
		if let Some(chosen) = chosen {
			// Coin control: spend exactly these, all of them, or refuse.
			ensure!(!chosen.is_empty(), "no VTXOs chosen");
			let mut inputs = Vec::with_capacity(chosen.len());
			for id in chosen {
				ensure!(!inputs.iter().any(|v: &WalletVtxo| v.id() == *id), "VTXO {} chosen twice", id);
				let v = candidates.iter().find(|v| v.id() == *id)
					.with_context(|| format!("VTXO {} is not spendable", id))?;
				inputs.push(v.clone());
			}
			// The funded package decides exactly: a coin spent whole pays 2 reserves, one
			// with change pays 3 and must leave a non-dust change. Below the whole-coin
			// minimum it can never work, so say so plainly; otherwise let it decide.
			let count = u64::try_from(inputs.len()).context("input count overflow")?;
			let minimum = amount.checked_add(reserve.checked_mul(
				count.checked_mul(2).context("recovery reserve overflow")?,
			).context("recovery reserve overflow")?).context("payment amount overflow")?;
			let total = inputs.iter().map(|v| v.amount()).sum::<Amount>();
			ensure!(total >= minimum,
				"the chosen VTXOs hold {} but this payment needs at least {} (amount plus {} recovery reserve)",
				total, minimum, minimum - amount);
			return self.check_arkoor_inputs(inputs, amount, policy, change, tip).await
				.with_context(|| format!("the chosen VTXOs hold {}: spend them whole (amount {}) or leave \
					a change of at least 1330 sats after 3 reserves per coin", total, total - minimum + amount));
		}
		// Automatic selection: the cheapest constructible single coin, then bounded
		// combinations, priced by the funded package itself.
		let selection = self.spend_input_selection().await?.expires_after(tip);
		let candidates = selection.eligible(candidates);
		let ids = candidates.iter().map(|v| v.id()).collect::<Vec<_>>();
		let full = self.inner.db.get_full_vtxos(&ids).await?.into_iter()
			.map(|v| (v.id(), v)).collect::<HashMap<_, _>>();
		let (inputs, _) = selection.select_ark_constructible(candidates, |inputs| {
			let hydrated = inputs.iter().map(|v| full.get(&v.id()).cloned()
				.context("Missing Ark input ancestry")).collect::<anyhow::Result<Vec<_>>>()?;
			match ArkoorPackageBuilder::new_funded_payment(hydrated,
				ArkoorDestination { total_amount: amount, policy: policy.clone() },
				VtxoPolicy::new_pubkey(change),
			) {
				Ok((_, reserve)) => Ok(Some(reserve)),
				Err(ArkoorConstructionError::Dust | ArkoorConstructionError::Unbalanced { .. }) => Ok(None),
				Err(error) => Err(error.into()),
			}
		})?;
		self.check_arkoor_inputs(inputs, amount, policy, change, tip).await
	}

	async fn check_arkoor_inputs(
		&self, inputs: Vec<WalletVtxo>, amount: Amount, policy: VtxoPolicy, change: PublicKey,
		tip: BlockHeight,
	) -> anyhow::Result<(Vec<WalletVtxo>, Amount)> {
		let ids = inputs.iter().map(|v| v.id()).collect::<Vec<_>>();
		let full = self.inner.db.get_full_vtxos(&ids).await?;
		for input in &full {
			self.validate_funded_admission(input).await?;
			let deadline = u32::try_from(input.exit_depth()).context("exit depth overflow")?
				.checked_add(u32::from(input.exit_delta().to_u16()) + 14)
				.and_then(|v| v.checked_add(tip.to_u32())).context("exit deadline overflow")?;
			ensure!(deadline < input.expiry_height().to_u32(), "refresh required before another transfer");
		}
		let (_, recovery_reserve) = ArkoorPackageBuilder::new_funded_payment(
			full, ArkoorDestination { total_amount: amount, policy }, VtxoPolicy::new_pubkey(change),
		).context("payment would leave an unfunded or dust recovery output; refresh first")?;
		Ok((inputs, recovery_reserve))
	}

	/// Returns every in-progress arkoor send checkpoint.
	pub async fn pending_arkoor_sends(&self) -> anyhow::Result<Vec<ArkoorSend>> {
		Ok(self.inner.db.get_all_wallet_action_checkpoints().await?
			.into_iter()
			.filter_map(|cp| cp.into_arkoor_send())
			.collect())
	}

	/// Drives every pending arkoor send forward by one step or to
	/// completion if it's ready.
	pub async fn sync_pending_arkoor_sends(&self) -> anyhow::Result<()> {
		let pending = self.pending_arkoor_sends().await?;
		if pending.is_empty() {
			return Ok(());
		}
		info!("Syncing {} pending arkoor sends", pending.len());
		for send in pending {
			let id = send.id.clone();
			if let Err(e) = self.drive_action(send, DriveMode::UntilParkOrDone).await {
				warn!("Failed to sync arkoor send {}: {:#}", id, e);
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn resolve_change_pieces_fallback_and_validation() {
		let change = Amount::from_sat(30_000);
		let pieces = vec![Amount::from_sat(10_000), Amount::from_sat(20_000)];

		// stored pieces are used as-is when they sum to the change
		assert_eq!(resolve_change_pieces(Some(pieces.clone()), change).unwrap(), pieces);

		// a sum mismatch is an error, not a silently different package
		assert!(resolve_change_pieces(Some(pieces), Amount::from_sat(30_001)).is_err());

		// no stored pieces (pre-split checkpoint) rebuilds a single whole output
		assert_eq!(resolve_change_pieces(None, change).unwrap(), vec![change]);
		assert_eq!(resolve_change_pieces(None, Amount::ZERO).unwrap(), Vec::<Amount>::new());
	}

	#[test]
	fn check_delivery_requires_a_mailbox() {
		use std::str::FromStr;

		let mailbox = VtxoDelivery::ServerMailbox {
			blinded_id: ark::mailbox::BlindedMailboxIdentifier::from_str(
				"024b0d4a4e8a29d2f36a83b4ff4a0e5c5e6f0f8b8d1f2a3b4c5d6e7f80912a3b4c",
			).unwrap(),
		};
		let unknown = VtxoDelivery::Unknown { delivery_type: 0xff, data: vec![1, 2, 3] };

		// an address without any delivery mechanism is the receiver's choice
		assert_eq!(check_delivery(&[]), Ok(()));

		// an address that only lists mechanisms we can't deliver to can't be paid
		assert!(matches!(
			check_delivery(&[unknown.clone()]),
			Err(ArkoorAddressError::UnknownDeliveryMechanism(_)),
		));

		// one usable mechanism is enough, whatever else is listed
		assert_eq!(check_delivery(&[mailbox.clone()]), Ok(()));
		assert_eq!(check_delivery(&[unknown, mailbox]), Ok(()));
	}

	#[test]
	fn split_change_amount_pieces() {
		let pay = Amount::from_sat(10_000);

		for factor in 1..=3u8 {
			// zero change yields no pieces
			assert_eq!(split_change_amount(Amount::ZERO, pay, factor), Vec::<Amount>::new());

			// change at or below the payment stays whole
			for sats in [1, 5_000, 10_000] {
				let change = Amount::from_sat(sats);
				assert_eq!(split_change_amount(change, pay, factor), vec![change]);
			}

			// change above the payment splits into factor pieces that add back up
			for sats in [10_001, 123_457, 100_000_000] {
				let change = Amount::from_sat(sats);
				let pieces = split_change_amount(change, pay, factor);
				assert_eq!(pieces.len(), factor as usize);
				assert_eq!(pieces.iter().copied().sum::<Amount>(), change);
			}
		}
	}
}
