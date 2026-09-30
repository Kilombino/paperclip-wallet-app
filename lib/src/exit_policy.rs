//! Checks for a fully signed, funded recovery path under a specified relay policy.
//! These checks are admission checks, not a promise of future block inclusion.

use bitcoin::{Amount, FeeRate, Transaction};
use bitcoin_ext::{BlockDelta, BlockHeight};

use crate::{Vtxo, VtxoPolicy};
pub use bitcoin_ext::fee::ExitFormat;
use crate::vtxo::Full;
use crate::lightning::Preimage;

#[derive(Debug, Clone, Copy)]
pub struct FundedExitPolicy {
	pub minimum_relay: FeeRate,
	pub dust_relay: FeeRate,
	/// Additional blocks beyond one confirmation per ancestor and the CSV delay.
	pub confirmation_margin: BlockDelta,
	/// Reserve for the final claim, deducted from the recovered output.
	pub claim_fee: Amount,
}

pub const PAPERCLIP_EXIT_PROFILE: u32 = 2;

/// Fixed reserves for the private regtest profile; not a future fee guarantee.
pub fn paperclip_funding() -> crate::tree::signed::TreeExitFunding {
	crate::tree::signed::TreeExitFunding::new(Amount::from_sat(1000), Amount::from_sat(1000))
		.expect("constant funded profile is valid")
		.with_format(bitcoin_ext::fee::ExitFormat::StandardV2)
}

pub fn paperclip_policy() -> FundedExitPolicy {
	FundedExitPolicy {
		minimum_relay: FeeRate::from_sat_per_kwu(250),
		dust_relay: FeeRate::from_sat_per_kwu(750),
		confirmation_margin: BlockDelta::new(12),
		claim_fee: Amount::from_sat(1000),
	}
}

impl FundedExitPolicy {
	/// Check an ordinary pubkey balance before accepting it as recoverable.
	/// HTLC recovery has extra conditions and needs separate checks.
	pub fn check(
		&self,
		vtxo: &Vtxo<Full>,
		funding: &Transaction,
		tip: BlockHeight,
	) -> Result<(), String> {
		if !matches!(vtxo.policy(), VtxoPolicy::Pubkey(..)) {
			return Err("funded recovery check only supports ordinary pubkey balances".into());
		}
		let deadline = self.recovery_deadline(vtxo, tip, u32::from(vtxo.exit_delta().to_u16()), None)?;
		self.check_path(vtxo, funding, deadline)
	}

	/// Check the user's outgoing HTLC refund path, including both timelocks.
	pub fn check_lightning_send(
		&self, vtxo: &Vtxo<Full>, funding: &Transaction, tip: BlockHeight,
	) -> Result<(), String> {
		let VtxoPolicy::ServerHtlcSend(policy) = vtxo.policy() else {
			return Err("expected a current outgoing Lightning HTLC".into());
		};
		let delay = u32::from(vtxo.exit_delta().to_u16()).checked_mul(2)
			.filter(|d| *d <= u32::from(u16::MAX)).ok_or("HTLC refund delay overflow")?;
		if policy.htlc_expiry.to_u32() >= 500_000_000 {
			return Err("HTLC expiry must be a block height".into());
		}
		let deadline = self.recovery_deadline(vtxo, tip, delay, Some(policy.htlc_expiry))?;
		self.check_path(vtxo, funding, deadline)
	}

	/// Check the entire incoming recovery path before revealing the preimage.
	/// The user must confirm a claim before the server's absolute HTLC timeout.
	pub fn check_lightning_receive(
		&self, vtxo: &Vtxo<Full>, funding: &Transaction, tip: BlockHeight, preimage: &Preimage,
	) -> Result<(), String> {
		let VtxoPolicy::ServerHtlcRecv(policy) = vtxo.policy() else {
			return Err("expected a current incoming Lightning HTLC".into());
		};
		if preimage.compute_payment_hash() != policy.payment_hash {
			return Err("preimage does not match incoming HTLC".into());
		}
		let delay = u32::from(vtxo.exit_delta().to_u16())
			.checked_add(u32::from(policy.htlc_expiry_delta.to_u16()))
			.filter(|d| *d <= u32::from(u16::MAX)).ok_or("HTLC claim delay overflow")?;
		let deadline = self.recovery_deadline(vtxo, tip, delay, None)?;
		if policy.htlc_expiry.to_u32() >= 500_000_000 || deadline >= policy.htlc_expiry.to_u32() {
			return Err("insufficient time to claim before the incoming HTLC timeout".into());
		}
		self.check_path(vtxo, funding, deadline)
	}

	fn recovery_deadline(
		&self, vtxo: &Vtxo<Full>, tip: BlockHeight, delay: u32, absolute: Option<BlockHeight>,
	) -> Result<u32, String> {
		if self.minimum_relay.to_sat_per_kwu() == 0 || self.confirmation_margin.to_u16() == 0
			|| self.dust_relay.to_sat_per_kwu() > 25_000_000 || self.claim_fee == Amount::ZERO {
			return Err("invalid exit policy bounds".into());
		}
		let steps = u32::try_from(vtxo.genesis.items.len()).map_err(|_| "exit depth overflow")?;
		let relative = tip.to_u32().checked_add(steps).and_then(|h| h.checked_add(delay))
			.ok_or("exit deadline overflow")?;
		relative.max(absolute.map(|h| h.to_u32()).unwrap_or(0))
			.checked_add(u32::from(self.confirmation_margin.to_u16()))
			.ok_or_else(|| "exit deadline overflow".into())
	}

	fn check_path(&self, vtxo: &Vtxo<Full>, funding: &Transaction, deadline: u32) -> Result<(), String> {
		if deadline >= vtxo.expiry_height().to_u32() {
			return Err("insufficient time to complete recovery before expiry".into());
		}
		vtxo.validate(funding).map_err(|e| e.to_string())?;
		let mut prev = funding.output.get(vtxo.chain_anchor().vout as usize)
			.ok_or("missing funding output")?.clone();
		for item in vtxo.transactions() {
			let total = item.tx.output.iter().try_fold(Amount::ZERO, |sum, o| {
				if o.value < o.script_pubkey.minimal_non_dust_custom(self.dust_relay) { return None; }
				sum.checked_add(o.value)
			}).ok_or("dust output or output value overflow in recovery path")?;
			let fee = prev.value.checked_sub(total).ok_or("unfunded recovery transaction")?;
			let required = (item.tx.vsize() as u64).checked_mul(4)
				.and_then(|w| w.checked_mul(self.minimum_relay.to_sat_per_kwu()))
				.and_then(|v| v.checked_add(999)).map(|v| v / 1000)
				.ok_or("relay fee overflow")?;
			if fee.to_sat() < required { return Err("recovery parent is below its individual relay floor".into()); }
			prev = item.tx.output.get(item.output_idx).ok_or("invalid recovery output index")?.clone();
		}
		let claim_floor = prev.script_pubkey.minimal_non_dust_custom(self.dust_relay);
		if prev.value.checked_sub(self.claim_fee).is_none_or(|a| a < claim_floor) {
			return Err("insufficient balance for a non-dust final claim after fees".into());
		}
		Ok(())
	}
}
