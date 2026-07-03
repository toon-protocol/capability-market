// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "./interfaces/IERC20.sol";
import {IRiscZeroVerifier} from "./interfaces/IRiscZeroVerifier.sol";

/// @title CapabilityMarket — parimutuel escrow with commit-reveal + RISC Zero settlement
/// @notice The dedicated escrow contract that IS the capability market. Holds USDC stakes
///         directly, runs parimutuel math, verifies RISC Zero proofs, pays winners.
///
///         Spec: toon-protocol/toon-meta#120 (interface + parimutuel math + state machine),
///               toon-protocol/toon-meta#119 story 6 (reveal check sequence),
///               toon-protocol/toon-meta#121 (journal struct + createMarket eligibility checks).
///
///         State machine (timestamp-driven, enforced at every entry point):
///           Open (t <= lockWindowEnd, stakes accepted)
///             -> Committed (lockWindowEnd < t <= deadline, commits accepted)
///             -> Revealed (t <= deadline + commitRevealWindow, reveals accepted)
///               -> ResolvedYes (valid reveal landed)
///               -> ResolvedNo  (timeout, keeper called settleTimeout)
contract CapabilityMarket {
    // ---------------------------------------------------------------------
    // Types
    // ---------------------------------------------------------------------

    enum Side {
        YES,
        NO
    }

    enum Resolution {
        Unresolved,
        ResolvedYes,
        ResolvedNo
    }

    struct Market {
        address creator;
        bytes32 imageId; // RISC Zero image ID (#119)
        bytes32 predicateArweaveTx; // where predicate bytes live
        bytes32 marketParamsHash; // hash of {problem params, deadline} — input manifest hash (#121)
        uint256 deadline; // commit window closes here
        uint256 commitRevealWindow; // grace period after deadline for reveals
        uint256 lockWindowEnd; // stakes close here — strictly < deadline
        uint256 resolutionBountyBps; // keeper's cut for permissionless settleTimeout
        uint256 yesPool;
        uint256 noPool;
        Resolution resolution;
        address winner; // msg.sender of the valid reveal (ResolvedYes only)
        uint256 bountyPaid; // keeper bounty actually paid out at resolution
    }

    struct Stake {
        Side side;
        uint256 amount;
        bool withdrawn;
    }

    struct Commitment {
        bytes32 commitmentHash; // keccak256(solutionHash ‖ arweaveTx ‖ msg.sender ‖ salt)
        uint256 timestamp;
    }

    /// @notice Journal per toon-meta#121 — must match the Rust journal crate's
    ///         `{image_id, market_params_hash, submission_hash, verdict}` exactly.
    ///         Canonical encoding (`journal-v1`): 97 tightly packed bytes —
    ///         image_id(32) ‖ market_params_hash(32) ‖ submission_hash(32) ‖ verdict(1),
    ///         verdict MUST be 0x00 (false) or 0x01 (true). The guest commits these raw
    ///         bytes via `env::commit_slice`, so `sha256(journal)` over them IS the
    ///         journal digest the RISC Zero verifier checks. Decoded by
    ///         [`decodeJournal`], which is strict byte-for-byte with the Rust
    ///         `Journal::decode` (golden vectors:
    ///         predicates/crates/journal/tests/golden_journal_vectors.json).
    struct Journal {
        bytes32 imageId;
        bytes32 marketParamsHash;
        bytes32 submissionHash;
        bool verdict;
    }

    // ---------------------------------------------------------------------
    // Constants / immutables
    // ---------------------------------------------------------------------

    /// @dev toon-meta#121 eligibility check 5: bounty sanity ceiling (100 = 1%).
    uint256 public constant MAX_BOUNTY_BPS = 100;
    uint256 public constant BPS_DENOMINATOR = 10_000;

    /// @notice Exact `journal-v1` encoded length: 3 × 32-byte digests + 1 verdict byte.
    uint256 public constant JOURNAL_LENGTH = 97;

    /// @notice The staking token (USDC on Base, 6 decimals).
    IERC20 public immutable usdc;
    /// @notice The audited RISC Zero verifier on Base (version-pinned; see #119 story 2).
    IRiscZeroVerifier public immutable verifier;

    // ---------------------------------------------------------------------
    // Storage
    // ---------------------------------------------------------------------

    uint256 public marketCount;
    mapping(uint256 marketId => Market) internal _markets;
    mapping(uint256 marketId => mapping(address staker => Stake)) internal _stakes;
    mapping(uint256 marketId => mapping(address committer => Commitment)) internal _commitments;

    /// @dev Reentrancy lock.
    uint256 private _locked = 1;

    // ---------------------------------------------------------------------
    // Events
    // ---------------------------------------------------------------------

    event MarketCreated(
        uint256 indexed marketId,
        address indexed creator,
        bytes32 imageId,
        bytes32 predicateArweaveTx,
        bytes32 marketParamsHash,
        uint256 deadline,
        uint256 commitRevealWindow,
        uint256 lockWindowEnd,
        uint256 resolutionBountyBps,
        uint256 seedNoStake
    );
    event Staked(uint256 indexed marketId, address indexed staker, Side side, uint256 amount);
    event Committed(uint256 indexed marketId, address indexed committer, bytes32 commitmentHash);
    event Revealed(uint256 indexed marketId, address indexed winner, bytes32 solutionHash, bytes32 arweaveTx);
    event TimeoutSettled(uint256 indexed marketId, address indexed keeper, uint256 bounty);
    event Withdrawn(uint256 indexed marketId, address indexed staker, uint256 amount);

    // ---------------------------------------------------------------------
    // Errors
    // ---------------------------------------------------------------------

    error MarketDoesNotExist();
    error ImageIdZero();
    error MarketParamsHashZero();
    error DeadlineNotInFuture();
    error LockWindowNotBeforeDeadline();
    error CommitRevealWindowZero();
    error BountyTooHigh();
    error ZeroAmount();
    error StakingClosed();
    error SideMismatch();
    error CommitWindowClosed();
    error CommitmentHashZero();
    error NoCommitment();
    error CommitmentMismatch();
    error CommitAfterDeadline();
    error RevealWindowClosed();
    error JournalWrongLength();
    error JournalInvalidVerdictByte();
    error JournalImageIdMismatch();
    error JournalMarketParamsHashMismatch();
    error JournalSubmissionHashMismatch();
    error JournalVerdictFalse();
    error AlreadyResolved();
    error NotResolved();
    error TimeoutWindowNotElapsed();
    error NothingToWithdraw();
    error TransferFailed();
    error Reentrancy();

    // ---------------------------------------------------------------------
    // Modifiers
    // ---------------------------------------------------------------------

    modifier nonReentrant() {
        if (_locked != 1) revert Reentrancy();
        _locked = 2;
        _;
        _locked = 1;
    }

    modifier marketExists(uint256 marketId) {
        if (marketId >= marketCount) revert MarketDoesNotExist();
        _;
    }

    // ---------------------------------------------------------------------
    // Constructor
    // ---------------------------------------------------------------------

    constructor(IERC20 usdc_, IRiscZeroVerifier verifier_) {
        usdc = usdc_;
        verifier = verifier_;
    }

    // ---------------------------------------------------------------------
    // Creation
    // ---------------------------------------------------------------------

    /// @notice Create a market. Anyone can create one.
    /// @dev Enforces the on-chain eligibility checks from toon-meta#121 (checks 1, 3, 4, 5).
    ///      Check 2 (predicate ELF retrievable from Arweave, risc0 compute_image_id(elf) ==
    ///      imageId) is an off-chain, pre-broadcast check — the contract cannot resolve Arweave.
    /// @param seedNoStake Author's cold-start liquidity, pulled via transferFrom into the NO
    ///        pool and recorded as the creator's NO stake.
    function createMarket(
        bytes32 imageId,
        bytes32 predicateArweaveTx,
        bytes32 marketParamsHash,
        uint256 deadline,
        uint256 commitRevealWindow,
        uint256 lockWindowEnd,
        uint256 resolutionBountyBps,
        uint256 seedNoStake
    ) external nonReentrant returns (uint256 marketId) {
        // #121 eligibility check 1: image ID present (32-byte hash, not a name).
        if (imageId == bytes32(0)) revert ImageIdZero();
        // #121 eligibility check 3: input manifest hash committed at creation, immutable after.
        if (marketParamsHash == bytes32(0)) revert MarketParamsHashZero();
        // #121 eligibility check 4: deadline sanity.
        if (deadline <= block.timestamp) revert DeadlineNotInFuture();
        if (lockWindowEnd >= deadline) revert LockWindowNotBeforeDeadline();
        if (commitRevealWindow == 0) revert CommitRevealWindowZero();
        // #121 eligibility check 5: bounty sanity.
        if (resolutionBountyBps > MAX_BOUNTY_BPS) revert BountyTooHigh();

        marketId = marketCount++;
        Market storage m = _markets[marketId];
        m.creator = msg.sender;
        m.imageId = imageId;
        m.predicateArweaveTx = predicateArweaveTx;
        m.marketParamsHash = marketParamsHash;
        m.deadline = deadline;
        m.commitRevealWindow = commitRevealWindow;
        m.lockWindowEnd = lockWindowEnd;
        m.resolutionBountyBps = resolutionBountyBps;

        emit MarketCreated(
            marketId,
            msg.sender,
            imageId,
            predicateArweaveTx,
            marketParamsHash,
            deadline,
            commitRevealWindow,
            lockWindowEnd,
            resolutionBountyBps,
            seedNoStake
        );

        if (seedNoStake > 0) {
            _stake(marketId, m, msg.sender, Side.NO, seedNoStake);
        }
    }

    // ---------------------------------------------------------------------
    // Staking
    // ---------------------------------------------------------------------

    /// @notice Stake USDC on a side. Direct deposit via transferFrom; no channel integration.
    ///         Accepted only while block.timestamp <= lockWindowEnd. A staker may add to their
    ///         position but may not switch sides.
    function stake(uint256 marketId, Side side, uint256 amount) external nonReentrant marketExists(marketId) {
        Market storage m = _markets[marketId];
        if (block.timestamp > m.lockWindowEnd) revert StakingClosed();
        _stake(marketId, m, msg.sender, side, amount);
    }

    function _stake(uint256 marketId, Market storage m, address staker, Side side, uint256 amount) internal {
        if (amount == 0) revert ZeroAmount();

        Stake storage s = _stakes[marketId][staker];
        if (s.amount > 0 && s.side != side) revert SideMismatch();
        s.side = side;
        s.amount += amount;

        if (side == Side.YES) {
            m.yesPool += amount;
        } else {
            m.noPool += amount;
        }

        if (!usdc.transferFrom(staker, address(this), amount)) revert TransferFailed();

        emit Staked(marketId, staker, side, amount);
    }

    // ---------------------------------------------------------------------
    // Commit
    // ---------------------------------------------------------------------

    /// @notice Commit to a solution — front-running defense.
    ///         Accepted only while lockWindowEnd < block.timestamp <= deadline.
    /// @param commitmentHash keccak256(abi.encodePacked(solutionHash, arweaveTx, msg.sender, salt))
    function commit(uint256 marketId, bytes32 commitmentHash) external marketExists(marketId) {
        Market storage m = _markets[marketId];
        if (block.timestamp <= m.lockWindowEnd || block.timestamp > m.deadline) {
            revert CommitWindowClosed();
        }
        if (commitmentHash == bytes32(0)) revert CommitmentHashZero();

        _commitments[marketId][msg.sender] = Commitment({commitmentHash: commitmentHash, timestamp: block.timestamp});

        emit Committed(marketId, msg.sender, commitmentHash);
    }

    // ---------------------------------------------------------------------
    // Reveal
    // ---------------------------------------------------------------------

    /// @notice Reveal a committed solution with its RISC Zero proof. Check sequence per
    ///         toon-meta#119 story 6:
    ///           1. recompute the commitment and check msg.sender binding
    ///           2. verify commit timestamp <= deadline
    ///           3. call the RISC Zero verifier with (proof, market imageId, sha256(journal))
    ///           4. decode the journal and check field-by-field against the market's committed
    ///              values and the submitted solutionHash
    ///           5. check journal verdict == true
    ///           6. mark market ResolvedYes with msg.sender as winner
    function reveal(
        uint256 marketId,
        bytes32 solutionHash,
        bytes32 arweaveTx,
        bytes32 salt,
        bytes calldata proof,
        bytes calldata journal
    ) external nonReentrant marketExists(marketId) {
        Market storage m = _markets[marketId];
        if (m.resolution != Resolution.Unresolved) revert AlreadyResolved();
        // Reveal window: commit window plus the commitRevealWindow grace period. After it
        // closes, settleTimeout owns resolution.
        if (block.timestamp > m.deadline + m.commitRevealWindow) revert RevealWindowClosed();

        // 1. Recompute the commitment; msg.sender is baked into the preimage, so a mempool
        //    bot replaying the calldata from its own address cannot match.
        Commitment storage c = _commitments[marketId][msg.sender];
        if (c.commitmentHash == bytes32(0)) revert NoCommitment();
        bytes32 recomputed = keccak256(abi.encodePacked(solutionHash, arweaveTx, msg.sender, salt));
        if (recomputed != c.commitmentHash) revert CommitmentMismatch();

        // 2. Commit must have landed inside the commit window.
        if (c.timestamp > m.deadline) revert CommitAfterDeadline();

        // 3. Verify the proof against the market's pinned image ID. Reverts when invalid.
        //    sha256 over the raw 97 committed bytes IS the journal digest.
        verifier.verify(proof, m.imageId, sha256(journal));

        // 4. Decode the journal (strict journal-v1, toon-meta#121) and check field-by-field.
        Journal memory j = decodeJournal(journal);
        if (j.imageId != m.imageId) revert JournalImageIdMismatch();
        if (j.marketParamsHash != m.marketParamsHash) revert JournalMarketParamsHashMismatch();
        if (j.submissionHash != solutionHash) revert JournalSubmissionHashMismatch();

        // 5. The predicate must have actually held.
        if (!j.verdict) revert JournalVerdictFalse();

        // 6. Resolve YES; the revealer is the winning miner.
        m.resolution = Resolution.ResolvedYes;
        m.winner = msg.sender;

        emit Revealed(marketId, msg.sender, solutionHash, arweaveTx);
    }

    /// @notice Strict `journal-v1` decoder. MUST accept/reject the exact same byte-string
    ///         set as the Rust journal crate's `Journal::decode`: exactly 97 bytes —
    ///         image_id(32) ‖ market_params_hash(32) ‖ submission_hash(32) ‖ verdict(1) —
    ///         and a verdict byte of 0x00 or 0x01; anything else reverts. Strictness
    ///         matters: were 0x02+ accepted as "true", two different byte strings would
    ///         decode to the same journal and the sha256 digest binding would no longer
    ///         be injective over decoded values. Conformance is pinned by the golden
    ///         vectors in predicates/crates/journal/tests/golden_journal_vectors.json.
    function decodeJournal(bytes calldata journal) public pure returns (Journal memory j) {
        if (journal.length != JOURNAL_LENGTH) revert JournalWrongLength();
        j.imageId = bytes32(journal[0:32]);
        j.marketParamsHash = bytes32(journal[32:64]);
        j.submissionHash = bytes32(journal[64:96]);
        uint8 verdictByte = uint8(journal[96]);
        if (verdictByte > 0x01) revert JournalInvalidVerdictByte();
        j.verdict = verdictByte == 0x01;
    }

    // ---------------------------------------------------------------------
    // Timeout settlement
    // ---------------------------------------------------------------------

    /// @notice Permissionless keeper settlement. Callable only after
    ///         deadline + commitRevealWindow with no valid reveal. Resolves NO and pays the
    ///         keeper resolutionBountyBps of the total pool.
    function settleTimeout(uint256 marketId) external nonReentrant marketExists(marketId) {
        Market storage m = _markets[marketId];
        if (m.resolution != Resolution.Unresolved) revert AlreadyResolved();
        if (block.timestamp <= m.deadline + m.commitRevealWindow) revert TimeoutWindowNotElapsed();

        m.resolution = Resolution.ResolvedNo;

        uint256 totalPool = m.yesPool + m.noPool;
        uint256 bounty = totalPool * m.resolutionBountyBps / BPS_DENOMINATOR;
        if (bounty > 0) {
            m.bountyPaid = bounty;
            if (!usdc.transfer(msg.sender, bounty)) revert TransferFailed();
        }

        emit TimeoutSettled(marketId, msg.sender, bounty);
    }

    // ---------------------------------------------------------------------
    // Withdraw
    // ---------------------------------------------------------------------

    /// @notice Pull-based winner payout. Parimutuel: for a winner with stake s in a winning
    ///         pool of size W, payout = s + losersPool * s / W, where
    ///         winnersShare = totalPool - keeperBounty and losersPool = winnersShare - W.
    ///         (Equivalently payout = s * winnersShare / W, which also degrades gracefully
    ///         when the keeper bounty exceeds the losing pool.)
    ///
    ///         Edge case — empty winning pool (W == 0): there is no winner to claim the pot,
    ///         so stakers on the losing side reclaim their stake pro-rata of what remains
    ///         after the keeper bounty. No funds are ever locked.
    function withdraw(uint256 marketId) external nonReentrant marketExists(marketId) {
        Stake storage s = _stakes[marketId][msg.sender];
        if (s.withdrawn) revert NothingToWithdraw();

        uint256 amount = _withdrawable(marketId, msg.sender);
        if (amount == 0) revert NothingToWithdraw();

        s.withdrawn = true;
        if (!usdc.transfer(msg.sender, amount)) revert TransferFailed();

        emit Withdrawn(marketId, msg.sender, amount);
    }

    // ---------------------------------------------------------------------
    // Views
    // ---------------------------------------------------------------------

    function getMarket(uint256 marketId) external view marketExists(marketId) returns (Market memory) {
        return _markets[marketId];
    }

    function getStake(uint256 marketId, address staker) external view marketExists(marketId) returns (Stake memory) {
        return _stakes[marketId][staker];
    }

    function getCommitment(uint256 marketId, address committer)
        external
        view
        marketExists(marketId)
        returns (Commitment memory)
    {
        return _commitments[marketId][committer];
    }

    /// @notice Amount `staker` can currently withdraw from a resolved market. Zero while the
    ///         market is unresolved, for losing-side stakers, and after withdrawal.
    function getWithdrawable(uint256 marketId, address staker) external view marketExists(marketId) returns (uint256) {
        if (_stakes[marketId][staker].withdrawn) return 0;
        return _withdrawable(marketId, staker);
    }

    // ---------------------------------------------------------------------
    // Internal
    // ---------------------------------------------------------------------

    function _withdrawable(uint256 marketId, address staker) internal view returns (uint256) {
        Market storage m = _markets[marketId];
        if (m.resolution == Resolution.Unresolved) return 0;

        Stake storage s = _stakes[marketId][staker];
        if (s.amount == 0) return 0;

        Side winningSide = m.resolution == Resolution.ResolvedYes ? Side.YES : Side.NO;
        uint256 winningPool = winningSide == Side.YES ? m.yesPool : m.noPool;
        uint256 totalPool = m.yesPool + m.noPool;
        uint256 winnersShare = totalPool - m.bountyPaid;

        if (winningPool == 0) {
            // No winner exists to claim the pot; losing-side stakers reclaim pro-rata of the
            // remainder after the keeper bounty (here the "losing" pool IS the total pool).
            return s.amount * winnersShare / totalPool;
        }

        if (s.side != winningSide) return 0;

        // payout = s + losersPool * s / W  ==  s * winnersShare / W
        // Floor division: rounding dust stays in the contract, never overdraws.
        return s.amount * winnersShare / winningPool;
    }
}
