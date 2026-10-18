use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{debug_handler, Json, Router};
use bitcoin::{Amount, FeeRate};
use bitcoin_ext::FeeRateExt;
use utoipa::OpenApi;

use std::str::FromStr;

use bark::onchain::OnchainWalletTrait;
use serde::Deserialize;

use crate::api::v1::onchain::cast_bdk_mut;
use crate::error::{self, HandlerResult, ContextExt, badarg};
use crate::ServerState;

#[derive(OpenApi)]
#[openapi(
	paths(
		board_amount,
		board_all,
		get_pending_boards,
	),
	components(schemas(
		bark_json::web::BoardRequest,
		bark_json::web::BoardAllRequest,
		bark_json::cli::PendingBoardInfo,
	)),
	tags((name = "boards", description = "Move on-chain bitcoin onto the Ark protocol."))
)]
pub struct BoardsApiDoc;

pub fn router() -> Router<Arc<ServerState>> {
	Router::new()
		.route("/board-amount", post(board_amount))
		.route("/board-all", post(board_all))
		.route("/pending", get(get_pending_boards))
		.route("/board-selected", post(board_selected))
		.route("/board-selected/estimate", post(board_selected_estimate))
}

/// A board from chosen on-chain coins (coin control): exactly these coins fund it, the
/// amount into Ark with change back to the wallet, or all of them when no amount is given.
#[derive(Deserialize)]
pub struct BoardSelectedRequest {
	pub outpoints: Vec<String>,
	pub amount_sat: Option<u64>,
	pub fee_rate_sat_per_vb: Option<f64>,
}

fn selected_coins(body: &BoardSelectedRequest) -> HandlerResult<Vec<bitcoin::OutPoint>> {
	if body.outpoints.is_empty() {
		badarg!("No coins selected");
	}
	let mut out = Vec::with_capacity(body.outpoints.len());
	for o in &body.outpoints {
		out.push(bitcoin::OutPoint::from_str(o).badarg("Invalid outpoint")?);
	}
	Ok(out)
}

/// Builds, without signing or sending, the funding transaction of a board from the chosen
/// coins, and returns its fee, what goes into Ark and the change.
#[debug_handler]
pub async fn board_selected_estimate(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<BoardSelectedRequest>,
) -> HandlerResult<Json<bark_json::web::OnchainSendSelectedEstimate>> {
	let wallet = state.require_wallet()?;
	let onchain = state.require_onchain()?;
	let outpoints = selected_coins(&body)?;
	let fee_rate = match board_fee_rate(body.fee_rate_sat_per_vb)? {
		Some(r) => r,
		None => wallet.chain().fee_rates().await.regular,
	};
	// Any key gives a funding address of the same shape: the size, so the fee, is the same.
	let keypair = wallet.peek_keypair(0).await?;
	let (addr, _) = wallet.board_funding_address(&keypair).await?;
	let spk = addr.script_pubkey();
	let psbt = cast_bdk_mut(&mut *onchain.write().await)?
		.prepare_selected_tx(&outpoints, addr, body.amount_sat.map(Amount::from_sat), fee_rate)
		.badarg("Cannot build the board from these coins")?;
	let fee = anyhow::Context::context(psbt.fee(), "fee")?;
	let to_ark = psbt.unsigned_tx.output.iter().filter(|o| o.script_pubkey == spk)
		.map(|o| o.value).sum::<Amount>();
	let total_out = psbt.unsigned_tx.output.iter().map(|o| o.value).sum::<Amount>();
	Ok(Json(bark_json::web::OnchainSendSelectedEstimate {
		fee_sat: fee.to_sat(),
		amount_sat: to_ark.to_sat(),
		change_sat: (total_out - to_ark).to_sat(),
	}))
}

/// Boards from the chosen coins: signs and hands the funding transaction to the board, which
/// broadcasts it and registers the coin with the server once it has its confirmations.
#[debug_handler]
pub async fn board_selected(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<BoardSelectedRequest>,
) -> HandlerResult<Json<bark_json::cli::PendingBoardInfo>> {
	let wallet = state.require_wallet()?;
	let onchain = state.require_onchain()?;
	let outpoints = selected_coins(&body)?;
	let fee_rate = match board_fee_rate(body.fee_rate_sat_per_vb)? {
		Some(r) => r,
		None => wallet.chain().fee_rates().await.regular,
	};
	let (user_keypair, _) = wallet.derive_store_next_keypair().await?;
	let (addr, expiry_height) = wallet.board_funding_address(&user_keypair).await?;
	let signed = {
		let mut guard = onchain.write().await;
		let bdk = cast_bdk_mut(&mut *guard)?;
		let psbt = bdk.prepare_selected_tx(&outpoints, addr, body.amount_sat.map(Amount::from_sat), fee_rate)
			.badarg("Cannot build the board from these coins")?;
		bdk.finish_psbt(psbt).await?
	};
	let board = wallet.board_psbt(signed, user_keypair, expiry_height).await?;
	Ok(axum::Json(board.into()))
}

#[utoipa::path(
	post,
	path = "/board-amount",
	summary = "Board a specific amount",
	request_body = bark_json::web::BoardRequest,
	responses(
		(status = 200, description = "Returns the board result", body = bark_json::cli::PendingBoardInfo),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Moves the specified amount of bitcoin in the on-chain wallet onto the Ark \
		protocol. Creates and broadcasts a funding transaction, then returns the pending board \
		details. The resulting VTXO is not spendable off-chain until the funding transaction \
		reaches the number of on-chain confirmations required by the Ark server.",
	tag = "boards"
)]
#[debug_handler]
pub async fn board_amount(
	State(state): State<Arc<ServerState>>,
	Json(body): Json<bark_json::web::BoardRequest>,
) -> HandlerResult<Json<bark_json::cli::PendingBoardInfo>> {
	let wallet = state.require_wallet()?;
	let amount = Amount::from_sat(body.amount_sat);
	let fee_rate = board_fee_rate(body.fee_rate_sat_per_vb)?;
	let board = wallet.board_amount_with_fee_rate(amount, fee_rate).await?;
	Ok(axum::Json(board.into()))
}

/// The funding fee rate a board request asked for, if any. Below the 0.1 sat/vB relay floor
/// the funding transaction would never propagate, so refuse it up front.
pub(crate) fn board_fee_rate(sat_per_vb: Option<f64>) -> HandlerResult<Option<FeeRate>> {
	let Some(v) = sat_per_vb else { return Ok(None) };
	if !(0.1..=10_000.0).contains(&v) {
		badarg!("Fee rate must be between 0.1 and 10000 sat/vB");
	}
	Ok(Some(FeeRate::from_sat_per_vb_decimal_checked_ceil(v)
		.badarg("Fee rate must be finite and non-negative")?))
}

#[utoipa::path(
	post,
	path = "/board-all",
	summary = "Board all on-chain bitcoin",
	request_body(content = Option<bark_json::web::BoardAllRequest>),
	responses(
		(status = 200, description = "Returns the board result", body = bark_json::cli::PendingBoardInfo),
		(status = 500, description = "Internal server error", body = error::InternalServerError)
	),
	description = "Moves all bitcoin in the on-chain wallet onto the Ark protocol. Creates and \
		broadcasts a funding transaction that drains the on-chain balance into a single VTXO, \
		then returns the pending board details. The resulting VTXO is not spendable off-chain \
		until the funding transaction reaches the number of on-chain confirmations required by \
		the Ark server.",
	tag = "boards"
)]
#[debug_handler]
pub async fn board_all(
	State(state): State<Arc<ServerState>>,
	body: Option<Json<bark_json::web::BoardAllRequest>>,
) -> HandlerResult<Json<bark_json::cli::PendingBoardInfo>> {
	let wallet = state.require_wallet()?;
	let fee_rate = board_fee_rate(body.and_then(|Json(b)| b.fee_rate_sat_per_vb))?;
	let board = wallet.board_all_with_fee_rate(fee_rate).await?;
	Ok(axum::Json(board.into()))
}

#[utoipa::path(
	get,
	path = "/pending",
	summary = "List pending boards",
	responses(
		(status = 200, description = "Returns all pending boards", body = Vec<bark_json::cli::PendingBoardInfo>),
		(status = 500, description = "Internal server error")
	),
	description = "Returns all boards whose funding transactions have not yet reached the \
		number of on-chain confirmations required by the Ark server.",
	tag = "boards"
)]
#[debug_handler]
pub async fn get_pending_boards(
	State(state): State<Arc<ServerState>>,
) -> HandlerResult<Json<Vec<bark_json::cli::PendingBoardInfo>>> {
	let wallet = state.require_wallet()?;

	let boards = wallet.pending_boards().await?.into_iter()
		.map(bark_json::cli::PendingBoardInfo::from).collect();

	Ok(axum::Json(boards))
}
