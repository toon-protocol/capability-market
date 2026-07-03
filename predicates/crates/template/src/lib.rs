//! Predicate authoring template (toon-meta#119 story 1).
//!
//! Copy this crate to author a new capability-market predicate. The layout
//! separates concerns so review and testing stay cheap:
//!
//! - **`src/lib.rs` (this file)** — ALL predicate logic, pure Rust,
//!   host-testable with plain `cargo test`. Your reviewer reads this file
//!   against the English proposition.
//! - **`methods/guest/src/main.rs`** — the guest shell. Ten lines of I/O:
//!   `env::read` inputs in manifest order, call [`evaluate`], `env::commit_slice`
//!   the canonical journal bytes. You should not need to edit it beyond the
//!   crate name.
//! - **`methods/`** — `risc0-build` glue that embeds the compiled guest ELF
//!   and its image ID as `TEMPLATE_GUEST_ELF` / `TEMPLATE_GUEST_ID`.
//!
//! See this crate's README.md for the full build → image ID → Arweave
//! pipeline and the reproducibility requirement.
//!
//! # The example predicate
//!
//! As a placeholder, this template implements the simplest useful predicate:
//! "a sha256 preimage of the 32-byte target pinned in the market params is
//! published". Replace [`check`] (and the market-params/submission encodings
//! it documents) with your own verifier; keep the signature.
//!
//! # Contract with the rest of the system
//!
//! - `market_params` and `submission` arrive as **raw bytes**; the journal
//!   commits `sha256` of those exact bytes ([`journal::predicate_journal`]),
//!   so parse errors must yield `verdict: false`, never a panic — a panicking
//!   guest cannot produce the `false` proof a challenger may need.
//! - `image_id` is a guest *input* committed verbatim: a guest cannot hash
//!   itself. On-chain, the RISC Zero verifier binds the seal to the market's
//!   pinned image ID, and the contract additionally requires
//!   `journal.imageId == market.imageId`.

use journal::Journal;

/// The predicate: raw market-params and submission bytes in, verdict out.
///
/// Example semantics (replace me): `market_params` is exactly 32 bytes — a
/// sha256 digest; verdict is true iff `sha256(submission) == market_params`.
///
/// MUST be total: malformed input is `false`, never a panic.
pub fn check(market_params: &[u8], submission: &[u8]) -> bool {
    let Ok(target) = <&[u8; 32]>::try_from(market_params) else {
        return false;
    };
    &journal::sha256(submission) == target
}

/// The full guest computation, host-callable for tests: judge the raw bytes
/// and assemble the journal exactly as the guest commits it. Predicates
/// normally keep this function as-is.
pub fn evaluate(image_id: [u8; 32], market_params: &[u8], submission: &[u8]) -> Journal {
    let verdict = check(market_params, submission);
    journal::predicate_journal(image_id, market_params, submission, verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_correct_preimage() {
        let submission = b"the witness".to_vec();
        let params = journal::sha256(&submission);
        assert!(check(&params, &submission));
    }

    #[test]
    fn rejects_wrong_preimage() {
        let params = journal::sha256(b"the witness");
        assert!(!check(&params, b"not the witness"));
    }

    #[test]
    fn malformed_params_are_false_not_panic() {
        assert!(!check(&[], b"x"));
        assert!(!check(&[0u8; 31], b"x"));
        assert!(!check(&[0u8; 33], b"x"));
    }

    #[test]
    fn evaluate_commits_hashes_of_raw_bytes() {
        let submission = b"the witness".to_vec();
        let params = journal::sha256(&submission);
        let image_id = [9u8; 32];
        let j = evaluate(image_id, &params, &submission);
        assert!(j.verdict);
        assert_eq!(j.image_id, image_id);
        assert_eq!(j.market_params_hash, journal::sha256(&params));
        assert_eq!(j.submission_hash, journal::sha256(&submission));
    }
}
