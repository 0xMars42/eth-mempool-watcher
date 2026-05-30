# eth-mempool-watcher

> Real-time Ethereum L1 mempool monitor in Rust. Streams pending transactions
> with full bodies, decodes DEX swap calldata across 6 selectors, and flags
> MEV patterns (sniper clusters, bot repetition, large WETH swaps) on the fly.

[![CI](https://github.com/0xMars42/eth-mempool-watcher/actions/workflows/ci.yml/badge.svg)](https://github.com/0xMars42/eth-mempool-watcher/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Sister project to [`base-arb-scanner`](https://github.com/0xMars42/base-arb-scanner)
which watches **Base post-block**. This one watches **Ethereum L1 pre-block** —
the mempool, where the real MEV game is played.

---

## What it does

On every new pending transaction (received via WebSocket, no polling, no API key):

1. **Filters** on a whitelist of DEX routers (Uniswap V2 Router02, Uniswap V3
   SwapRouter / SwapRouter02, Universal Router, 1inch v6).
2. **Decodes** the calldata when it matches a known swap selector — extracts
   `token_in`, `token_out`, `amount_in`, `amount_out_min`, `fee_tier`, `recipient`.
3. **Feeds** the decoded swap into a rolling-window detector that emits
   three kinds of pattern alerts.

All logic that doesn't depend on the network is **pure and tested** — 14 unit
tests covering routers, decoder edge cases, and detector behavior (including
the post-live-run fixes documented below).

## Live capture (Ethereum mainnet, 2026-05-30 morning)

A short ~15-minute run on `wss://ethereum-rpc.publicnode.com` produced this :

```
total pending tx scanned   17 748
DEX router hits               137  (0.8 % hit rate)
distribution                  V2=67, UR=56, V3=9, 1inch=5
patterns detected               5  ← the interesting bit
```

### The 5 patterns

**1. SniperCluster** — 2 distinct addresses racing for the same fresh memecoin :

```text
INFO 🎯 PATTERN detected kind="SniperCluster"
     token_out=0x42bBFa2e77757C645eeaAd1655E0911a7553Efbc
     n_swaps=3
     # cumul on this token over the window:
     #   3× from 0xb1b2d032AA...   (same wallet retrying)
     #   2× from 0x72283052cD...   (competing wallet)
```

**2–5. BotRepetition burst** — 4 distinct bots fired 2 swaps each, all within
**281 milliseconds** at 09:35:14, indicating a coordinated reaction to a
market event (likely a fresh token deployment) :

```text
09:35:14.438  PATTERN detected kind="BotRepetition" from=0x8ca0A5d1...
09:35:14.570  PATTERN detected kind="BotRepetition" from=0xB8a52FfF...
09:35:14.645  PATTERN detected kind="BotRepetition" from=0x0dCfbEf3...
09:35:14.719  PATTERN detected kind="BotRepetition" from=0x086bc4c2...
```

This is the signature MEV pattern the detector was designed to surface :
multiple automated wallets reacting to the same on-chain event in real time.

### The fix loop that got us here

The same run, on a previous build, surfaced **2 false-positive SniperClusters
on `token_out = WETH`** — because every wallet selling its memecoin for WETH
counted toward the cluster. WETH is a quote currency, not a sniper target.

Fix committed as [`94dd057`](https://github.com/0xMars42/eth-mempool-watcher/commit/94dd057) :

1. Added `QUOTE_TOKENS = [WETH, USDC, USDT, DAI]` and excluded them from
   `SniperCluster.token_out`.
2. Refactored `Observation.swap` to `Option<SwapDetails>` so Universal Router
   envelopes (where token info isn't decoded yet) can still feed
   `BotRepetition` (which only needs `from`).

Result on the next run : 0 false positives on quote tokens, and the
4-bot burst above became visible because `BotRepetition` now sees UR traffic.

### Other decoded outputs

A V2 path swap, fully decoded :

```text
INFO DEX swap decoded
     router="Uniswap V2 Router02"
     label="UniV2 ExactInputPath 0xc02aaa39...->0x42bbfa2e77... (2 hops)"
     token_in=0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2   # WETH
     token_out=0x42bBFa2e77757C645eeaAd1655E0911a7553Efbc  # memecoin
     amount_in=300000000000000000                          # 0.3 WETH
     from=0xb1b2d032AA2F52347fbcfd08E5C3Cc55216E8404
     max_fee_gwei="0.34"
```

A Universal Router envelope (~80 % of DEX flow in the public mempool — the
per-command decode is Phase C.2) :

```text
INFO UR envelope
     router="Uniswap Universal Router"
     label="UniversalRouter execute (1 cmds, 1 inputs)"
     n_commands=1 n_inputs=1
```

## Architecture

```
src/
├── lib.rs                     # crate root
├── main.rs                    # WebSocket loop + dispatcher
├── routers.rs                 # DEX router whitelist + lookup (3 tests)
├── decode/
│   ├── mod.rs                 # DecodedSwap enum + decode() dispatcher
│   ├── uniswap_v2.rs          # 6 V2 selectors (incl. fee-on-transfer)
│   ├── uniswap_v3.rs          # V3 single/multi + multicall envelope
│   └── universal_router.rs    # UR execute() with/without deadline
└── detect.rs                  # rolling-window detector (11 tests)
```

Design choices:

- **No polling, no `eth_getTransactionByHash`** — we use the Geth/Reth
  extension `eth_subscribe newPendingTransactions` with `fullTransactions=true`,
  supported by `publicnode.com` for free. The body comes with the
  notification, so zero extra RPC per pending tx.
- **`tokio` `current_thread`** runtime — I/O is serial, no need for `Send`.
- **Detector logic is pure** — `observe_at(obs, now)` is testable
  deterministically by injecting `Instant`.
- **Decoder is selector-keyed**, with `sol!`-generated ABIs that handle Solidity
  overloads correctly (Universal Router's two `execute()` variants).

## Selectors covered

| Router | Function | Selector | Status |
|---|---|---|---|
| Uni V2 | `swapExactETHForTokens` | `0x7ff36ab5` | ✅ |
| Uni V2 | `swapExactETHForTokensSupportingFeeOnTransferTokens` | `0xb6f9de95` | ✅ |
| Uni V2 | `swapExactTokensForETH` | `0x18cbafe5` | ✅ |
| Uni V2 | `swapExactTokensForETHSupportingFeeOnTransferTokens` | `0x791ac947` | ✅ |
| Uni V2 | `swapExactTokensForTokens` | `0x38ed1739` | ✅ |
| Uni V2 | `swapExactTokensForTokensSupportingFeeOnTransferTokens` | `0x5c11d795` | ✅ |
| Uni V3 SwapRouter02 | `exactInputSingle` | `0x04e45aaf` | ✅ |
| Uni V3 SwapRouter02 | `exactOutputSingle` | `0x5023b4df` | ✅ (mapped to ExactInput) |
| Uni V3 SwapRouter (V1) | `exactInputSingle` | `0x414bf389` | ✅ |
| Uni V3 SwapRouter02 | `multicall` | `0xac9650d8` | ✅ (envelope only) |
| Universal Router | `execute(bytes,bytes[])` | `0x24856bc3` | ✅ (envelope only) |
| Universal Router | `execute(bytes,bytes[],uint256)` | `0x3593564c` | ✅ (envelope only) |
| Uni V3 SwapRouter02 | `exactInput` / `exactOutput` (packed-bytes path) | `0xb858183f` / `0x09b81346` | 📋 Phase C.2 |
| Universal Router | per-command decode (V3_SWAP_*, V2_SWAP_*, PERMIT2_*) | — | 📋 Phase C.2 |

The Universal Router covers ~80% of DEX traffic in the public mempool, so
per-command decoding (Phase C.2) is the biggest remaining unlock.

## Detection heuristics

| Detection | Trigger | Use case |
|---|---|---|
| `SniperCluster` | ≥ 3 swaps to the same `token_out` within 30s | coordinated bots on a fresh memecoin |
| `BotRepetition` | ≥ 2 swaps from the same `from` within 10s | a single automated wallet |
| `LargeWethSwap` | `token_in == WETH` and `amount ≥ 0.5 ETH` | a swap big enough to be a sandwich target |

The detector is **market-dependent** — quiet periods may produce zero alerts.
A morning run during a calm phase observed 11 router hits in 90s with no
fires; an earlier run saw a 3-tx coordinated buy on the same memecoin from
3 different addresses, which the same code would now flag as a
`SniperCluster`.

## Quick start

Requires Rust 1.95+.

```bash
git clone https://github.com/0xMars42/eth-mempool-watcher.git
cd eth-mempool-watcher
cargo run --release
```

Works out of the box on `wss://ethereum-rpc.publicnode.com` — no API key,
no Alchemy / QuickNode account required. Override with `ETH_WS_URL` in `.env`
if you want a dedicated provider.

## Quality

- `cargo fmt --check` clean
- `cargo clippy --all-targets -- -D warnings` clean (zero warnings)
- **14 unit tests** covering router lookup, decoder mapping, detector
  heuristics, and the post-live-run regression cases (quote-token exclusion +
  envelope-only observations)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Roadmap

| Phase | Status | What |
|---|---|---|
| A | ✅ | WebSocket connect + **full** pending tx bodies stream |
| B | ✅ | Filter by DEX router whitelist (Uni V2/V3/Universal, 1inch v6) |
| C | ✅ | Decode swap calldata — Uni V2 (3 selectors), Uni V3 (single), UR envelope |
| C.1 | ✅ | Extra selectors — V2 fee-on-transfer (3), V3 exactOut + V1, V3 multicall |
| E | ✅ | MEV pattern detection (SniperCluster, BotRepetition, LargeWethSwap) |
| G | ✅ | CI + LICENSE + README polish + public push |
| E.1 | ✅ | Post-live-run fixes : quote-token exclusion + envelope-only obs |
| C.2 | 📋 | UR per-command decode + V3 packed-path swaps (the big unlock) |
| D | 📋 | Quoter-based price impact simulation |
| F | 📋 | Periodic stats summary + end-of-run report |

## License

MIT. See [LICENSE](LICENSE).

## Author

[0xMars42](https://github.com/0xMars42) — portfolio project for roles in
**Rust / EVM infrastructure / MEV / crypto-quant**.
