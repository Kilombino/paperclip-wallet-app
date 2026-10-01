//! Fee estimation for various wallet operations.

use anyhow::Context;
use bitcoin::Amount;

use ark::{Vtxo, VtxoId};
use ark::fees::VtxoFeeInfo;

use crate::Wallet;
use crate::vtxo::selection::InputSelection;

/// Result of a fee estimation containing the total cost, fee amount, and VTXOs used. It's very
/// important to consider that fees can change over time, so you should expect to renew this
/// estimate frequently when presenting this information to users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeEstimate {
	/// The total amount including fees.
	pub gross_amount: Amount,
	/// The additional cost. For funded Ark sends this includes the recovery reserve.
	pub fee: Amount,
	/// The amount excluding fees. For sends, this is the amount the recipient
	/// receives. For receives, this is the amount the user gets.
	pub net_amount: Amount,
	/// The VTXOs that would be used for this operation, if necessary.
	pub vtxos_spent: Vec<VtxoId>,
}

impl FeeEstimate {
	pub fn new(
		gross_amount: Amount,
		fee: Amount,
		net_amount: Amount,
		vtxos_spent: Vec<VtxoId>,
	) -> Self {
		Self {
			gross_amount,
			fee,
			net_amount,
			vtxos_spent,
		}
	}
}

impl Wallet {
	/// Estimate fees for a board operation. `FeeEstimate::net_amount` will be the amount of the
	/// newly boarded VTXO. Note: This doesn't include the onchain cost of creating the chain
	/// anchor transaction.
	pub async fn estimate_board_offchain_fee(
		&self,
		board_amount: Amount,
	) -> anyhow::Result<FeeEstimate> {
		let (_, ark_info) = self.require_server().await?;

		if board_amount < ark_info.min_board_amount {
			bail!("board amount of {} does not meet minimum value of {}",
				board_amount, ark_info.min_board_amount,
			);
		}
		if let Some(max) = ark_info.max_vtxo_amount {
			if board_amount > max {
				bail!("board amount of {} exceeds maximum value of {}", board_amount, max);
			}
		}

		let fee = ark_info.fees.board.calculate(board_amount).context("fee overflowed")?;
		let net_amount = board_amount.checked_sub(fee).unwrap_or(Amount::ZERO);

		Ok(FeeEstimate::new(board_amount, fee, net_amount, vec![]))
	}

	/// Estimate the recovery reserve using the same input selection and package builder as a send.
	/// This does not lock inputs, store keys, or request signatures. Selection errors propagate.
	pub async fn estimate_arkoor_payment_fee(&self, amount: Amount) -> anyhow::Result<FeeEstimate> {
		let (change, _) = self.peek_next_keypair().await?;
		let policy = ark::VtxoPolicy::new_pubkey(change.public_key());
		let (inputs, reserve) = self.plan_arkoor_payment(amount, policy, change.public_key()).await?;
		Ok(FeeEstimate::new(
			amount.checked_add(reserve).context("payment amount overflow")?, reserve, amount,
			inputs.iter().map(|v| v.id()).collect(),
		))
	}

	/// Validate the destination and estimate an Ark send without changing wallet state.
	pub async fn estimate_arkoor_send(
		&self, destination: &ark::Address, amount: Amount,
	) -> anyhow::Result<FeeEstimate> {
		self.validate_arkoor_address(destination).await?;
		let (change, _) = self.peek_next_keypair().await?;
		ensure!(destination.policy().user_pubkey() != change.public_key(),
			"Cannot create arkoor to same address as change");
		let (inputs, reserve) = self.plan_arkoor_payment(
			amount, destination.policy().clone(), change.public_key(),
		).await?;
		Ok(FeeEstimate::new(
			amount.checked_add(reserve).context("payment amount overflow")?, reserve, amount,
			inputs.iter().map(|v| v.id()).collect(),
		))
	}

	/// Estimate fees for a lightning receive operation. `FeeEstimate::gross_amount` is the
	/// lightning payment amount, `FeeEstimate::net_amount` is how much the end user will receive.
	pub async fn estimate_lightning_receive_fee(
		&self,
		amount: Amount,
	) -> anyhow::Result<FeeEstimate> {
		let (_, ark_info) = self.require_server().await?;

		if let Some(max) = ark_info.max_vtxo_amount {
			if amount > max {
				bail!("amount of {} exceeds maximum value of {}", amount, max);
			}
		}

		let fee = ark_info.fees.lightning_receive.calculate(amount).context("fee overflowed")?;
		// Minimum estimate for one HTLC; each additional input reserves the same claim cost.
		let fee = fee.checked_add(ark::exit_policy::paperclip_funding().per_transaction() * 2)
			.context("fee overflow")?;
		let net_amount = amount.checked_sub(fee).unwrap_or(Amount::ZERO);

		Ok(FeeEstimate::new(amount, fee, net_amount, vec![]))
	}

	/// Estimate fees for a lightning send operation. `FeeEstimate::net_amount` is the amount to be
	/// paid to a given invoice/address.
	///
	/// Uses the same builder-validated input selection as the actual send. Errors
	/// are propagated rather than replaced by a hypothetical fee for an unbuildable payment.
	pub async fn estimate_lightning_send_fee(&self, amount: Amount) -> anyhow::Result<FeeEstimate> {
		let (_, info) = self.require_server().await?;
		let tip = self.inner.chain.tip().await?;
		let (key, _) = self.peek_next_keypair().await?;
		let policy = ark::VtxoPolicy::new_server_htlc_send(key.public_key(),
			ark::lightning::PaymentHash::from_byte_array([0; 32]), tip + info.htlc_send_expiry_delta);
		let (inputs, fee, reserve) = self.plan_lightning_payment(amount, policy, key.public_key()).await?;
		let fee = fee.checked_add(reserve).context("Lightning fee overflow")?;
		let total = amount.checked_add(fee).context("payment amount overflow")?;
		Ok(FeeEstimate::new(total, fee, amount, inputs.iter().map(|v| v.id()).collect()))
	}

	/// Estimate fees for an offboard operation. `FeeEstimate::net_amount` is the onchain amount the
	/// user can expect to receive by offboarding `FeeEstimate::vtxos_used`.
	pub async fn estimate_offboard<G>(
		&self,
		address: &bitcoin::Address,
		vtxos: impl IntoIterator<Item = impl AsRef<Vtxo<G>>>,
	) -> anyhow::Result<FeeEstimate> {
		let (srv, ark_info) = self.require_server().await?;
		let offboard_feerate = srv.offboard_feerate().await?;
		let script_buf = address.script_pubkey();
		let current_height = self.inner.chain.tip().await?;

		let vtxos = vtxos.into_iter();
		let capacity = vtxos.size_hint().1.unwrap_or(vtxos.size_hint().0);
		let mut vtxo_ids = Vec::with_capacity(capacity);
		let mut fee_info = Vec::with_capacity(capacity);
		let mut amount = Amount::ZERO;
		for vtxo in vtxos {
			let vtxo = vtxo.as_ref();
			vtxo_ids.push(vtxo.id());
			fee_info.push(VtxoFeeInfo::from_vtxo_and_tip(vtxo, current_height));
			amount = amount + vtxo.amount();
		}

		let fee = ark_info.fees.offboard.calculate(
			&script_buf,
			amount,
			offboard_feerate,
			fee_info,
		).context("Error whilst calculating offboard fee")?;

		let net_amount = amount.checked_sub(fee).unwrap_or(Amount::ZERO);
		Ok(FeeEstimate::new(amount, fee, net_amount, vtxo_ids))
	}

	/// Estimate fees for offboarding the entire Ark balance to a given address.
	/// Uses the same fee calculation as `offboard_all`.
	pub async fn estimate_offboard_all(
		&self,
		address: &bitcoin::Address,
	) -> anyhow::Result<FeeEstimate> {
		let vtxos = self.spendable_vtxos().await?;
		self.estimate_offboard(address, &vtxos).await
	}

	/// Estimate fees for a refresh operation (round participation). `FeeEstimate::net_amount` is
	/// the sum of the newly refreshed VTXOs.
	pub async fn estimate_refresh_fee<G>(
		&self,
		vtxos: impl IntoIterator<Item = impl AsRef<Vtxo<G>>>,
	) -> anyhow::Result<FeeEstimate> {
		let (_, ark_info) = self.require_server().await?;
		let current_height = self.inner.chain.tip().await?;

		let vtxos = vtxos.into_iter();
		let capacity = vtxos.size_hint().1.unwrap_or(vtxos.size_hint().0);
		let mut vtxo_ids = Vec::with_capacity(capacity);
		let mut vtxo_fee_infos = Vec::with_capacity(capacity);
		let mut total_amount = Amount::ZERO;
		for vtxo in vtxos.into_iter() {
			let vtxo = vtxo.as_ref();
			vtxo_ids.push(vtxo.id());
			vtxo_fee_infos.push(VtxoFeeInfo::from_vtxo_and_tip(vtxo, current_height));
			total_amount = total_amount + vtxo.amount();
		}

		if let Some(max) = ark_info.max_vtxo_amount {
			if total_amount > max {
				bail!("total refresh amount of {} exceeds maximum value of {}", total_amount, max);
			}
		}

		// Calculate refresh fees
		let fee = ark_info.fees.refresh.calculate(vtxo_fee_infos).context("fee overflowed")?;
		let output_amount = total_amount.checked_sub(fee).unwrap_or(Amount::ZERO);
		Ok(FeeEstimate::new(total_amount, fee, output_amount, vtxo_ids))
	}

	/// Estimate fees for a send-onchain operation. `FeeEstimate::net_amount` is the onchain amount
	/// the user will receive and `FeeEstimate::gross_amount` is the offchain amount the user will
	/// pay using `FeeEstimate::vtxos_used`.
	///
	/// Uses the same iterative approach as `send_onchain` to account for VTXO expiry-based fees.
	///
	/// If the wallet is lacking enough funds to send `amount` onchain, then the estimate will be
	/// the maximum possible fee, assuming the user acquires enough funds to cover the payment.
	pub async fn estimate_send_onchain(
		&self,
		address: &bitcoin::Address,
		amount: Amount,
	) -> anyhow::Result<FeeEstimate> {
		let (srv, ark_info) = self.require_server().await?;
		let offboard_feerate = srv.offboard_feerate().await?;
		let script_buf = address.script_pubkey();

		let selection = InputSelection::new()
			.max_inputs(srv.ark_info().await.max_offboard_inputs)
			.fee_scheme(self.inner.chain.tip().await?, |a, v|
				ark_info.fees.offboard.calculate(&script_buf, a, offboard_feerate, v)
					.ok_or_else(|| anyhow!("Error whilst calculating fee")),
			);
		let (inputs, fee) = match selection.select(self.spendable_vtxos().await?, amount) {
			Ok((inputs, fee)) => (inputs, fee),
			Err(_) => {
				// We choose to ignore every error, even those which are not due to insufficient
				// funds.
				let info = [VtxoFeeInfo { amount, expiry_blocks: u32::MAX }];
				let fee = ark_info.fees.offboard.calculate(
					&script_buf, amount, offboard_feerate, info,
				).context("fee overflowed")?;
				(Vec::new(), fee)
			}
		};

		let total_cost = amount.checked_add(fee).unwrap_or(Amount::MAX);
		let vtxo_ids = inputs.into_iter().map(|v| v.id()).collect();

		Ok(FeeEstimate::new(total_cost, fee, amount, vtxo_ids))
	}

	/// Estimate the onchain fees to unilaterally (emergency) exit the given VTXOs.
	///
	/// This is a thin wrapper over [crate::exit::Exit::estimate_emergency_exit_fee]; see it for the
	/// meaning of the returned breakdown and the parameters.
	pub async fn estimate_emergency_exit_fee(
		&self,
		vtxos: &[VtxoId],
		fee_rate: Option<bitcoin::FeeRate>,
		destination: Option<bitcoin::Address>,
		fee_margin: Option<f64>,
	) -> anyhow::Result<crate::exit::ExitFeeEstimate, crate::exit::ExitError> {
		self.exit_mgr()
			.estimate_emergency_exit_fee(vtxos, self, fee_rate, destination, fee_margin)
			.await
	}
}
