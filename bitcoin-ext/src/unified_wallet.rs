//! Explicit unified signer for Bark's `tr(key)` BDK wallets.
//! Unsupported descriptors fail closed; there is no call to BDK's legacy signer.

use bdk_wallet::{KeychainKind, Wallet};
use bdk_wallet::miniscript::descriptor::DescriptorSecretKey;
use bitcoin::key::TapTweak;
use bitcoin::secp256k1::{Keypair, Secp256k1};
use bitcoin::{Psbt, ScriptBuf, Witness};

use crate::unified::{self, Execution};

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("missing or conflicting previous output")]
	Prevout,
	#[error("unsupported wallet input or signature mode")]
	Unsupported,
	#[error(transparent)]
	Digest(#[from] unified::Error),
}

pub fn sign(wallet: &Wallet, psbt: &mut Psbt) -> Result<bool, Error> {
	if psbt.inputs.len() != psbt.unsigned_tx.input.len() { return Err(Error::Prevout); }
	let secp = Secp256k1::new();
	let mut keys = wallet.get_signers(KeychainKind::External).as_key_map(&secp);
	keys.extend(wallet.get_signers(KeychainKind::Internal).as_key_map(&secp));
	let prevouts = psbt.inputs.iter().zip(&psbt.unsigned_tx.input).map(|(p, vin)| {
		let out = p.witness_utxo.clone().ok_or(Error::Prevout)?;
		if let Some(tx) = &p.non_witness_utxo {
			if tx.compute_txid() != vin.previous_output.txid
				|| tx.output.get(vin.previous_output.vout as usize) != Some(&out) {
				return Err(Error::Prevout);
			}
		}
		Ok(out)
	}).collect::<Result<Vec<_>, Error>>()?;
	// Work on a copy: any error leaves the caller's PSBT untouched.
	let mut result = psbt.clone();
	for (idx, input) in result.inputs.iter_mut().enumerate() {
		if input.sighash_type.is_some_and(|s| s.to_u32() != u32::from(unified::ALL)) {
			return Err(Error::Unsupported);
		}
		if let Some(witness) = &input.final_script_witness {
			// Profile 2 public fee anchor has no signature. Permit only its exact
			// witness script and matching prevout, not arbitrary unsigned inputs.
			let anchor = crate::fee::standard_anchor_script();
			if prevouts[idx].script_pubkey == anchor.to_p2wsh()
				&& *witness == bitcoin::Witness::from_slice(&[anchor.as_bytes()]) {
				continue;
			}
			if prevouts[idx].script_pubkey == ScriptBuf::new_p2a() && witness.is_empty() {
				continue;
			}
			// Foreign exit inputs are signed by the Ark signer. Check all
			// signature-sized stack elements, excluding script/control block.
			if !prevouts[idx].script_pubkey.is_p2tr() { return Err(Error::Unsupported); }
			let stack = witness.iter().collect::<Vec<_>>();
			// Annexes are not produced by our wallet and require separate parsing.
			if stack.len() == 2 || stack.last().is_some_and(|s| s.first() == Some(&0x50)) {
				return Err(Error::Unsupported);
			}
			let stack = if stack.len() >= 3 { &stack[..stack.len()-2] } else { &stack[..] };
			if !stack.iter().any(|s| s.len() == 65 && s[64] == unified::ALL)
				|| stack.iter().any(|s| s.len() == 64
				|| (s.len() == 65 && s[64] != unified::ALL)) {
				return Err(Error::Unsupported);
			}
			continue;
		}
		if input.sighash_type.is_some_and(|s| s.to_u32() != u32::from(unified::ALL)) {
			return Err(Error::Unsupported);
		}
		let internal = input.tap_internal_key.ok_or(Error::Unsupported)?;
		if input.tap_merkle_root.is_some() || !input.tap_scripts.is_empty() {
			return Err(Error::Unsupported);
		}
		if prevouts[idx].script_pubkey != ScriptBuf::new_p2tr(&secp, internal, None) {
			return Err(Error::Prevout);
		}
		let (_, (fingerprint, path)) = input.tap_key_origins.get(&internal).ok_or(Error::Unsupported)?;
		let mut keypair = None;
		for secret in keys.values() {
			let xkey = match secret { DescriptorSecretKey::XPrv(k) => k, _ => continue };
			let (expected, prefix) = match &xkey.origin {
				Some((fp, p)) => (*fp, p.as_ref()),
				None => (xkey.xkey.fingerprint(&secp), &[][..]),
			};
			let full = path.as_ref();
			if *fingerprint != expected || !full.starts_with(prefix) { continue; }
			let derived = xkey.xkey.derive_priv(&secp, &&full[prefix.len()..])
				.map_err(|_| Error::Unsupported)?;
			let pair = Keypair::from_secret_key(&secp, &derived.private_key);
			if pair.x_only_public_key().0 == internal { keypair = Some(pair); break; }
		}
		let pair = keypair.ok_or(Error::Unsupported)?.tap_tweak(&secp, None).to_keypair();
		let hash = unified::digest(&psbt.unsigned_tx, idx, &prevouts, unified::ALL,
			Execution { script_type: 2, script_code: None, annex: None, leaf: None })?;
		let sig = secp.sign_schnorr(&hash.into(), &pair);
		input.sighash_type = Some(bitcoin::psbt::PsbtSighashType::from_u32(u32::from(unified::ALL)));
		input.final_script_witness = Some(Witness::from_slice(&[unified::signature(&sig)]));
	}
	*psbt = result;
	Ok(true)
}

#[cfg(test)]
mod tests {
	use super::*;
	use bdk_wallet::chain::BlockId;
	use bdk_wallet::test_utils::{insert_checkpoint, receive_output_in_latest_block};
	use bitcoin::{Amount, BlockHash, Network};
	use bitcoin::hashes::Hash;

	#[test]
	fn standard_v2_anchor_signing_checks_exact_witness() {
		use crate::bdk::TxBuilderExt;
		let master = bitcoin::bip32::Xpriv::new_master(Network::Regtest, &[43; 32]).unwrap();
		let mut wallet = Wallet::create(
			format!("tr({master}/0/*)"), format!("tr({master}/1/*)"))
			.network(Network::Regtest).create_wallet_no_persist().unwrap();
		insert_checkpoint(&mut wallet, BlockId { height: 1000, hash: BlockHash::all_zeros() });
		receive_output_in_latest_block(&mut wallet, Amount::from_sat(100_000));
		let address = wallet.reveal_next_address(KeychainKind::External).address;
		let anchor = crate::fee::ExitFormat::StandardV2.anchor(Amount::from_sat(1000));
		let point = bitcoin::OutPoint::new(bitcoin::Txid::all_zeros(), 0);
		let mut builder = wallet.build_tx();
		builder.version(2);
		builder.only_witness_utxo();
		builder.add_fee_anchor_spend(point, &anchor);
		builder.add_recipient(address.script_pubkey(), Amount::from_sat(50_000));
		builder.fee_absolute(Amount::from_sat(1000));
		let psbt = builder.finish().unwrap();
		let index = psbt.unsigned_tx.input.iter().position(|i| i.previous_output == point).unwrap();
		let mut invalid = psbt.clone();
		invalid.inputs[index].final_script_witness = Some(bitcoin::Witness::new());
		assert!(sign(&wallet, &mut invalid).is_err());
		let mut signed = psbt;
		assert!(sign(&wallet, &mut signed).unwrap());
		let tx = signed.extract_tx().unwrap();
		assert_eq!(tx.version, bitcoin::transaction::Version::TWO);
		assert_eq!(tx.input[index].witness,
			bitcoin::Witness::from_slice(&[crate::fee::standard_anchor_script().as_bytes()]));
	}

	#[test]
	fn unified_wallet_signs_and_rejects_legacy_request_atomically() {
		let master = bitcoin::bip32::Xpriv::new_master(Network::Regtest, &[42; 32]).unwrap();
		let mut wallet = Wallet::create(
			format!("tr({master}/0/*)"), format!("tr({master}/1/*)"))
			.network(Network::Regtest).create_wallet_no_persist().unwrap();
		insert_checkpoint(&mut wallet, BlockId { height: 1000, hash: BlockHash::all_zeros() });
		receive_output_in_latest_block(&mut wallet, Amount::from_sat(100_000));
		receive_output_in_latest_block(&mut wallet, Amount::from_sat(100_000));
		let address = wallet.reveal_next_address(KeychainKind::External).address;
		let mut builder = wallet.build_tx();
		builder.add_recipient(address.script_pubkey(), Amount::from_sat(150_000));
		builder.fee_absolute(Amount::from_sat(1000));
		let psbt = builder.finish().unwrap();
		assert_eq!(psbt.inputs.len(), 2);
		let mut invalid = psbt.clone();
		invalid.inputs[1].sighash_type = Some(bitcoin::TapSighashType::Default.into());
		let before = invalid.clone();
		assert!(sign(&wallet, &mut invalid).is_err());
		assert_eq!(invalid, before);
		let mut signed = psbt;
		assert!(sign(&wallet, &mut signed).unwrap());
		let satisfaction = wallet.public_descriptor(KeychainKind::External)
			.max_weight_to_satisfy().unwrap() + bitcoin::Weight::from_wu(1);
		let expected_weight = signed.unsigned_tx.weight()
			+ satisfaction * signed.inputs.len() as u64 + bitcoin::Weight::from_wu(2);
		assert_eq!(signed.clone().extract_tx().unwrap().weight(), expected_weight);
		let prevouts = signed.inputs.iter().map(|p| p.witness_utxo.clone().unwrap()).collect::<Vec<_>>();
		for (idx, input) in signed.inputs.iter().enumerate() {
			let witness = input.final_script_witness.as_ref().unwrap();
			assert_eq!(witness.len(), 1);
			let sig = witness.iter().next().unwrap();
			assert_eq!(sig.len(), 65);
			assert_eq!(sig[64], unified::ALL);
			let hash = unified::digest(&signed.unsigned_tx, idx, &prevouts, unified::ALL,
				Execution { script_type: 2, script_code: None, annex: None, leaf: None }).unwrap();
			let pubkey = bitcoin::secp256k1::XOnlyPublicKey::from_slice(&prevouts[idx].script_pubkey.as_bytes()[2..]).unwrap();
			Secp256k1::new().verify_schnorr(&bitcoin::secp256k1::schnorr::Signature::from_slice(&sig[..64]).unwrap(), &hash.into(), &pubkey).unwrap();
		}
	}
}
