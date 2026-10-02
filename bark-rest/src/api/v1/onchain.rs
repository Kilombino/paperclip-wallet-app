use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{debug_handler, Json, Router};
use bitcoin::Amount;
use tracing::info;
use utoipa::OpenApi;

use bark::onchain::{OnchainWallet, OnchainWalletTrait};

use crate::ServerState;
use crate::error::{self, HandlerResult, ContextExt};

/// Cast an [OnchainWalletTrait] to an [OnchainWallet]
fn cast_bdk(w: &dyn OnchainWalletTrait) -> anyhow::Result<&OnchainWallet> {
	(w as &dyn std::any::Any).downcast_ref::<OnchainWallet>()
		.context("onchain wallet is not a BDK wallet")
}

/// Cast a mutable [OnchainWalletTrait] to an [OnchainWallet]
fn cast_bdk_mut(w: &mut dyn OnchainWalletTrait) -> anyhow::Result<&mut OnchainWallet> {
	(w as &mut dyn std::any::Any).downcast_mut::<OnchainWallet>()
		.context("onchain wallet is not a BDK wallet")
}

pub fn router() -> Router<Arc<ServerState>> {
	Router::new()
		.route("/message/sign", post(onchain_message_sign))
		.route("/message/verify", post(onchain_message_verify))
		.route("/balance", get(onchain_balance))
		.route("/addresses/next", post(onchain_address))
		.route("/send", post(onchain_send))
		.route("/send-many", post(onchain_send_many))
		.route("/drain", post(onchain_drain))
		.route("/send-selected", post(onchain_send_selected))
		.route("/send-selected/estimate", post(onchain_send_selected_estimate))
		.route("/utxos", get(onchain_utxos))
		.route("/transactions", get(onchain_transactions))
		.route("/sync", post(onchain_sync))
}

#[derive(OpenApi)]
#[openapi(
	paths(
		onchain_message_sign,
		onchain_message_verify,
		onchain_balance,
		onchain_address,
		onchain_send,
		onchain_send_many,
		onchain_drain,
		onchain_send_selected,
		onchain_send_selected_estimate,
		onchain_utxos,
		onchain_transactions,
		onchain_sync,
	),
	components(schemas(
		bark_json::web::OnchainMessageRequest,
		bark_json::web::OnchainMessageProof,
		bark_json::web::OnchainMessageVerifyRequest,
		bark_json::web::OnchainMessageVerification,
		bark_json::cli::onchain::OnchainBalance,
		bark_json::cli::onchain::Address,
		bark_json::cli::onchain::Send,
		bark_json::web::OnchainSendRequest,
		bark_json::web::OnchainSendManyRequest,
		bark_json::web::OnchainDrainRequest,
		bark_json::web::OnchainSendSelectedRequest,
		bark_json::web::OnchainSendSelectedEstimate,
		bark_json::primitives::UtxoInfo,
		bark_json::primitives::TransactionInfo,
		bark_json::primitives::WalletTxInfo,
		bark_json::primitives::BlockRef,
	)),
	tags((name = "onchain", description = "Manage barkd's on-chain bitcoin wallet."))
)]
pub struct OnchainApiDoc;

#[utoipa::path(post, path = "/message/sign", tag = "onchain",
	request_body = bark_json::web::OnchainMessageRequest,
	responses((status = 200, body = bark_json::web::OnchainMessageProof)),
	summary = "Sign an on-chain Taproot address ownership message using BIP322-simple")]
pub async fn onchain_message_sign(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainMessageRequest>,
) -> HandlerResult<Json<bark_json::web::OnchainMessageProof>> {
	let network = state.require_wallet()?.network().await?;
	let address = bitcoin::Address::from_str(&body.address).badarg("Invalid address")?
		.require_network(network).badarg("Wrong address network")?;
	let onchain = state.require_onchain()?;
	let guard = onchain.read().await;
	let signature = cast_bdk(&*guard)?.sign_onchain_message(&address, &body.message)
		.badarg("Unable to sign on-chain message")?;
	Ok(Json(bark_json::web::OnchainMessageProof {
		address: address.to_string(), message: body.message, signature,
		scheme: "bip322-simple".into(),
	}))
}

#[utoipa::path(post, path = "/message/verify", tag = "onchain",
	request_body = bark_json::web::OnchainMessageVerifyRequest,
	responses((status = 200, body = bark_json::web::OnchainMessageVerification)),
	summary = "Verify a BIP322-simple Taproot address ownership message")]
pub async fn onchain_message_verify(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainMessageVerifyRequest>,
) -> HandlerResult<Json<bark_json::web::OnchainMessageVerification>> {
	let network = state.require_wallet()?.network().await?;
	let address = bitcoin::Address::from_str(&body.address).badarg("Invalid address")?
		.require_network(network).badarg("Wrong address network")?;
	let valid = bark::onchain::message::verify_onchain_message(&address, &body.message, &body.signature)
		.badarg("Invalid or unsupported message proof")?;
	Ok(Json(bark_json::web::OnchainMessageVerification { valid }))
}

#[utoipa::path(
	get,
	path = "/balance",
	summary = "Get on-chain balance",
	responses(
		(status = 200, description = "Returns the on-chain balance", body = bark_json::cli::onchain::OnchainBalance),
	),
	description = "Returns the current on-chain wallet balance, broken down by confirmation \
		status. The `trusted_spendable_sat` field is the sum of `confirmed_sat` and \
		`trusted_pending_sat`—the balance that can be safely spent without risk of \
		double-spend.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_balance(
	State(state): State<Arc<ServerState>>,
) -> HandlerResult<Json<bark_json::cli::onchain::OnchainBalance>> {
	let onchain = state.require_onchain()?;
	let guard = onchain.read().await;
	let balance = cast_bdk(&*guard)?.balance();
	let onchain_balance = bark_json::cli::onchain::OnchainBalance {
		total: balance.total(),
		trusted_spendable: balance.trusted_spendable(),
		immature: balance.immature,
		trusted_pending: balance.trusted_pending,
		untrusted_pending: balance.untrusted_pending,
		confirmed: balance.confirmed,
	};

	Ok(axum::Json(onchain_balance))
}

#[utoipa::path(
	post,
	path = "/addresses/next",
	summary = "Generate on-chain address",
	responses(
		(status = 200, description = "Returns the on-chain address", body = bark_json::cli::onchain::Address),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Generates a new on-chain receiving address. Each call returns the next \
		unused address from the wallet's HD keychain.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_address(
	State(state): State<Arc<ServerState>>,
) -> HandlerResult<Json<bark_json::cli::onchain::Address>> {
	let onchain = state.require_onchain()?;

	let address = onchain.write().await.address().await
		.context("Wallet failed to generate address")?;

	Ok(axum::Json(bark_json::cli::onchain::Address {
		address: address.into_unchecked()
	}))
}

#[utoipa::path(
	post,
	path = "/send",
	summary = "Send on-chain payment",
	request_body = bark_json::web::OnchainSendRequest,
	responses(
		(status = 200, description = "Returns the send result", body = bark_json::cli::onchain::Send),
		(status = 400, description = "The provided destination address is invalid", body = error::BadRequestError),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Sends the specified amount to an on-chain address. Broadcasts the \
		transaction immediately at a fee rate targeting confirmation within three blocks \
		and returns the transaction ID.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_send(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainSendRequest>,
) -> HandlerResult<Json<bark_json::cli::onchain::Send>> {
	let wallet = state.require_wallet()?;
	let onchain = state.require_onchain()?;

	let net = wallet.network().await?;
	let addr = bitcoin::Address::from_str	(&body.destination)
		.badarg("Invalid destination address")?
		.require_network(net)
		.badarg("Address is not valid for configured network")?;

	let fee_rate = wallet.chain().fee_rates().await.regular;
	let amount = Amount::from_sat(body.amount_sat);
	let txid = cast_bdk_mut(&mut *onchain.write().await)?.send(wallet.chain(), addr, amount, fee_rate).await
		.context("Failed to send onchain payment")?;

	Ok(axum::Json(bark_json::cli::onchain::Send { txid }))
}

/// Parse a coin-control request into (coins, destination, amount).
async fn selected_parts(
	wallet: &bark::Wallet,
	body: &bark_json::web::OnchainSendSelectedRequest,
) -> HandlerResult<(Vec<bitcoin::OutPoint>, bitcoin::Address, Option<Amount>)> {
	let net = wallet.network().await?;
	let addr = bitcoin::Address::from_str(&body.destination)
		.badarg("Invalid destination address")?
		.require_network(net)
		.badarg("Address is not valid for configured network")?;
	let mut outpoints = Vec::with_capacity(body.outpoints.len());
	for o in &body.outpoints {
		outpoints.push(bitcoin::OutPoint::from_str(o).badarg("Invalid outpoint")?);
	}
	Ok((outpoints, addr, body.amount_sat.map(Amount::from_sat)))
}

#[utoipa::path(
	post,
	path = "/send-selected/estimate",
	summary = "Estimate a coin-control send",
	request_body = bark_json::web::OnchainSendSelectedRequest,
	responses(
		(status = 200, description = "The fee, amount and change", body = bark_json::web::OnchainSendSelectedEstimate),
		(status = 400, description = "Invalid address, coin or amount", body = error::BadRequestError),
	),
	description = "Builds, without signing or sending, the transaction that spends exactly the \
		given coins, and returns its fee, what the destination gets and the change.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_send_selected_estimate(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainSendSelectedRequest>,
) -> HandlerResult<Json<bark_json::web::OnchainSendSelectedEstimate>> {
	let wallet = state.require_wallet()?;
	let onchain = state.require_onchain()?;
	let (outpoints, addr, amount) = selected_parts(&wallet, &body).await?;
	let fee_rate = wallet.chain().fee_rates().await.regular;
	let dest_spk = addr.script_pubkey();
	let psbt = cast_bdk_mut(&mut *onchain.write().await)?
		.prepare_selected_tx(&outpoints, addr, amount, fee_rate)
		.badarg("Cannot build the transaction from these coins")?;
	let fee = psbt.fee().context("fee")?;
	let to_dest = psbt.unsigned_tx.output.iter().filter(|o| o.script_pubkey == dest_spk)
		.map(|o| o.value).sum::<Amount>();
	let total_out = psbt.unsigned_tx.output.iter().map(|o| o.value).sum::<Amount>();
	Ok(Json(bark_json::web::OnchainSendSelectedEstimate {
		fee_sat: fee.to_sat(),
		amount_sat: to_dest.to_sat(),
		change_sat: (total_out - to_dest).to_sat(),
	}))
}

#[utoipa::path(
	post,
	path = "/send-selected",
	summary = "Send from chosen coins (coin control)",
	request_body = bark_json::web::OnchainSendSelectedRequest,
	responses(
		(status = 200, description = "Returns the send result", body = bark_json::cli::onchain::Send),
		(status = 400, description = "Invalid address, coin or amount", body = error::BadRequestError),
	),
	description = "Spends exactly the given coins: the amount to the destination with change \
		back to the wallet, or all of them to the destination when no amount is given.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_send_selected(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainSendSelectedRequest>,
) -> HandlerResult<Json<bark_json::cli::onchain::Send>> {
	let wallet = state.require_wallet()?;
	let onchain = state.require_onchain()?;
	let (outpoints, addr, amount) = selected_parts(&wallet, &body).await?;
	let fee_rate = wallet.chain().fee_rates().await.regular;
	let txid = cast_bdk_mut(&mut *onchain.write().await)?
		.send_selected(wallet.chain(), &outpoints, addr, amount, fee_rate).await
		.context("Failed to send from the selected coins")?;
	Ok(Json(bark_json::cli::onchain::Send { txid }))
}

#[utoipa::path(
	post,
	path = "/send-many",
	summary = "Send to multiple addresses",
	request_body = bark_json::web::OnchainSendManyRequest,
	responses(
		(status = 200, description = "Returns the send result", body = bark_json::cli::onchain::Send),
		(status = 400, description = "One of the provided destinations is invalid", body = error::BadRequestError),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Batches multiple payments into a single on-chain transaction. Each \
		destination is formatted as `address:amount`. Broadcasts the transaction immediately \
		at a fee rate targeting confirmation within three blocks and returns the transaction \
		ID.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_send_many(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainSendManyRequest>,
) -> HandlerResult<Json<bark_json::cli::onchain::Send>> {
	let onchain = state.require_onchain()?;
	let wallet = state.require_wallet()?;

	let net = wallet.network().await?;
	let outputs = body.destinations
		.iter()
		.map(|dest| {
			let mut parts = dest.splitn(2, ':');
			let addr = {
				let s = parts.next()
					.badarg("invalid destination format, expected address:amount")?;
				bitcoin::Address::from_str(s)
					.badarg("invalid address")?
					.require_network(net)
					.badarg("address is not valid for configured network")?
			};
			let amount = {
				let s = parts.next()
					.badarg("invalid destination format, expected address:amount")?;
				Amount::from_str(s)
					.badarg("invalid amount")?
			};
			Ok((addr, amount))
		})
		.collect::<Result<Vec<_>, error::ErrorResponse>>()?;

	info!("Attempting to send the following:");
	for (address, amount) in &outputs {
		info!("{} to {}", amount, address);
	}

	let fee_rate = wallet.chain().fee_rates().await.regular;
	let txid = cast_bdk_mut(&mut *onchain.write().await)?.send_many(wallet.chain(), &outputs, fee_rate).await
		.context("Failed to send many onchain payments")?;

	Ok(axum::Json(bark_json::cli::onchain::Send { txid }))
}

#[utoipa::path(
	post,
	path = "/drain",
	summary = "Drain on-chain wallet",
	request_body = bark_json::web::OnchainDrainRequest,
	responses(
		(status = 200, description = "Returns the drain result", body = bark_json::cli::onchain::Send),
		(status = 400, description = "The provided destination address is invalid", body = error::BadRequestError),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Sends the entire on-chain wallet balance to the specified address. The \
		recipient receives the full balance minus transaction fees. Broadcasts immediately \
		at a fee rate targeting confirmation within three blocks and returns the transaction \
		ID.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_drain(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::OnchainDrainRequest>,
) -> HandlerResult<Json<bark_json::cli::onchain::Send>> {
	let onchain = state.require_onchain()?;
	let wallet = state.require_wallet()?;

	let net = wallet.network().await?;
	let addr = bitcoin::Address::from_str(&body.destination)
		.badarg("Invalid destination address")?
		.require_network(net)
		.badarg("Address is not valid for configured network")?;

	let fee_rate = wallet.chain().fee_rates().await.regular;
	let txid = cast_bdk_mut(&mut *onchain.write().await)?.drain(wallet.chain(), addr, fee_rate).await
		.context("Failed to drain onchain wallet")?;

	Ok(axum::Json(bark_json::cli::onchain::Send { txid }))
}

#[utoipa::path(
	get,
	path = "/utxos",
	summary = "List on-chain UTXOs",
	responses(
		(status = 200, description = "Returns the on-chain UTXOs", body = Vec<bark_json::primitives::UtxoInfo>),
	),
	description = "Returns all UTXOs in the on-chain wallet. Each entry includes the outpoint, \
		amount, and confirmation height (if confirmed).",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_utxos(
	State(state): State<Arc<ServerState>>,
) -> HandlerResult<Json<Vec<bark_json::primitives::UtxoInfo>>> {
	let onchain = state.require_onchain()?;

	let guard = onchain.read().await;
	let utxos = cast_bdk(&*guard)?.utxos()
		.into_iter()
		.map(bark_json::primitives::UtxoInfo::from)
		.collect::<Vec<_>>();

	Ok(axum::Json(utxos))
}

#[utoipa::path(
	get,
	path = "/transactions",
	summary = "List on-chain transactions",
	responses(
		(status = 200, description = "Returns the on-chain transactions", body = Vec<bark_json::primitives::WalletTxInfo>),
	),
	description = "Returns all on-chain wallet transactions, ordered from oldest to newest. \
		Each entry includes the raw transaction, its fee (when known), the wallet's net balance \
		change, and confirmation status. The fee is `null` for inbound or collaboratively-funded \
		transactions whose foreign prevouts BDK has not indexed.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_transactions(
	State(state): State<Arc<ServerState>>,
) -> HandlerResult<Json<Vec<bark_json::primitives::WalletTxInfo>>> {
	let onchain = state.require_onchain()?;

	let guard = onchain.read().await;
	let mut transactions = cast_bdk(&*guard)?.list_transaction_infos()?;
	// transactions are ordered from newest to oldest, so we reverse them so last terminal item is newest
	transactions.reverse();

	let transactions = transactions.into_iter()
		.map(bark_json::primitives::WalletTxInfo::from)
		.collect::<Vec<_>>();

	Ok(axum::Json(transactions))
}

#[utoipa::path(
	post,
	path = "/sync",
	summary = "Sync on-chain wallet",
	responses(
		(status = 200, description = "Synced on-chain wallet"),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Syncs the on-chain wallet state with the chain source. Fetches new blocks \
		and transactions, updates the UTXO set, and re-submits any stale unconfirmed \
		transactions to the mempool.",
	tag = "onchain"
)]
#[debug_handler]
pub async fn onchain_sync(
	State(state): State<Arc<ServerState>>,
) -> HandlerResult<()> {
	let onchain = state.require_onchain()?;
	let wallet = state.require_wallet()?;

	onchain.write().await.sync(wallet.chain()).await?;
	Ok(())
}
