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

## Phase A — what works now (smoke test, full tx bodies)

- Connects to a public WebSocket endpoint
- Subscribes to `newPendingTransactions` **with `fullTransactions=true`** (Geth/Reth
  extension, supported by publicnode.com)
- Logs throughput + body stats every 2 seconds — no extra RPC needed for the body

Observed live on `wss://ethereum-rpc.publicnode.com` (free, no API key):

```text
INFO mempool tick total=69 window=11 tx_per_sec="4.5" pct_with_to="99%"
     avg_input_bytes=594 sample_from=0x2212...7643 sample_hash=0x1dc7...0743
```

→ ~5–15 full tx bodies per second, 98–99% have a `to` field (i.e. not contract
deployments), median calldata ~600 bytes (the size of typical DEX swaps).

## Roadmap

| Phase | Status | What |
|---|---|---|
| A | ✅ | WS connect + **full** pending tx bodies stream |
| B | ⏳ | Filter by router (Uniswap V2 / V3 / Universal Router) |
| C | 📋 | Decode swap calldata (`alloy::sol!` on routers) |
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
