// Knots extended header and PoW hash, following primitives/block.{h,cpp}
// at v29.4.2.knots20260508rc2. SHA256d transaction hashing is unchanged.

#[allow(missing_docs)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(crate = "actual_serde"))]
pub struct Blake2bHeader {
    pub nonce2: u32,
    pub nonce3: u32,
    pub extranonce: [u8; 16],
    pub time_offset: u32,
    pub txcount: u16,
    pub flags: u8,
    pub xor_mask_clear_bits: u8,
    pub xor_key: [u8; 16],
    pub height: i32,
    pub merge_mining_rhs: [u8; 32],
}
impl_consensus_encoding!(Blake2bHeader, nonce2, nonce3, extranonce, time_offset,
    txcount, flags, xor_mask_clear_bits, xor_key, height, merge_mining_rhs);

impl Encodable for Header {
    fn consensus_encode<W: Write + ?Sized>(&self, w: &mut W) -> Result<usize, io::Error> {
        let extended = (self.version.to_consensus() as u32 & 0x80000000) != 0;
        if extended != self.blake2b.is_some() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "inconsistent Blake2b header"));
        }
        let time = match &self.blake2b {
            Some(x) if x.flags & 4 != 0 => self.time.wrapping_sub(x.time_offset),
            _ => self.time,
        };
        self.version.consensus_encode(w)?;
        self.prev_blockhash.consensus_encode(w)?;
        self.merkle_root.consensus_encode(w)?;
        time.consensus_encode(w)?;
        self.bits.consensus_encode(w)?;
        self.nonce.consensus_encode(w)?;
        if let Some(x) = &self.blake2b { x.consensus_encode(w)?; }
        Ok(if extended { 164 } else { 80 })
    }
}

impl Decodable for Header {
    fn consensus_decode<R: Read + ?Sized>(r: &mut R) -> Result<Self, encode::Error> {
        let version = Version::consensus_decode(r)?;
        let prev_blockhash = BlockHash::consensus_decode(r)?;
        let merkle_root = TxMerkleNode::consensus_decode(r)?;
        let mut time = u32::consensus_decode(r)?;
        let bits = CompactTarget::consensus_decode(r)?;
        let nonce = u32::consensus_decode(r)?;
        let blake2b = if version.to_consensus() as u32 & 0x80000000 != 0 {
            let x = Blake2bHeader::consensus_decode(r)?;
            if x.flags & 4 != 0 { time = time.wrapping_add(x.time_offset); }
            Some(x)
        } else { None };
        Ok(Self { version, prev_blockhash, merkle_root, time, bits, nonce, blake2b })
    }
}

fn blake_tag(tag: &[u8], data: &[u8]) -> [u8; 32] {
    let taghash = hashes::sha256::Hash::hash(tag).to_byte_array();
    let mut v = taghash.to_vec();
    v.extend(taghash);
    v.extend(data);
    hashes::sha256::Hash::hash(&v).to_byte_array()
}

fn blake_hash(data: &[u8]) -> [u8; 32] {
    let hash = blake2b_simd::Params::new().hash_length(32).hash(data);
    let mut out = [0; 32];
    out.copy_from_slice(hash.as_bytes());
    out
}

impl Blake2bHeader {
    fn block_hash(&self, header: &Header) -> BlockHash {
        let mut prev = header.prev_blockhash.to_byte_array();
        prev.reverse();
        let mut mask = [0u8; 32];
        if self.xor_key != [0; 16] {
            mask = blake_tag(b"Bitcoin block hash PoW XOR mask", &self.xor_key);
            let full = usize::from(self.xor_mask_clear_bits / 8);
            mask[..full].fill(0);
            mask[full] &= 0xff >> (self.xor_mask_clear_bits % 8);
        }
        let time = if self.flags & 4 != 0 {
            header.time.wrapping_sub(self.time_offset)
        } else { header.time };
        let mut h1 = crate::consensus::serialize(&header.version);
        h1.extend(prev);
        h1.extend(self.height.to_le_bytes());
        h1.extend(header.merkle_root.to_byte_array());
        h1.extend(time.to_le_bytes());
        h1.push(0);
        h1.extend(crate::consensus::serialize(&header.bits));
        h1.extend(u32::from(self.txcount).to_le_bytes());
        h1.extend([self.flags, self.xor_mask_clear_bits]);
        h1.extend(blake_tag(b"Bitcoin block hash PoW XOR key", &self.xor_key));
        let mut h2 = blake_tag(b"Bitcoin block header 1", &h1).to_vec();
        h2.extend([0u8; 32]);
        h2.extend(self.merge_mining_rhs);
        let h2 = blake_tag(b"Merge-mining hook", &h2);
        let mut first = vec![0u8; 4];
        first.extend(h2);
        first.extend(self.extranonce);
        let first = blake_hash(&first);
        let mut second = Vec::new();
        match self.flags & 3 {
            0 => {
                let mut hidden = blake_tag(b"Bitcoin prevblock header, hashed", &prev);
                hidden[..6].fill(0);
                second.extend(hidden);
            },
            2 => { second.extend([0u8; 48]); second.extend(h2); },
            3 => { second.extend([0u8; 80]); second.extend(h2); },
            _ => {},
        }
        second.extend(header.nonce.to_le_bytes());
        second.extend(self.nonce2.to_le_bytes());
        if self.flags & 3 == 1 {
            second.extend(self.nonce3.to_le_bytes());
            second.extend(self.time_offset.to_le_bytes());
        } else {
            second.extend(self.time_offset.to_le_bytes());
            second.extend(self.nonce3.to_le_bytes());
        }
        second.extend(first);
        if self.flags & 3 == 1 { second.extend(h2); }
        let mut result = blake_hash(&second);
        for (byte, mask) in result.iter_mut().zip(mask) { *byte ^= mask; }
        result.reverse();
        BlockHash::from_byte_array(result)
    }
}
