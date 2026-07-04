//! End-to-end local proving of the matmul guest (toon-meta#119 story 3,
//! guest-side half). Runs the real RISC Zero executor over the real guest
//! ELF in dev mode (`RISC0_DEV_MODE=1`): the guest is actually executed and
//! the committed journal is real; only the cryptographic seal is a dev-mode
//! placeholder. `cargo test` therefore exercises the exact bytes
//! `CapabilityMarket.sol` will decode. Production proving is the same code
//! path without the env var (plus Bonsai/Kalypso for GPU provers).

use journal::Journal;
use matmul::{encode_manifest, encode_scheme, schemes, FLAGSHIP_RANK_BOUND};
use matmul_methods::{MATMUL_GUEST_ELF, MATMUL_GUEST_ID};
use risc0_zkvm::{default_prover, sha::Digest, ExecutorEnv, Receipt};

fn image_id_bytes() -> [u8; 32] {
    *Digest::from(MATMUL_GUEST_ID).as_ref()
}

// Deadline literal pinned into the manifest for these fixtures (audit-only).
const FROZEN_CLOCK: u64 = 1_735_689_600;

fn dev_prove(manifest_bytes: &[u8], submission: &[u8]) -> Receipt {
    // Dev mode: execute the guest for real, skip the STARK. Deterministic
    // and fast enough for CI; see module docs.
    std::env::set_var("RISC0_DEV_MODE", "1");
    let env = ExecutorEnv::builder()
        .write(&image_id_bytes())
        .unwrap()
        .write(&manifest_bytes.to_vec())
        .unwrap()
        .write(&submission.to_vec())
        .unwrap()
        .build()
        .unwrap();
    let receipt = default_prover()
        .prove(env, MATMUL_GUEST_ELF)
        .unwrap()
        .receipt;
    // Binds the receipt to this guest's image ID (dev-mode variant).
    receipt.verify(MATMUL_GUEST_ID).unwrap();
    receipt
}

fn decode_journal(receipt: &Receipt) -> Journal {
    let bytes = &receipt.journal.bytes;
    assert_eq!(
        bytes.len(),
        journal::ENCODED_LEN,
        "guest must commit exactly the 97-byte canonical journal"
    );
    let j = Journal::decode(bytes).expect("journal must be canonical journal-v1");
    // The digest the on-chain verifier is handed is sha256 of the raw
    // committed bytes — must equal Journal::digest().
    assert_eq!(journal::sha256(bytes), j.digest());
    j
}

#[test]
fn proves_strassen49_true_at_bound_49() {
    let manifest_bytes = encode_manifest(49, FROZEN_CLOCK);
    let submission = encode_scheme(&schemes::strassen_4x4_rank49());
    let receipt = dev_prove(&manifest_bytes, &submission);
    let j = decode_journal(&receipt);
    assert!(j.verdict, "rank-49 Strassen² scheme is valid at bound 49");
    assert_eq!(j.image_id, image_id_bytes());
    // The guest commits sha256(manifest_bytes) as market_params_hash (#4).
    assert_eq!(j.market_params_hash, journal::sha256(&manifest_bytes));
    assert_eq!(j.submission_hash, journal::sha256(&submission));
}

#[test]
fn proves_verdict_false_against_flagship_bound_46() {
    // A *valid* rank-49 scheme submitted to the flagship rank-46 market:
    // the proof itself succeeds, but attests verdict = false.
    let manifest_bytes = encode_manifest(FLAGSHIP_RANK_BOUND, FROZEN_CLOCK);
    let submission = encode_scheme(&schemes::strassen_4x4_rank49());
    let j = decode_journal(&dev_prove(&manifest_bytes, &submission));
    assert!(!j.verdict);
    assert_eq!(j.market_params_hash, journal::sha256(&manifest_bytes));
}

#[test]
fn proves_verdict_false_for_malformed_submission_not_trap() {
    // Truncated submission (length not a multiple of 6): the guest must
    // COMMIT verdict = false — the FALSE path is a real, provable journal
    // (with the hash of the malformed bytes as evaluated), not a trap.
    let manifest_bytes = encode_manifest(49, FROZEN_CLOCK);
    let mut submission = encode_scheme(&schemes::strassen_4x4_rank49());
    submission.pop();
    let j = decode_journal(&dev_prove(&manifest_bytes, &submission));
    assert!(!j.verdict);
    assert_eq!(j.submission_hash, journal::sha256(&submission));
}

#[test]
fn proves_verdict_false_for_corrupted_scheme() {
    let manifest_bytes = encode_manifest(49, FROZEN_CLOCK);
    let mut scheme = schemes::strassen_4x4_rank49();
    scheme[0].v ^= 1 << 5; // one flipped GF(2) coefficient
    let submission = encode_scheme(&scheme);
    let j = decode_journal(&dev_prove(&manifest_bytes, &submission));
    assert!(!j.verdict);
}
