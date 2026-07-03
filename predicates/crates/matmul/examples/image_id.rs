//! Print the matmul guest's image ID (hex) and ELF size — the `(image_id,
//! guest_binary)` half of the toolchain output (toon-meta#119 story 1).
//! NOTE: the canonical on-chain image ID must come from the reproducible
//! docker build (`cargo risczero build`); see crates/template/README.md.
//!
//! Run: cargo run --release -p matmul --example image_id

use matmul_methods::{MATMUL_GUEST_ELF, MATMUL_GUEST_ID};
use risc0_zkvm::sha::Digest;

fn main() {
    println!("image_id: 0x{}", Digest::from(MATMUL_GUEST_ID));
    println!("guest_elf_bytes: {}", MATMUL_GUEST_ELF.len());
}
