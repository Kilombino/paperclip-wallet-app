
use bitcoin::{ScriptBuf, TxOut, Weight};
use bitcoin::Amount;


/// The size in bytes of a fee anchor created with P2A script.
pub const FEE_ANCHOR_WEIGHT: Weight = Weight::from_vb_unchecked(13);

/// The witness size of a witness spending a dust anchor.
pub const FEE_ANCHOR_SPEND_WEIGHT: Weight = Weight::from_wu(1);


lazy_static! {
	/// A pay-to-anchor (p2a) output script.
	pub static ref P2A_SCRIPT: ScriptBuf = ScriptBuf::new_p2a();
}

/// Signed recovery format. Never reinterpret a legacy signed transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitFormat {
	#[default]
	LegacyV3,
	StandardV2,
}

impl ExitFormat {
	pub fn code(self) -> u8 { match self { Self::LegacyV3 => 0, Self::StandardV2 => 1 } }
	pub fn from_code(code: u8) -> Result<Self, &'static str> {
		match code { 0 => Ok(Self::LegacyV3), 1 => Ok(Self::StandardV2), _ => Err("unknown exit format") }
	}
	pub fn version(self) -> bitcoin::transaction::Version {
		bitcoin::transaction::Version(match self { Self::LegacyV3 => 3, Self::StandardV2 => 2 })
	}
	pub fn anchor(self, value: Amount) -> TxOut {
		match self {
			Self::LegacyV3 => fee_anchor_with_amount(value),
			Self::StandardV2 => TxOut { value, script_pubkey: standard_anchor_script().to_p2wsh() },
		}
	}
}

/// Public fee-bump output using ordinary SegWit v0 rather than P2A/TRUC.
/// This is not a protected user balance; anyone may spend it.
pub fn standard_anchor_script() -> ScriptBuf { ScriptBuf::from_bytes(vec![0x51]) }

/// Create a p2a fee anchor output with the given amount.
pub fn fee_anchor_with_amount(amount: Amount) -> TxOut {
	TxOut {
		script_pubkey: P2A_SCRIPT.clone(),
		value: amount,
	}
}

/// Create a p2a fee anchor output with 0 value.
pub fn fee_anchor() -> TxOut {
	fee_anchor_with_amount(Amount::ZERO)
}

#[cfg(test)]
mod test {
	use bitcoin::{absolute::Height, psbt, transaction::Version, Transaction, TxIn, Witness};

use super::*;

	#[test]
	fn test_dust_fee_anchor_size() {
		assert_eq!(
			FEE_ANCHOR_WEIGHT,
			Weight::from_vb(bitcoin::consensus::serialize(&fee_anchor()).len() as u64).unwrap(),
		);
	}

	#[test]
	fn test_fee_anchor_spend_weight() {
		let psbt_in = psbt::Input {
			witness_utxo: Some(fee_anchor()),
			final_script_witness: Some(Witness::new()),
			..Default::default()
		};

		let psbt = psbt::Psbt {
			inputs: vec![psbt_in],
			outputs: vec![],

			unsigned_tx: Transaction {
				version: Version::TWO,
				lock_time: bitcoin::absolute::LockTime::Blocks(Height::ZERO),
				input: vec![TxIn::default()],
				output: vec![],
			},
			xpub: Default::default(),
			version: 0,
			proprietary: Default::default(),
			unknown: Default::default(),
		};

		assert_eq!(
			psbt.extract_tx().unwrap().input[0].witness.size() as u64,
			FEE_ANCHOR_SPEND_WEIGHT.to_wu(),
		);
	}
}

