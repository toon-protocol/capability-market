//! Embedded guest artifacts: `MATMUL_GUEST_ELF` and `MATMUL_GUEST_ID`
//! (the RISC Zero image ID, i.e. the 32-byte content hash of the guest).
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
