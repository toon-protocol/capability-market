//! Predicate retrievability / eligibility checker (envelope spec §3.1 check 2, §3.3).
//!
//! Fetches predicate guest bytes from a local path or an Arweave gateway URL,
//! transparently gunzips compressed-at-rest artifacts (gzip magic `1f 8b`),
//! recomputes the RISC Zero image ID with `risc0_binfmt::compute_image_id`
//! (NOT a flat sha256 — the image ID is a structured commitment over the ELF's
//! loaded memory image), and compares it against the expected image ID.
//!
//! Usage:
//!   check-predicate <path-or-url> <expected-image-id-hex> [retries] [retry-delay-secs]
//!
//! URL fetch shells out to `curl`; `retries` (default 1 attempt, i.e. no
//! retry) exists because freshly uploaded Arweave txs can take minutes to
//! propagate to gateways. Exit code 0 iff the recomputed image ID matches.

use std::process::ExitCode;

fn fetch(source: &str) -> Result<Vec<u8>, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let out = std::process::Command::new("curl")
            .args(["-sSfL", "--max-time", "120", source])
            .output()
            .map_err(|e| format!("failed to spawn curl: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "curl {source} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(out.stdout)
    } else {
        std::fs::read(source).map_err(|e| format!("failed to read {source}: {e}"))
    }
}

fn gunzip_if_needed(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.len() > 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        use std::io::Read;
        let mut out = Vec::new();
        // Single-member RFC 1952 stream per the envelope spec; GzDecoder
        // (not MultiGzDecoder) enforces exactly that.
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_end(&mut out)
            .map_err(|e| format!("gunzip failed: {e}"))?;
        Ok(out)
    } else {
        Ok(bytes)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} <path-or-url> <expected-image-id-hex> [retries] [retry-delay-secs]",
            args[0]
        );
        return ExitCode::from(2);
    }
    let source = &args[1];
    let expected = args[2].trim_start_matches("0x").to_lowercase();
    let retries: u32 = args.get(3).map_or(1, |s| s.parse().unwrap_or(1)).max(1);
    let delay: u64 = args.get(4).map_or(30, |s| s.parse().unwrap_or(30));

    let mut last_err = String::new();
    for attempt in 1..=retries {
        match fetch(source).and_then(gunzip_if_needed) {
            Ok(elf) => {
                let image_id = match risc0_binfmt::compute_image_id(&elf) {
                    Ok(id) => id,
                    Err(e) => {
                        eprintln!("FAIL: compute_image_id: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                let got = hex::encode(image_id.as_bytes());
                if got == expected {
                    println!("OK: image ID matches ({got}, {} ELF bytes)", elf.len());
                    return ExitCode::SUCCESS;
                }
                eprintln!("FAIL: image ID mismatch: expected {expected}, got {got}");
                return ExitCode::FAILURE;
            }
            Err(e) => {
                last_err = e;
                if attempt < retries {
                    eprintln!(
                        "attempt {attempt}/{retries} failed ({last_err}); retrying in {delay}s"
                    );
                    std::thread::sleep(std::time::Duration::from_secs(delay));
                }
            }
        }
    }
    eprintln!("FAIL: could not fetch {source}: {last_err}");
    ExitCode::FAILURE
}
