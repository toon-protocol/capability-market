// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IRiscZeroVerifier} from "../../src/interfaces/IRiscZeroVerifier.sol";

/// @notice Test double for the RISC Zero verifier. Like the real verifier, `verify` REVERTS
///         on an invalid seal. A seal is "valid" when it equals
///         abi.encodePacked(SEAL_TAG, imageId, journalDigest) — so tests exercise the wrong
///         image-ID and wrong-journal paths through the digest binding, exactly as the real
///         verifier would — or when `alwaysAccept` is toggled on.
contract MockRiscZeroVerifier is IRiscZeroVerifier {
    bytes32 public constant SEAL_TAG = keccak256("MockRiscZeroVerifier.seal");

    bool public alwaysAccept;
    bool public alwaysReject;

    error VerificationFailed();

    function setAlwaysAccept(bool v) external {
        alwaysAccept = v;
    }

    function setAlwaysReject(bool v) external {
        alwaysReject = v;
    }

    /// @notice Helper for tests: forge the seal the mock will accept for a given claim.
    function mockSeal(bytes32 imageId, bytes32 journalDigest) public pure returns (bytes memory) {
        return abi.encodePacked(SEAL_TAG, imageId, journalDigest);
    }

    function verify(bytes calldata seal, bytes32 imageId, bytes32 journalDigest) external view {
        if (alwaysReject) revert VerificationFailed();
        if (alwaysAccept) return;
        if (keccak256(seal) != keccak256(mockSeal(imageId, journalDigest))) {
            revert VerificationFailed();
        }
    }
}
