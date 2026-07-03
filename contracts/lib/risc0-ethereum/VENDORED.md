# Vendored: risc0-ethereum v3.0.1 (subset)

Source: https://github.com/risc0/risc0-ethereum @ tag `v3.0.1`
(pairs with risc0-zkvm 3.0.x — the repo's workspace pins `risc0-zkvm = "3.0.3"`;
local prover toolchain is r0vm/cargo-risczero 3.0.5).

Vendored directly (no git submodule), consistent with how `contracts/lib/forge-std`
is vendored in this repo. Files are copied verbatim from `contracts/src/` of the
upstream repo — do not edit; to upgrade, re-copy from a newer tag and update this note.

Subset vendored (only what the devnet deploy needs):

- `contracts/src/IRiscZeroVerifier.sol` — verifier interface + ReceiptClaim libs
- `contracts/src/IRiscZeroSelectable.sol`
- `contracts/src/Util.sol`
- `contracts/src/StructHash.sol`
- `contracts/src/groth16/ControlID.sol` — CONTROL_ROOT / BN254_CONTROL_ID for zkVM 3.0.x
  - CONTROL_ROOT = 0xa54dc85ac99f851c92d7c96d7318af41dbe7c0194edfcc37eb4d422a998c1f56
  - BN254_CONTROL_ID = 0x04446e66d300eb7fb45c9726bb53c793dda407a62e9601618bb43c5c14657ac0
- `contracts/src/groth16/Groth16Verifier.sol`
- `contracts/src/groth16/RiscZeroGroth16Verifier.sol`
- `contracts/src/test/RiscZeroMockVerifier.sol` — accepts dev-mode (fake) seals;
  deploy with selector `0xFFFFFFFF` to match risc0's FakeReceipt selector

Extra dependency: `RiscZeroGroth16Verifier`/`StructHash` import
`openzeppelin/contracts/utils/math/SafeCast.sol`, vendored (standalone file) in
`contracts/lib/openzeppelin-contracts/contracts/utils/math/SafeCast.sol` at the same
OpenZeppelin commit risc0-ethereum v3.0.1 pins as a submodule
(`acd4ff74de833399287ed6b31b4debf6b2b35527`).

Remappings for these paths live in `contracts/remappings.txt`.
