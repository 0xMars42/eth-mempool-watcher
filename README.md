# eth-mempool-watcher

> Real-time Ethereum L1 mempool monitor + sandwich pattern detector, in Rust.
> Streams pending transactions, decodes DEX router calls, and flags MEV
> attack candidates pre-block.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **Work in progress.** Phase A (smoke test) is done. Roadmap in
> `Desktop\Pro\remote-quant-roadmap\P2-mempool-monitor.md`.

## Why this exists

Sister project to [`base-arb-scanner`](https://github.com/0xMars42/base-arb-scanner)
which observed Base **post-block**. This one observes Ethereum L1
**pre-block** (the mempool) — where the real MEV game is played.

Base has no public mempool (Coinbase sequencer is centralized). Ethereum L1
does — and `wss://ethereum-rpc.publicnode.com` exposes
`eth_subscribe newPendingTransactions` for free, no API key needed.

## Phase A + B — what works now

- Connects to a public WebSocket endpoint
- Subscribes to `newPendingTransactions` **with `fullTransactions=true`** (Geth/Reth
  extension, supported by `publicnode.com` for free, no API key)
- For each pending tx, filters on a whitelist of DEX routers (Uniswap V2,
  Uniswap V3, Universal Router, 1inch v6)
- Logs each router hit with selector, signer, max-fee, and calldata size

Real 60-second sample (Ethereum mainnet, 2026-05-30 morning):

```text
INFO DEX router hit  router="Uniswap Universal Router"  selector="0x3593564c"
     from=0x8ca0...eaB44  value_wei=0  max_fee_gwei="0.48"  input_bytes=1252
     hash=0x5700f2b5...

INFO stats  total=437  router_hits=10  hit_rate_pct="2.3"
     per_router="Uniswap V2 Router02=2, Uniswap Universal Router=8"
```

What this tells us :
- **~7 full tx/s** in the public mempool, ~2% touch a DEX router
- **80%+ of DEX volume** flows through Universal Router (`0x3593564c` is its
  `execute(bytes, bytes[], uint256)` selector — the multi-step command encoding)
- Network was calm (max-fees of 0.28–0.49 gwei)
- Repeated identical-calldata tx from a few addresses = **sniper bots in the wild**,
  groundwork for the Phase E sandwich detector

## Roadmap

| Phase | Status | What |
|---|---|---|
| A | ✅ | WS connect + **full** pending tx bodies stream |
| B | ✅ | Filter by DEX router whitelist (Uni V2/V3/Universal, 1inch v6) |
| C | ✅ | Decode swap calldata — Uni V2 (3 selectors) + Uni V3 (exactInputSingle) + UR envelope |
| C.1 | 📋 | Decode Uni V2 fee-on-transfer variants + Uni V3 multicall + UR per-command |
| D | 📋 | Quoter-based price impact simulation |
| E | 📋 | Sandwich candidate detection (heuristic) |
| F | 📋 | Stats + dashboard |
| G | 📋 | CI + README polish + push public |

## Quick start

Requires Rust 1.95+.

```bash
git clone https://github.com/0xMars42/eth-mempool-watcher.git
cd eth-mempool-watcher
cargo run --release
```

Works out of the box on the public `publicnode.com` endpoint.

## Configuration

Environment variables (see `.env.example`):

| Variable | Default | Purpose |
|---|---|---|
| `ETH_WS_URL` | `wss://ethereum-rpc.publicnode.com` | WebSocket endpoint |
| `RUST_LOG` | `info` | tracing log level |

## License

MIT.

## Author

[0xMars42](https://github.com/0xMars42) — portfolio project for Rust/EVM/MEV roles.
