//! RISC Zero guest shell for the matmul predicate. All verification logic
//! lives in the host-testable `matmul` library; this file only does I/O:
//! read inputs by manifest order, run the check, commit the canonical
//! journal bytes.

use risc0_zkvm::guest::env;

fn main() {
    // Input manifest order (toon-meta#121): image_id, market_params, submission.
    // image_id is committed verbatim; its integrity is enforced on-chain by
    // the seal verification against the market's pinned image ID.
    let image_id: [u8; 32] = env::read();
    let market_params: Vec<u8> = env::read();
    let submission: Vec<u8> = env::read();

    let journal = matmul::evaluate(image_id, &market_params, &submission);

    // commit_slice, NOT commit: the journal must be the raw canonical
    // journal-v1 bytes so that sha256(journal) on-chain matches
    // Journal::digest() (see the journal crate docs).
    env::commit_slice(&journal.encode());
}
