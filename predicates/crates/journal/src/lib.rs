//! Canonical capability-market journal (toon-meta#121, consumed by toon-meta#119/#120).
//!
//! Every predicate guest program commits exactly one [`Journal`] via
//! `env::commit_slice(&journal.encode())`. `CapabilityMarket.sol` decodes the
//! same bytes on reveal. The two implementations MUST agree byte-for-byte.
//!
//! # Canonical encoding (`journal-v1`)
//!
//! Fixed-size, tightly packed, **97 bytes**, in this field order:
//!
//! | offset | size | field                | encoding                                   |
//! |--------|------|----------------------|--------------------------------------------|
//! | 0      | 32   | `image_id`           | opaque 32-byte digest, copied verbatim     |
//! | 32     | 32   | `market_params_hash` | opaque 32-byte digest, copied verbatim     |
//! | 64     | 32   | `submission_hash`    | opaque 32-byte digest, copied verbatim     |
//! | 96     | 1    | `verdict`            | `0x00` = false, `0x01` = true; all other values are a decode error |
//!
//! ## Endianness
//!
//! There are no multi-byte integers in the layout, so endianness never
//! applies. The three 32-byte fields are *byte strings* (digests), not
//! integers: byte `i` of the Rust array is byte `i` of the encoding, and maps
//! to byte `i` of the Solidity `bytes32` (i.e. `image_id[0]` is the
//! most-significant byte of `bytes32 imageId`). No byte-swapping anywhere.
//!
//! ## Version tag
//!
//! There is deliberately **no in-band version byte**. A journal is only ever
//! interpreted in the context of a proof whose image ID is pinned on-chain by
//! the market, and the image ID transitively pins the exact encoder the guest
//! was built with. Changing this layout means new guest images (new image
//! IDs) and an explicit protocol migration — exactly the version-pinning
//! posture of toon-meta#119 story 2. The spec-level version is exported as
//! [`ENCODING_VERSION`] for tooling.
//!
//! ## `image_id` provenance
//!
//! A guest cannot hash itself, so `image_id` is supplied to the guest as an
//! input and committed verbatim. Its integrity is NOT established by the
//! guest: the on-chain RISC Zero verifier checks the seal against the
//! market's pinned image ID, and `CapabilityMarket.sol` additionally requires
//! `journal.imageId == market.imageId`. A prover lying about `image_id` in
//! the input produces a journal the contract rejects.
//!
//! # Digest
//!
//! [`Journal::digest`] is `sha256(encode())` — the exact value
//! `CapabilityMarket.sol` passes to the RISC Zero verifier as the journal
//! digest (`sha256(journal)` over the raw committed bytes). This is why the
//! guest must commit with `env::commit_slice(&journal.encode())` and NOT
//! `env::commit(&journal)`: the latter would serialize through RISC Zero's
//! word-oriented serde and produce different bytes.

use sha2::{Digest, Sha256};

/// Spec version of the canonical encoding. Bumping this is a protocol
/// migration (see module docs); it is not serialized in-band.
pub const ENCODING_VERSION: u8 = 1;

/// Encoded size in bytes: 3 × 32-byte digests + 1 verdict byte.
pub const ENCODED_LEN: usize = 97;

/// The journal committed by every capability-market predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Journal {
    /// RISC Zero image ID of the guest that produced this journal
    /// (supplied as guest input; enforced on-chain, see module docs).
    pub image_id: [u8; 32],
    /// sha256 of the market's canonical input manifest / params encoding.
    pub market_params_hash: [u8; 32],
    /// sha256 of the raw submission bytes being judged.
    pub submission_hash: [u8; 32],
    /// The predicate's verdict on the submission.
    pub verdict: bool,
}

/// Errors from [`Journal::decode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Input was not exactly [`ENCODED_LEN`] bytes.
    WrongLength { got: usize },
    /// Verdict byte was neither `0x00` nor `0x01`.
    InvalidVerdictByte(u8),
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::WrongLength { got } => {
                write!(f, "journal must be exactly {ENCODED_LEN} bytes, got {got}")
            }
            DecodeError::InvalidVerdictByte(b) => {
                write!(f, "verdict byte must be 0x00 or 0x01, got {b:#04x}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

impl Journal {
    /// Serialize to the canonical 97-byte `journal-v1` layout.
    pub fn encode(&self) -> [u8; ENCODED_LEN] {
        let mut out = [0u8; ENCODED_LEN];
        out[0..32].copy_from_slice(&self.image_id);
        out[32..64].copy_from_slice(&self.market_params_hash);
        out[64..96].copy_from_slice(&self.submission_hash);
        out[96] = self.verdict as u8;
        out
    }

    /// Parse the canonical layout. Strict: length must be exactly 97 and the
    /// verdict byte must be `0x00`/`0x01` (a Solidity decoder must apply the
    /// same strictness so both sides accept an identical byte-string set).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() != ENCODED_LEN {
            return Err(DecodeError::WrongLength { got: bytes.len() });
        }
        let verdict = match bytes[96] {
            0x00 => false,
            0x01 => true,
            b => return Err(DecodeError::InvalidVerdictByte(b)),
        };
        let mut image_id = [0u8; 32];
        let mut market_params_hash = [0u8; 32];
        let mut submission_hash = [0u8; 32];
        image_id.copy_from_slice(&bytes[0..32]);
        market_params_hash.copy_from_slice(&bytes[32..64]);
        submission_hash.copy_from_slice(&bytes[64..96]);
        Ok(Journal {
            image_id,
            market_params_hash,
            submission_hash,
            verdict,
        })
    }

    /// `sha256(encode())` — the journal digest `CapabilityMarket.sol` hands
    /// to the RISC Zero verifier.
    pub fn digest(&self) -> [u8; 32] {
        sha256(&self.encode())
    }
}

/// sha256 helper shared by predicates (submission / market-params hashing).
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(bytes));
    out
}

/// Assemble the journal every predicate commits: hashes the raw market-params
/// and submission bytes exactly as received (before any parsing), so the
/// on-chain commitments bind the bytes, not a parsed view of them.
pub fn predicate_journal(
    image_id: [u8; 32],
    market_params: &[u8],
    submission: &[u8],
    verdict: bool,
) -> Journal {
    Journal {
        image_id,
        market_params_hash: sha256(market_params),
        submission_hash: sha256(submission),
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        vectors: Vec<Vector>,
    }

    #[derive(Deserialize)]
    struct Vector {
        name: String,
        image_id: String,
        market_params_hash: String,
        submission_hash: String,
        verdict: bool,
        encoded: String,
        sha256: String,
    }

    fn hex32(s: &str) -> [u8; 32] {
        let v = hex::decode(s).unwrap();
        v.try_into().unwrap()
    }

    fn golden() -> Vec<Vector> {
        let raw = include_str!("../tests/golden_journal_vectors.json");
        serde_json::from_str::<Fixture>(raw).unwrap().vectors
    }

    #[test]
    fn golden_vectors_encode() {
        for v in golden() {
            let j = Journal {
                image_id: hex32(&v.image_id),
                market_params_hash: hex32(&v.market_params_hash),
                submission_hash: hex32(&v.submission_hash),
                verdict: v.verdict,
            };
            assert_eq!(hex::encode(j.encode()), v.encoded, "encode: {}", v.name);
            assert_eq!(hex::encode(j.digest()), v.sha256, "digest: {}", v.name);
        }
    }

    #[test]
    fn golden_vectors_decode_round_trip() {
        for v in golden() {
            let bytes = hex::decode(&v.encoded).unwrap();
            let j = Journal::decode(&bytes).unwrap();
            assert_eq!(hex::encode(j.image_id), v.image_id, "{}", v.name);
            assert_eq!(
                hex::encode(j.market_params_hash),
                v.market_params_hash,
                "{}",
                v.name
            );
            assert_eq!(hex::encode(j.submission_hash), v.submission_hash, "{}", v.name);
            assert_eq!(j.verdict, v.verdict, "{}", v.name);
            assert_eq!(j.encode().as_slice(), bytes.as_slice(), "{}", v.name);
        }
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert_eq!(
            Journal::decode(&[0u8; 96]),
            Err(DecodeError::WrongLength { got: 96 })
        );
        assert_eq!(
            Journal::decode(&[0u8; 98]),
            Err(DecodeError::WrongLength { got: 98 })
        );
        assert_eq!(Journal::decode(&[]), Err(DecodeError::WrongLength { got: 0 }));
    }

    #[test]
    fn decode_rejects_noncanonical_verdict_byte() {
        // Anything but 0x00/0x01 must fail — otherwise two different byte
        // strings would decode to the same journal and sha256 binding breaks.
        for b in [0x02u8, 0x80, 0xff] {
            let mut bytes = [0u8; ENCODED_LEN];
            bytes[96] = b;
            assert_eq!(
                Journal::decode(&bytes),
                Err(DecodeError::InvalidVerdictByte(b))
            );
        }
    }

    #[test]
    fn verdict_byte_is_last() {
        let j = Journal {
            image_id: [0u8; 32],
            market_params_hash: [0u8; 32],
            submission_hash: [0u8; 32],
            verdict: true,
        };
        let enc = j.encode();
        assert_eq!(enc[96], 0x01);
        assert!(enc[..96].iter().all(|&b| b == 0));
    }
}
