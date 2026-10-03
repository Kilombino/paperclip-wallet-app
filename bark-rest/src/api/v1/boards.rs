use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{debug_handler, Json, Router};
use bitcoin::{Amount, FeeRate};
use bitcoin_ext::FeeRateExt;
use utoipa::OpenApi;

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
fn board_fee_rate(sat_per_vb: Option<f64>) -> HandlerResult<Option<FeeRate>> {
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
