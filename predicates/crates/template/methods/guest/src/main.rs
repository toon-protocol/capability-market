//! Guest shell — predicate authors copy this file verbatim (only the library
//! crate name changes). All logic lives in the host-testable library.

use risc0_zkvm::guest::env;

fn main() {
    // Input manifest order (toon-meta#121): image_id, market_params, submission.
    // image_id is committed verbatim; its integrity is enforced on-chain by
    // the seal verification against the market's pinned image ID.
    let image_id: [u8; 32] = env::read();
    let market_params: Vec<u8> = env::read();
    let submission: Vec<u8> = env::read();

    let journal = template::evaluate(image_id, &market_params, &submission);

    // commit_slice, NOT commit: the journal must be the raw canonical
    // journal-v1 bytes so that sha256(journal) on-chain matches
    // Journal::digest() (see the journal crate docs).
    env::commit_slice(&journal.encode());
}
