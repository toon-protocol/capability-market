//! RISC Zero guest shell for the matmul predicate. All verification logic
//! lives in the host-testable `matmul` library; this file only does I/O:
//! read inputs by manifest order, run the check, commit the canonical
//! journal bytes.

use risc0_zkvm::guest::env;

fn main() {
    // Guest inputs (toon-meta#121, capability-market#4): image_id,
    // manifest_bytes, submission. image_id is committed verbatim; its integrity
    // is enforced on-chain by seal verification against the market's pinned
    // image ID. `manifest_bytes` is the canonical input manifest — the guest
    // commits market_params_hash = sha256(manifest_bytes) and extracts the rank
    // bound from it by name.
    let image_id: [u8; 32] = env::read();
    let manifest_bytes: Vec<u8> = env::read();
    let submission: Vec<u8> = env::read();

    let journal = matmul::evaluate(image_id, &manifest_bytes, &submission);

    // commit_slice, NOT commit: the journal must be the raw canonical
    // journal-v1 bytes so that sha256(journal) on-chain matches
    // Journal::digest() (see the journal crate docs).
    env::commit_slice(&journal.encode());
}
