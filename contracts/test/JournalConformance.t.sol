// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {CapabilityMarketBase} from "./CapabilityMarketBase.t.sol";
import {CapabilityMarket} from "../src/CapabilityMarket.sol";

/// Conformance suite for the strict `journal-v1` decoder (toon-meta#121).
///
/// The vectors below are embedded verbatim from the canonical shared fixture
/// `predicates/crates/journal/tests/golden_journal_vectors.json` (branch
/// feat/risc0-toolchain-matmul, PR #3), which the Rust journal crate's tests also
/// consume. If that file changes, this suite must change with it — the Solidity
/// decoder and the Rust `Journal::decode` MUST accept/reject an identical
/// byte-string set, and `sha256(journal)` here must equal `Journal::digest()` there
/// (it is the digest handed to the RISC Zero verifier).
contract JournalConformanceTest is CapabilityMarketBase {
    struct Vector {
        string name;
        bytes32 vImageId;
        bytes32 vMarketParamsHash;
        bytes32 vSubmissionHash;
        bool vVerdict;
        bytes encoded;
        bytes32 sha256Digest;
    }

    function goldenVectors() internal pure returns (Vector[4] memory v) {
        v[0] = Vector({
            name: "all-zero fields, verdict false",
            vImageId: 0x0000000000000000000000000000000000000000000000000000000000000000,
            vMarketParamsHash: 0x0000000000000000000000000000000000000000000000000000000000000000,
            vSubmissionHash: 0x0000000000000000000000000000000000000000000000000000000000000000,
            vVerdict: false,
            encoded: hex"00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            sha256Digest: 0x136dd1a7d0a62859f2077a62b7673c5c712fb750604a15f5f6140ab2c5112327
        });
        v[1] = Vector({
            name: "ascending bytes, verdict true",
            vImageId: 0x000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f,
            vMarketParamsHash: 0x202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f,
            vSubmissionHash: 0x404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f,
            vVerdict: true,
            encoded: hex"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f01",
            sha256Digest: 0x6f4921d8a2566cb8ffb47f3011bcd2c87e0e3d2af1f8a714b6cb86f07b96220d
        });
        v[2] = Vector({
            name: "saturated/pattern fields, verdict true",
            vImageId: 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff,
            vMarketParamsHash: 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,
            vSubmissionHash: 0x5555555555555555555555555555555555555555555555555555555555555555,
            vVerdict: true,
            encoded: hex"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa555555555555555555555555555555555555555555555555555555555555555501",
            sha256Digest: 0xadc18bc469f66467f75b56e93966fd952e75f9dc6666a91cb8ac27669e91ed0d
        });
        v[3] = Vector({
            name: "realistic digests, verdict false",
            vImageId: 0xc98fdcab9c8ef2a3a2f8f5f0d305aa50e4324a4c0a1eb0c50c0f1a2b3c4d5e6f,
            vMarketParamsHash: 0x7d87c5ea75f7378bb701e404c50639161af3eff66293e9f375b5f17eb50476f4,
            vSubmissionHash: 0x0202020202020202020202020202020202020202020202020202020202020202,
            vVerdict: false,
            encoded: hex"c98fdcab9c8ef2a3a2f8f5f0d305aa50e4324a4c0a1eb0c50c0f1a2b3c4d5e6f7d87c5ea75f7378bb701e404c50639161af3eff66293e9f375b5f17eb50476f4020202020202020202020202020202020202020202020202020202020202020200",
            sha256Digest: 0x0c3861aa741eecccb33cc9685705120918400abcb22098962bc88fb13cb0e45a
        });
    }

    // -- golden vectors: accept ---------------------------------------------

    function test_goldenVectors_decodeFields() public view {
        Vector[4] memory vectors = goldenVectors();
        for (uint256 i = 0; i < vectors.length; i++) {
            Vector memory v = vectors[i];
            assertEq(v.encoded.length, market.JOURNAL_LENGTH(), v.name);

            CapabilityMarket.Journal memory j = market.decodeJournal(v.encoded);
            assertEq(j.imageId, v.vImageId, v.name);
            assertEq(j.marketParamsHash, v.vMarketParamsHash, v.name);
            assertEq(j.submissionHash, v.vSubmissionHash, v.name);
            assertEq(j.verdict, v.vVerdict, v.name);
        }
    }

    /// sha256 over the raw journal bytes — the exact journalDigest reveal() hands the
    /// RISC Zero verifier — must equal the Rust crate's Journal::digest().
    function test_goldenVectors_sha256Digest() public pure {
        Vector[4] memory vectors = goldenVectors();
        for (uint256 i = 0; i < vectors.length; i++) {
            Vector memory v = vectors[i];
            assertEq(sha256(v.encoded), v.sha256Digest, v.name);
        }
    }

    /// The test-side encoder (makeJournal) must reproduce the canonical bytes, so every
    /// other test in the suite exercises reveal() with spec-exact journals.
    function test_goldenVectors_makeJournalMatchesEncoding() public pure {
        Vector[4] memory vectors = goldenVectors();
        for (uint256 i = 0; i < vectors.length; i++) {
            Vector memory v = vectors[i];
            assertEq(
                keccak256(makeJournal(v.vImageId, v.vMarketParamsHash, v.vSubmissionHash, v.vVerdict)),
                keccak256(v.encoded),
                v.name
            );
        }
    }

    // -- rejection cases (mirror the Rust crate's decode_rejects_* tests) ----

    function test_decodeJournal_rejects96Bytes() public {
        bytes memory tooShort = new bytes(96);
        vm.expectRevert(CapabilityMarket.JournalWrongLength.selector);
        market.decodeJournal(tooShort);
    }

    function test_decodeJournal_rejects98Bytes() public {
        bytes memory tooLong = new bytes(98);
        tooLong[96] = 0x01; // plausible verdict byte; length alone must reject
        vm.expectRevert(CapabilityMarket.JournalWrongLength.selector);
        market.decodeJournal(tooLong);
    }

    function test_decodeJournal_rejectsEmpty() public {
        vm.expectRevert(CapabilityMarket.JournalWrongLength.selector);
        market.decodeJournal(bytes(""));
    }

    /// The pre-fix encoding — abi.encode(Journal) is 4×32 = 128 bytes — must be rejected.
    function test_decodeJournal_rejectsLegacyAbiEncoding() public {
        bytes memory legacy = abi.encode(
            CapabilityMarket.Journal({
                imageId: imageId, marketParamsHash: marketParamsHash, submissionHash: solutionHash, verdict: true
            })
        );
        assertEq(legacy.length, 128);
        vm.expectRevert(CapabilityMarket.JournalWrongLength.selector);
        market.decodeJournal(legacy);
    }

    function test_decodeJournal_rejectsNonCanonicalVerdictByte() public {
        bytes1[3] memory bad = [bytes1(0x02), bytes1(0x80), bytes1(0xff)];
        for (uint256 i = 0; i < bad.length; i++) {
            bytes memory journal = new bytes(97);
            journal[96] = bad[i];
            vm.expectRevert(CapabilityMarket.JournalInvalidVerdictByte.selector);
            market.decodeJournal(journal);
        }
    }

    // -- the same strictness enforced through reveal() ------------------------

    function test_reveal_rejectsWrongLengthJournal() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id);

        // internally consistent seal (mock binds to sha256 of whatever bytes are sent),
        // so the revert comes from the decoder, not the verifier
        bytes memory legacy = abi.encode(
            CapabilityMarket.Journal({
                imageId: imageId, marketParamsHash: marketParamsHash, submissionHash: solutionHash, verdict: true
            })
        );
        expectRevealRevert(CapabilityMarket.JournalWrongLength.selector, miner, id, legacy);
    }

    function test_reveal_rejectsVerdictByte0x02() public {
        uint256 id = createDefaultMarket();
        setupStakedAndCommitted(id);

        bytes memory journal = abi.encodePacked(imageId, marketParamsHash, solutionHash, bytes1(0x02));
        assertEq(journal.length, 97);
        expectRevealRevert(CapabilityMarket.JournalInvalidVerdictByte.selector, miner, id, journal);
    }
}
