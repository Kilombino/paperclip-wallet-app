//! BIP322-simple Taproot address ownership proofs. These use the standard
//! Bitcoin proof format, not XBT transaction signatures. The virtual transaction
//! is constructed here and cannot spend coins; callers cannot supply a PSBT.

use anyhow::{ensure, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use bitcoin::{absolute, transaction, Address, Amount, OutPoint, Psbt, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::{schnorr, Message, Secp256k1, XOnlyPublicKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};

use super::OnchainWallet;

pub const MESSAGE_MAX_BYTES: usize = 4096;

fn proof_transaction(address: &Address, message: &str) -> anyhow::Result<(Transaction, TxOut)> {
	ensure!(address.script_pubkey().is_p2tr(), "Only Taproot on-chain addresses are supported");
	ensure!(message.len() <= MESSAGE_MAX_BYTES, "Message exceeds 4096 UTF-8 bytes");
	let tag = sha256::Hash::hash(b"BIP0322-signed-message");
	let mut engine = sha256::Hash::engine();
	engine.input(tag.as_byte_array());
	engine.input(tag.as_byte_array());
	engine.input(message.as_bytes());
	let hash = sha256::Hash::from_engine(engine);
	let prevout = TxOut { value: Amount::ZERO, script_pubkey: address.script_pubkey() };
	let spend = Transaction {
		version: transaction::Version(0), lock_time: absolute::LockTime::ZERO,
		input: vec![TxIn {
			previous_output: OutPoint::null(), sequence: Sequence::ZERO,
			script_sig: ScriptBuf::builder().push_int(0).push_slice(hash.to_byte_array()).into_script(),
			witness: Witness::new(),
		}],
		output: vec![prevout.clone()],
	};
	let sign = Transaction {
		version: transaction::Version(0), lock_time: absolute::LockTime::ZERO,
		input: vec![TxIn { previous_output: OutPoint::new(spend.compute_txid(), 0),
			sequence: Sequence::ZERO, script_sig: ScriptBuf::new(), witness: Witness::new() }],
		output: vec![TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::builder()
			.push_opcode(bitcoin::opcodes::all::OP_RETURN).into_script() }],
	};
	Ok((sign, prevout))
}

impl OnchainWallet {
	/// Sign exact UTF-8 bytes with an owned, known Taproot address. No wallet
	/// state is changed and nothing is broadcast, even when the wallet is empty.
	pub fn sign_onchain_message(&self, address: &Address, message: &str) -> anyhow::Result<String> {
		sign_with_wallet(&self.inner, address, message)
	}
}

fn sign_with_wallet(wallet: &bdk_wallet::Wallet, address: &Address, message: &str) -> anyhow::Result<String> {
	ensure!(wallet.derivation_of_spk(address.script_pubkey()).is_some(), "Address is not owned by this wallet");
	let (tx, prevout) = proof_transaction(address, message)?;
	let mut psbt = Psbt::from_unsigned_tx(tx)?;
	psbt.inputs[0].witness_utxo = Some(prevout);
	psbt.inputs[0].sighash_type = Some(TapSighashType::Default.into());
	// Deliberately use the standard signer ONLY for this fixed BIP322 proof.
	// Real XBT transactions continue to use unified_wallet::sign.
	ensure!(wallet.sign(&mut psbt, Default::default())?, "Unable to sign address proof");
	let witness = psbt.inputs[0].final_script_witness.as_ref().context("Missing proof witness")?;
	let signature = format!("smp{}", STANDARD.encode(serialize(witness)));
	ensure!(verify_onchain_message(address, message, &signature)?, "Generated proof did not verify");
	Ok(signature)
}

/// Verify standard BIP322-simple key-path Taproot proofs. Accept the historical
/// unprefixed encoding too, but not legacy compact ECDSA or full proofs.
pub fn verify_onchain_message(address: &Address, message: &str, signature: &str) -> anyhow::Result<bool> {
	let (tx, prevout) = proof_transaction(address, message)?;
	ensure!(signature.len() <= 256, "Signature is too long");
	let bytes = STANDARD.decode(signature.strip_prefix("smp").unwrap_or(signature))?;
	let witness: Witness = deserialize(&bytes)?;
	ensure!(witness.len() == 1, "Only Taproot key-path proofs are supported");
	let signature = bitcoin::taproot::Signature::from_slice(witness.iter().next().context("Empty proof")?)?;
	ensure!(matches!(signature.sighash_type, TapSighashType::Default | TapSighashType::All), "Unsupported proof sighash");
	let key = XOnlyPublicKey::from_slice(&prevout.script_pubkey.as_bytes()[2..])?;
	let hash = SighashCache::new(&tx).taproot_key_spend_signature_hash(0, &Prevouts::All(&[prevout]), signature.sighash_type)?;
	let message = Message::from_digest(hash.to_byte_array());
	let sig: schnorr::Signature = signature.signature;
	Ok(Secp256k1::verification_only().verify_schnorr(&sig, &message, &key).is_ok())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::str::FromStr;
	use bdk_wallet::{KeychainKind, Wallet};
	use bitcoin::{bip32::Xpriv, Network};

	#[test]
	fn onchain_message_published_bip322_vector() {
		// bitcoin/bips bip-0322/basic-test-vectors.json, "No prefix fallback".
		let address = Address::from_str("bc1pss0zhytly75awhm6x2hhvd5lnzv3vssgrf9axfheq8ldyzn88ges79fler").unwrap().require_network(Network::Bitcoin).unwrap();
		let sig = "AUCJYOwOjxYAvatTAGYaVlNXBVyFuc4MwNQkOuK2tl8xhfKDONd0NjfYyNSYcRqeCp8hsAnCEPHAVEkO9h6vbQ/R";
		assert!(verify_onchain_message(&address, "No prefix fallback", sig).unwrap());
		assert!(verify_onchain_message(&address, "No prefix fallback", &format!("smp{sig}")).unwrap());
		assert!(!verify_onchain_message(&address, "No prefix fallback\n", sig).unwrap());
		assert!(verify_onchain_message(&address, "No prefix fallback", "smpAA==").is_err());
	}

	#[test]
	fn onchain_message_signs_empty_wallet_and_binds_exact_bytes() {
		let master = Xpriv::new_master(Network::Regtest, &[42; 32]).unwrap();
		let mut wallet = Wallet::create_single(bdk_wallet::template::Bip86(master, KeychainKind::External))
			.network(Network::Regtest).create_wallet_no_persist().unwrap();
		let address = wallet.reveal_next_address(KeychainKind::External).address;
		let other = wallet.reveal_next_address(KeychainKind::External).address;
		for message in ["", "Paperclip pool\nnonce: 123\n", " UTF-8: Ã© æµ‹è¯• "] {
			let proof = sign_with_wallet(&wallet, &address, message).unwrap();
			assert!(proof.starts_with("smp"));
			assert!(verify_onchain_message(&address, message, &proof).unwrap());
			assert!(!verify_onchain_message(&other, message, &proof).unwrap());
			assert!(!verify_onchain_message(&address, &format!("{message} "), &proof).unwrap());
		}
		assert_eq!(wallet.balance().total(), Amount::ZERO);
		let foreign = Address::from_str("bc1pss0zhytly75awhm6x2hhvd5lnzv3vssgrf9axfheq8ldyzn88ges79fler").unwrap().require_network(Network::Bitcoin).unwrap();
		assert!(sign_with_wallet(&wallet, &foreign, "proof").is_err());
		assert!(sign_with_wallet(&wallet, &address, &"x".repeat(MESSAGE_MAX_BYTES + 1)).is_err());
	}
}
