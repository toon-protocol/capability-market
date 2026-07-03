//! Host prover for the devnet e2e lifecycle run (toon-meta#84/#119/#120).
//!
//! Loads a matmul guest ELF (use the CANONICAL committed artifact —
//! `predicates/artifacts/matmul-guest.canonical.bin.gz`, gunzipped — so the
//! image ID matches `predicates/ARTIFACTS.json` and the on-chain markets),
//! builds the ExecutorEnv with the guest's input manifest
//! (image_id, market_params, submission), proves, and prints a JSON object
//! with `journal_hex` / `seal_hex` ready for `cast send ... reveal(...)`.
//!
//! Modes:
//! - `dev` — RISC0_DEV_MODE fake receipt; `encode_seal` emits the
//!   0xffffffff mock-verifier seal. Accepted ONLY by marketMock.
//! - `groth16` — real local proving + STARK→SNARK Groth16 wrap (pulls the
//!   risc0 groth16 prover docker image on first use; minutes of wall
//!   clock). Accepted by marketReal's RiscZeroGroth16Verifier.
//!
//! Usage:
//!   e2e-prover --elf <path> --mode dev|groth16 [--rank-bound 49]

use std::time::Instant;

use anyhow::{bail, Context, Result};
use matmul::{encode_market_params, encode_scheme, schemes};
use risc0_zkvm::{compute_image_id, default_prover, ExecutorEnv, ProverOpts};

fn main() -> Result<()> {
    let mut elf_path: Option<String> = None;
    let mut mode: Option<String> = None;
    let mut rank_bound: u32 = 49;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--elf" => elf_path = Some(args.next().context("--elf needs a path")?),
            "--mode" => mode = Some(args.next().context("--mode needs dev|groth16")?),
            "--rank-bound" => {
                rank_bound = args
                    .next()
                    .context("--rank-bound needs a number")?
                    .parse()
                    .context("--rank-bound must be a u32")?
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    let elf_path = elf_path.context("--elf <path> is required")?;
    let mode = mode.context("--mode dev|groth16 is required")?;

    let elf = std::fs::read(&elf_path).with_context(|| format!("reading ELF at {elf_path}"))?;
    let image_id = compute_image_id(&elf)?;
    let image_id_bytes: [u8; 32] = *image_id.as_ref();

    // Guest input manifest (toon-meta#121 order): image_id, market_params, submission.
    // The rank-49 Strassen⊗Strassen scheme is the known-valid witness at bound 49.
    let market_params = encode_market_params(rank_bound);
    let submission = encode_scheme(&schemes::strassen_4x4_rank49());

    let env = ExecutorEnv::builder()
        .write(&image_id_bytes)?
        .write(&market_params.to_vec())?
        .write(&submission.to_vec())?
        .build()?;

    let opts = match mode.as_str() {
        "dev" => {
            // Dev mode: the guest is really executed (journal bytes are real),
            // only the seal is a fake placeholder. encode_seal turns a fake
            // receipt into the 0xffffffff mock-verifier seal.
            std::env::set_var("RISC0_DEV_MODE", "1");
            ProverOpts::default()
        }
        "groth16" => {
            // Fully local proving via the installed `r0vm` server process
            // ("ipc"; the in-process "local" prover needs the heavy `prove`
            // feature — same proof, same machine). The Groth16 wrap shells
            // out to the risczero/risc0-groth16-prover docker image.
            std::env::remove_var("RISC0_DEV_MODE");
            std::env::set_var("RISC0_PROVER", "ipc");
            ProverOpts::groth16()
        }
        other => bail!("unknown mode: {other} (expected dev|groth16)"),
    };

    let started = Instant::now();
    let receipt = default_prover().prove_with_opts(env, &elf, &opts)?.receipt;
    let proving_seconds = started.elapsed().as_secs_f64();

    // Binds the receipt (fake or Groth16) to the canonical image ID.
    receipt.verify(image_id)?;

    let journal_bytes = receipt.journal.bytes.clone();
    let decoded = journal::Journal::decode(&journal_bytes)
        .map_err(|e| anyhow::anyhow!("guest committed a non-canonical journal: {e}"))?;
    anyhow::ensure!(decoded.verdict, "guest verdict is FALSE — refusing to emit a losing proof");
    anyhow::ensure!(
        decoded.image_id == image_id_bytes,
        "journal image_id does not match the ELF image ID"
    );

    let seal = risc0_ethereum_contracts::encode_seal(&receipt)?;

    println!(
        "{{\n  \"mode\": \"{mode}\",\n  \"rank_bound\": {rank_bound},\n  \"image_id\": \"0x{}\",\n  \"market_params_hash\": \"0x{}\",\n  \"solution_hash\": \"0x{}\",\n  \"journal_hex\": \"0x{}\",\n  \"seal_hex\": \"0x{}\",\n  \"proving_seconds\": {proving_seconds:.1}\n}}",
        hex::encode(image_id_bytes),
        hex::encode(decoded.market_params_hash),
        hex::encode(decoded.submission_hash),
        hex::encode(&journal_bytes),
        hex::encode(&seal),
    );
    Ok(())
}
