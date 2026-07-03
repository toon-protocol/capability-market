//! Journal envelope per toon-protocol/toon-meta#121.
//!
//! The exact struct committed by the guest via `env::commit()` and decoded by
//! `CapabilityMarket.sol` on reveal. Canonical encoding, endianness, and
//! version tag are documented in `docs/predicate-envelope.md` (toon-meta#121).
//
// TODO(dedup): replace with capability-market journal crate after feat/risc0-toolchain-matmul merges

// NOTE: the shared journal crate derives `serde::Serialize` (required for
// `env::commit()`); this local placeholder stays dependency-free so the
// predicate library is host-testable without the RISC Zero toolchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Journal {
    pub image_id: [u8; 32],
    pub market_params_hash: [u8; 32],
    pub submission_hash: [u8; 32],
    pub verdict: bool,
}
