// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice RISC Zero verifier interface (matches risc0-ethereum's IRiscZeroVerifier).
/// @dev `verify` REVERTS when the seal is invalid; it does not return a bool.
interface IRiscZeroVerifier {
    /// @param seal The encoded cryptographic proof (i.e. SNARK).
    /// @param imageId The identifier for the guest program.
    /// @param journalDigest The SHA-256 digest of the journal bytes.
    function verify(bytes calldata seal, bytes32 imageId, bytes32 journalDigest) external view;
}
