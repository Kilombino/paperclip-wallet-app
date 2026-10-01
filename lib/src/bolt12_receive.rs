//! Wallet-owned reusable offers. The relay never receives the signing key or
//! payment preimage. Invoice requests are bounded before the LDK parser runs.

use std::io::Cursor;
use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit};
use lightning::sign::KeysManager;
use lightning::onion_message::offers::OffersMessage;
use lightning::onion_message::messenger::{create_onion_message, Destination, OnionMessagePath};
use lightning::blinded_path::EmptyNodeIdLookUp;

use bitcoin::Network;
use bitcoin::constants::ChainHash;
use bitcoin::hashes::{sha256, Hash, HashEngine, Hmac, HmacEngine};
use bitcoin::secp256k1::{ecdh::SharedSecret, Keypair, PublicKey, Scalar, SecretKey};
use lightning::blinded_path::{BlindedHop, message::BlindedMessagePath};
use lightning::blinded_path::payment::{
	BlindedPaymentPath, Bolt12OfferContext, PaymentConstraints, PaymentContext,
	UnauthenticatedReceiveTlvs,
};
use lightning::ln::inbound_payment::ExpandedKey;
use lightning::offers::invoice::{Bolt12Invoice, UnsignedBolt12Invoice};
use lightning::offers::invoice_request::{InvoiceRequest, InvoiceRequestFields};
use lightning::offers::nonce::Nonce;
use lightning::offers::offer::{Amount as OfferAmount, Offer, OfferBuilder};
use lightning::sign::EntropySource;
use lightning::types::payment::PaymentSecret;
use lightning::util::ser::{BigSize, Readable, Writeable};

use crate::lightning::Preimage;
use crate::SECP;

pub const MAX_MESSAGE_BYTES: usize = 16_384;

#[derive(Debug, thiserror::Error)]
#[error("invalid BOLT12 receive request: {0}")]
pub struct ReceiveError(pub String);

fn invalid(message: impl Into<String>) -> ReceiveError { ReceiveError(message.into()) }
fn ldk_error(error: impl std::fmt::Debug) -> ReceiveError { invalid(format!("{error:?}")) }

struct Entropy;
impl EntropySource for Entropy {
	fn get_secure_random_bytes(&self) -> [u8; 32] { rand::random() }
}

/// Return TLVs without allocating from attacker-controlled length fields.
fn records(bytes: &[u8]) -> Result<Vec<(u64, &[u8])>, ReceiveError> {
	if bytes.len() > MAX_MESSAGE_BYTES { return Err(invalid("message too large")); }
	let mut cursor = Cursor::new(bytes);
	let mut result = Vec::new();
	let mut previous = None;
	while cursor.position() < bytes.len() as u64 {
		let kind = BigSize::read(&mut cursor).map_err(ldk_error)?.0;
		if previous.is_some_and(|p| kind <= p) { return Err(invalid("unordered TLVs")); }
		let length = BigSize::read(&mut cursor).map_err(ldk_error)?.0;
		let start = cursor.position() as usize;
		let length = usize::try_from(length).map_err(ldk_error)?;
		let end = start.checked_add(length).ok_or_else(|| invalid("TLV length overflow"))?;
		let value = bytes.get(start..end).ok_or_else(|| invalid("truncated TLV"))?;
		result.push((kind, value));
		previous = Some(kind);
		cursor.set_position(end as u64);
	}
	Ok(result)
}

fn append_record(bytes: &mut Vec<u8>, kind: u64, value: &[u8]) {
	BigSize(kind).write(bytes).expect("Vec write");
	BigSize(value.len() as u64).write(bytes).expect("Vec write");
	bytes.extend_from_slice(value);
}

/// Builders do not expose custom feature setters. Set the registered XBT bit
/// in an unsigned TLV stream, then let LDK parse and sign the complete message.
fn with_xbt_feature(bytes: &[u8], feature_type: u64) -> Result<Vec<u8>, ReceiveError> {
	let fields = records(bytes)?;
	let old = fields.iter().find(|(kind, _)| *kind == feature_type)
		.map(|(_, value)| *value).unwrap_or(&[]);
	let mut flags = vec![0; old.len().max(65)];
	let offset = flags.len() - old.len();
	flags[offset..].copy_from_slice(old);
	let bit_byte = flags.len() - 65;
	flags[bit_byte] |= 1;
	let mut output = Vec::new();
	let mut inserted = false;
	for (kind, value) in fields {
		if !inserted && kind >= feature_type {
			append_record(&mut output, feature_type, &flags);
			inserted = true;
		}
		if kind != feature_type { append_record(&mut output, kind, value); }
	}
	if !inserted { append_record(&mut output, feature_type, &flags); }
	Ok(output)
}

pub fn create_offer(
	issuer: PublicKey, relay: PublicKey, network: Network,
	description: String, amount_sat: Option<u64>,
) -> Result<Offer, ReceiveError> {
	if description.len() > 256 { return Err(invalid("description too long")); }
	let path = relay_message_path(relay)?;
	let mut builder = OfferBuilder::new(issuer).chain(network).description(description).path(path);
	if let Some(amount) = amount_sat {
		if amount == 0 { return Err(invalid("zero amount")); }
		builder = builder.amount_msats(amount.checked_mul(1000).ok_or_else(|| invalid("amount overflow"))?);
	}
	let offer = builder.build().map_err(ldk_error)?;
	Offer::try_from(with_xbt_feature(&offer.encode(), 12)?).map_err(ldk_error)
}

/// BOLT4 one-hop route terminating at CLN. LDK's receive-path constructor
/// authenticates the final payload with an LDK-local key; a remote CLN does
/// not possess that key. Use the standard empty-AAD encryption here instead.
/// The exact offer and wallet's signature authenticate the application request.
fn relay_message_path(relay: PublicKey) -> Result<BlindedMessagePath, ReceiveError> {
	let secret = SecretKey::from_slice(&Entropy.get_secure_random_bytes()).map_err(ldk_error)?;
	let path_key = PublicKey::from_secret_key(&SECP, &secret);
	let shared = SharedSecret::new(&relay, &secret);
	let subkey = |label: &[u8]| {
		let mut engine = HmacEngine::<sha256::Hash>::new(label);
		engine.input(shared.as_ref());
		Hmac::<sha256::Hash>::from_engine(engine).to_byte_array()
	};
	let tweak = Scalar::from_be_bytes(subkey(b"blinded_node_id")).map_err(ldk_error)?;
	let blinded_node_id = relay.mul_tweak(&SECP, &tweak).map_err(ldk_error)?;
	let cipher = ChaCha20Poly1305::new(&subkey(b"rho").into());
	let encrypted_payload = cipher.encrypt(&[0u8; 12].into(), &[][..]).map_err(ldk_error)?;
	Ok(BlindedMessagePath::from_blinded_path(relay, path_key,
		vec![BlindedHop { blinded_node_id, encrypted_payload }]))
}

/// Extract the exact offer fields from a signed invoice request.
pub fn request_offer(bytes: &[u8]) -> Result<Offer, ReceiveError> {
	let mut encoded = Vec::new();
	for (kind, value) in records(bytes)? {
		if (1..80).contains(&kind) || (1_000_000_000..2_000_000_000).contains(&kind) {
			append_record(&mut encoded, kind, value);
		}
	}
	Offer::try_from(encoded).map_err(ldk_error)
}

pub fn parse_request(
	bytes: &[u8], offer: &Offer, network: Network, maximum_sat: u64,
) -> Result<(InvoiceRequest, u64), ReceiveError> {
	if request_offer(bytes)?.encode() != offer.encode() { return Err(invalid("offer mismatch")); }
	if !offer.offer_features().requires_blake2b_identity() { return Err(invalid("missing XBT feature")); }
	let request = InvoiceRequest::try_from(bytes.to_vec()).map_err(ldk_error)?;
	if request.invoice_request_features().requires_unknown_bits() { return Err(invalid("unknown required request features")); }
	if request.chain() != ChainHash::using_genesis_block(network) { return Err(invalid("wrong chain")); }
	let amount_msat = request.amount_msats().or_else(|| match offer.amount() {
		Some(OfferAmount::Bitcoin { amount_msats }) => Some(amount_msats),
		_ => None,
	}).ok_or_else(|| invalid("missing amount"))?;
	if amount_msat == 0 || amount_msat % 1000 != 0 || amount_msat / 1000 > maximum_sat {
		return Err(invalid("amount outside receive limits"));
	}
	Ok((request, amount_msat / 1000))
}

/// Retries of an identical signed request use the same secret. A new payer
/// request uses a new secret. Do not publish it before the normal Ark claim.
pub fn request_preimage(key: &Keypair, request: &[u8]) -> Result<Preimage, ReceiveError> {
	if request.len() > MAX_MESSAGE_BYTES { return Err(invalid("message too large")); }
	let mut engine = HmacEngine::<sha256::Hash>::new(&key.secret_key().secret_bytes());
	engine.input(b"Paperclip XBT BOLT12 receive preimage v1\0");
	engine.input(request);
	Ok(Preimage::from(Hmac::<sha256::Hash>::from_engine(engine).to_byte_array()))
}

/// Sign an invoice locally. Persist it and its receive checkpoint before
/// returning it to the relay; the relay must register the hold before replying.
pub fn create_invoice(
	request: &InvoiceRequest, offer: &Offer, key: &Keypair, relay: PublicKey,
	preimage: Preimage, cltv_delta: u16, maximum_height: u32,
) -> Result<Bolt12Invoice, ReceiveError> {
	if offer.issuer_signing_pubkey() != Some(key.public_key()) { return Err(invalid("wrong signing key")); }
	if cltv_delta == 0 { return Err(invalid("zero CLTV delta")); }
	let entropy = Entropy;
	let tlvs = UnauthenticatedReceiveTlvs {
		payment_secret: PaymentSecret(rand::random()),
		payment_constraints: PaymentConstraints { max_cltv_expiry: maximum_height, htlc_minimum_msat: 1 },
		payment_context: PaymentContext::Bolt12Offer(Bolt12OfferContext {
			offer_id: offer.id(), invoice_request: InvoiceRequestFields {
				payer_signing_pubkey: request.payer_signing_pubkey(),
				quantity: request.quantity(),
				payer_note_truncated: None,
				human_readable_name: request.offer_from_hrn().clone(),
			},
		}),
	}.authenticate(Nonce::from_entropy_source(&entropy), &ExpandedKey::new(rand::random()));
	let path = BlindedPaymentPath::one_hop(relay, tlvs, cltv_delta, &entropy, &SECP).map_err(ldk_error)?;
	let unsigned = request.respond_with(vec![path], lightning::types::payment::PaymentHash(
		preimage.compute_payment_hash().to_byte_array(),
	)).map_err(ldk_error)?.relative_expiry(900).build().map_err(ldk_error)?;
	let unsigned = UnsignedBolt12Invoice::try_from(with_xbt_feature(&unsigned.encode(), 174)?)
		.map_err(ldk_error)?;
	unsigned.sign(|invoice: &UnsignedBolt12Invoice| {
		Ok(SECP.sign_schnorr_no_aux_rand(invoice.as_ref().as_digest(), key))
	}).map_err(ldk_error)
}

/// Construct a reply packet to inject at the relay CLN node. The temporary
/// signer is only used for onion construction, never for the invoice itself.
pub fn invoice_reply(
	relay: PublicKey, reply_path: BlindedMessagePath, invoice: Bolt12Invoice,
) -> Result<(PublicKey, Vec<u8>), ReceiveError> {
	let keys = KeysManager::new(&rand::random(), 0, 0, true);
	let lookup = EmptyNodeIdLookUp {};
	let path = OnionMessagePath {
		intermediate_nodes: if reply_path.introduction_node() == &lightning::blinded_path::IntroductionNode::NodeId(relay) { Vec::new() } else { vec![relay] },
		destination: Destination::BlindedPath(reply_path),
		first_node_addresses: Vec::new(),
	};
	let (first, message, _) = create_onion_message(
		&&keys, &&keys, &&lookup, &SECP, path, OffersMessage::Invoice(invoice), None,
	).map_err(ldk_error)?;
	if first != relay { return Err(invalid("reply does not start at relay")); }
	Ok((message.blinding_point, message.onion_routing_packet.encode()))
}

/// Verify that an invoice echoes the exact request fields, not another
/// request for the same offer. The invoice's signature is checked by LDK.
pub fn matches_request(invoice: &Bolt12Invoice, request: &[u8]) -> Result<bool, ReceiveError> {
	fn request_field(kind: u64) -> bool {
		kind < 160 || (1_000_000_000..3_000_000_000).contains(&kind)
	}
	let encoded = invoice.encode();
	let invoice_fields: Vec<_> = records(&encoded)?.into_iter().filter(|(k, _)| request_field(*k)).collect();
	let request_fields: Vec<_> = records(request)?.into_iter().filter(|(k, _)| request_field(*k)).collect();
	Ok(invoice_fields == request_fields)
}

/// Domain-separated proof that a relay session owns an offer's signing key.
pub fn session_challenge(offer: &Offer, challenge: &[u8]) -> Result<bitcoin::secp256k1::Message, ReceiveError> {
	if challenge.len() != 32 { return Err(invalid("invalid session challenge")); }
	let mut engine = sha256::Hash::engine();
	engine.input(b"Paperclip XBT BOLT12 relay session v1\0");
	engine.input(challenge);
	engine.input(&offer.encode());
	Ok(bitcoin::secp256k1::Message::from_digest(sha256::Hash::from_engine(engine).to_byte_array()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use bitcoin::secp256k1::SecretKey;
	use lightning::ln::channelmanager::PaymentId;
	use crate::lightning::{Bolt12InvoiceExt, Invoice};

	fn key(byte: u8) -> Keypair {
		Keypair::from_secret_key(&SECP, &SecretKey::from_slice(&[byte; 32]).unwrap())
	}
	fn request(offer: &Offer, amount_msat: u64, id: u8) -> Vec<u8> {
		offer.request_invoice(&ExpandedKey::new([23; 32]), Nonce::from_entropy_source(&Entropy), &SECP, PaymentId([id; 32]))
			.unwrap().amount_msats(amount_msat).unwrap().build_and_sign().unwrap().encode()
	}

	#[test]
	fn relay_path_uses_standard_bolt4_recipient_encryption() {
		let relay = key(2);
		let path = relay_message_path(relay.public_key()).unwrap();
		let shared = SharedSecret::new(&path.blinding_point(), &relay.secret_key());
		let mut engine = HmacEngine::<sha256::Hash>::new(b"rho");
		engine.input(shared.as_ref());
		let rho = Hmac::<sha256::Hash>::from_engine(engine).to_byte_array();
		let cipher = ChaCha20Poly1305::new(&rho.into());
		let payload = &path.blinded_hops()[0].encrypted_payload;
		assert_eq!(cipher.decrypt(&[0u8; 12].into(), payload.as_slice()).unwrap(), Vec::<u8>::new());
		let mut corrupted = payload.clone();
		corrupted[0] ^= 1;
		assert!(cipher.decrypt(&[0u8; 12].into(), corrupted.as_slice()).is_err());
	}

	#[test]
	fn reusable_offer_has_distinct_payment_secrets_and_signed_xbt_invoices() {
		let key = key(1);
		let relay = super::tests::key(2).public_key();
		let offer = create_offer(key.public_key(), relay, Network::Bitcoin, "Paperclip test".into(), None).unwrap();
		assert!(offer.offer_features().requires_blake2b_identity());
		let first = request(&offer, 20_000_000, 1);
		let second = request(&offer, 30_000_000, 2);
		let first_secret = request_preimage(&key, &first).unwrap();
		assert_eq!(first_secret, request_preimage(&key, &first).unwrap());
		assert_ne!(first_secret, request_preimage(&key, &second).unwrap());
		for (encoded, amount) in [(first, 20_000), (second, 30_000)] {
			let (req, sats) = parse_request(&encoded, &offer, Network::Bitcoin, 250_000).unwrap();
			assert_eq!(sats, amount);
			let secret = request_preimage(&key, &encoded).unwrap();
			let invoice = create_invoice(&req, &offer, &key, relay, secret, 200, 1_000_000).unwrap();
			invoice.validate_issuance(&offer, bitcoin::Amount::from_sat(amount)).unwrap();
			assert!(matches_request(&invoice, &encoded).unwrap());
			let invoice = Invoice::Bolt12(invoice);
			invoice.require_xbt().unwrap();
			invoice.check_signature().unwrap();
			assert_eq!(invoice.payment_hash(), secret.compute_payment_hash());
		}
	}

	#[test]
	fn request_rejects_wrong_offer_chain_amount_and_signature() {
		let offer = create_offer(key(1).public_key(), key(2).public_key(), Network::Bitcoin, "test".into(), None).unwrap();
		let other = create_offer(key(3).public_key(), key(2).public_key(), Network::Bitcoin, "other".into(), None).unwrap();
		let good = request(&offer, 20_000_000, 1);
		assert!(parse_request(&good, &other, Network::Bitcoin, 250_000).is_err());
		assert!(parse_request(&good, &offer, Network::Regtest, 250_000).is_err());
		assert!(parse_request(&good, &offer, Network::Bitcoin, 1).is_err());
		assert!(parse_request(&request(&offer, 1001, 2), &offer, Network::Bitcoin, 250_000).is_err());
		let mut forged = good.clone();
		*forged.last_mut().unwrap() ^= 1;
		assert!(parse_request(&forged, &offer, Network::Bitcoin, 250_000).is_err());
		for end in 0..good.len() {
			assert!(parse_request(&good[..end], &offer, Network::Bitcoin, 250_000).is_err());
		}
		assert!(request_offer(&vec![0; MAX_MESSAGE_BYTES + 1]).is_err());
		assert!(records(&[0, 0, 0, 0]).is_err(), "duplicate TLVs");
		assert!(records(&[1, 255, 255, 255, 255, 255, 255, 255, 255, 255]).is_err(), "overflowing length");
	}
}
