//! Proves the template scaffold end-to-end in dev mode: real guest
//! execution, real journal bytes, dev-mode seal. Authors keep this test
//! (adjusted to their encodings) when copying the template.

use journal::Journal;
use risc0_zkvm::{default_prover, sha::Digest, ExecutorEnv, Receipt};
use template_methods::{TEMPLATE_GUEST_ELF, TEMPLATE_GUEST_ID};

fn image_id_bytes() -> [u8; 32] {
    *Digest::from(TEMPLATE_GUEST_ID).as_ref()
}

fn dev_prove(market_params: &[u8], submission: &[u8]) -> Receipt {
    std::env::set_var("RISC0_DEV_MODE", "1");
    let env = ExecutorEnv::builder()
        .write(&image_id_bytes())
        .unwrap()
        .write(&market_params.to_vec())
        .unwrap()
        .write(&submission.to_vec())
        .unwrap()
        .build()
        .unwrap();
    let receipt = default_prover()
        .prove(env, TEMPLATE_GUEST_ELF)
        .unwrap()
        .receipt;
    receipt.verify(TEMPLATE_GUEST_ID).unwrap();
    receipt
}

#[test]
fn template_guest_commits_canonical_journal() {
    let submission = b"the witness".to_vec();
    let params = journal::sha256(&submission); // preimage predicate: target digest

    let receipt = dev_prove(&params, &submission);
    let bytes = &receipt.journal.bytes;
    assert_eq!(bytes.len(), journal::ENCODED_LEN);
    let j = Journal::decode(bytes).unwrap();
    assert!(j.verdict);
    assert_eq!(j.image_id, image_id_bytes());
    assert_eq!(j.market_params_hash, journal::sha256(&params));
    assert_eq!(j.submission_hash, journal::sha256(&submission));
    assert_eq!(journal::sha256(bytes), j.digest());

    // Host-side evaluate() must agree byte-for-byte with the guest.
    let host = template::evaluate(image_id_bytes(), &params, &submission);
    assert_eq!(host.encode().as_slice(), bytes.as_slice());
}

#[test]
fn template_guest_verdict_false_for_wrong_witness() {
    let params = journal::sha256(b"the witness");
    let j = Journal::decode(&dev_prove(&params, b"wrong").journal.bytes).unwrap();
    assert!(!j.verdict);
}
