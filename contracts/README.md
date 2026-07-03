# CapabilityMarket.sol

Parimutuel escrow contract for the TOON capability market ([toon-meta#120](https://github.com/toon-protocol/toon-meta/issues/120)). Holds USDC stakes directly, runs the parimutuel math, verifies RISC Zero proofs on reveal, and pays winners pull-based.

## Lifecycle

```
createMarket ── Open (stake, t <= lockWindowEnd)
             ── Committed (commit, lockWindowEnd < t <= deadline)
             ── Revealed (reveal, t <= deadline + commitRevealWindow)
                ├─ ResolvedYes  valid reveal: commitment binds msg.sender, RISC Zero
                │               verifier accepts, journal fields match, verdict == true
                └─ ResolvedNo   settleTimeout after the reveal window; permissionless
                                keeper earns resolutionBountyBps of the pool
withdraw ─────── winners pull: payout = s * (totalPool - bountyPaid) / winningPool
```

- **Commit-reveal front-running defense**: `commitmentHash = keccak256(solutionHash ‖ arweaveTx ‖ msg.sender ‖ salt)` — copying reveal calldata (or even the commitment hash) from the mempool fails, because `msg.sender` is baked into the preimage.
- **Reveal check sequence** per [toon-meta#119](https://github.com/toon-protocol/toon-meta/issues/119) story 6; journal struct `{imageId, marketParamsHash, submissionHash, verdict}` per [toon-meta#121](https://github.com/toon-protocol/toon-meta/issues/121), decoded with `abi.decode`.
- **On-chain eligibility checks** (#121) enforced at `createMarket`: nonzero image ID, committed `marketParamsHash`, deadline sanity, `commitRevealWindow > 0`, bounty ≤ `MAX_BOUNTY_BPS` (100 = 1%). Arweave retrievability of the predicate bytes is an off-chain pre-broadcast check.
- **Edge cases**: an empty winning pool refunds the other side pro-rata (no fund lockup); a keeper bounty exceeding the losing pool gives winners a pro-rata haircut instead of reverting; floor-division dust stays in the contract and never overdraws.

## Layout

- `src/CapabilityMarket.sol` — the escrow
- `src/interfaces/` — minimal vendored `IERC20`, `IRiscZeroVerifier` (matches risc0-ethereum: `verify` reverts on invalid seal)
- `test/` — unit + window/MEV cases, parimutuel fuzz, lifecycle, and a solvency invariant suite with a full-lifecycle handler; mocks under `test/mocks/` (6-decimal USDC, seal-checking RISC Zero verifier)
- `lib/forge-std` — vendored (no submodule)

## Develop

```sh
forge build   # warning-clean; block-timestamp lint excluded by design (timestamp-driven windows)
forge test
```

Deployment binds the constructor to Base's native Circle USDC and the version-pinned audited RISC Zero verifier (#119 story 2).
