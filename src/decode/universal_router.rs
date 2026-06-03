//! Decodage Uniswap Universal Router (UR v2) — Phase C.2.
//!
//! Le UR utilise un encoding custom : `execute(bytes commands, bytes[] inputs)`
//! où `commands` est une séquence d'opcodes 1-byte et `inputs[i]` est
//! l'ABI-encoded tuple de params pour la i-ème commande.
//!
//! On décode les 4 opcodes de swap :
//! - `0x00` V3_SWAP_EXACT_IN  : (recipient, amountIn, amountOutMin, bytes path, payerIsUser)
//! - `0x01` V3_SWAP_EXACT_OUT : (recipient, amountOut, amountInMax, bytes path, payerIsUser)
//! - `0x08` V2_SWAP_EXACT_IN  : (recipient, amountIn, amountOutMin, address[] path, payerIsUser)
//! - `0x09` V2_SWAP_EXACT_OUT : (recipient, amountOut, amountInMax, address[] path, payerIsUser)
//!
//! Les bits 7-6 d'un opcode sont des flags (allow-revert, etc.) — on masque
//! avec `0x3F` pour obtenir la commande réelle.
//!
//! Si aucune commande de swap n'est décodable (PERMIT2, WRAP_ETH, etc.),
//! on retombe sur `UniversalRouterEnvelope` (comportement v0).

use crate::decode::DecodedSwap;
use alloy::primitives::Address;
use alloy::sol;
use alloy::sol_types::{SolCall, SolValue};

sol! {
    // ── Outer execute() ────────────────────────────────────────────────────

    /// Selector 0x24856bc3 — sans deadline. sol! génère `execute_0Call`.
    function execute(bytes commands, bytes[] inputs) external payable;

    /// Selector 0x3593564c — avec deadline. sol! génère `execute_1Call`.
    function execute(bytes commands, bytes[] inputs, uint256 deadline) external payable;

    /// EXECUTE_SUB_PLAN (0x10) — inner payload = abi.encode(bytes commands, bytes[] inputs).
    struct SubPlan {
        bytes commands;
        bytes[] inputs;
    }

    // ── Params des commandes de swap (tuples ABI-encodés, pas des calls) ───

    /// V3_SWAP_EXACT_IN (0x00) — ABI tuple des inputs[i].
    struct V3ExactIn {
        address recipient;
        uint256 amountIn;
        uint256 amountOutMin;
        bytes path;
        bool payerIsUser;
    }

    /// V3_SWAP_EXACT_OUT (0x01).
    struct V3ExactOut {
        address recipient;
        uint256 amountOut;
        uint256 amountInMax;
        bytes path;
        bool payerIsUser;
    }

    /// V2_SWAP_EXACT_IN (0x08).
    struct V2ExactIn {
        address recipient;
        uint256 amountIn;
        uint256 amountOutMin;
        address[] path;
        bool payerIsUser;
    }

    /// V2_SWAP_EXACT_OUT (0x09).
    struct V2ExactOut {
        address recipient;
        uint256 amountOut;
        uint256 amountInMax;
        address[] path;
        bool payerIsUser;
    }
}

/// Masque pour ignorer les flags de l'opcode (bits 7-6 = allow-revert + réservé).
const CMD_MASK: u8 = 0x3F;
const CMD_V3_EXACT_IN: u8 = 0x00;
const CMD_V3_EXACT_OUT: u8 = 0x01;
const CMD_V2_EXACT_IN: u8 = 0x08;
const CMD_V2_EXACT_OUT: u8 = 0x09;
const CMD_EXECUTE_SUB_PLAN: u8 = 0x10;

/// Parse un V3 path (bytes packé) → (tokenIn, tokenOut, fee_pips).
/// Format : `tokenIn(20) | fee(3) | [token(20) | fee(3)]* | tokenOut(20)`.
/// Minimum 43 bytes (single-hop). On extrait le fee uniquement en single-hop.
fn parse_v3_path(path: &[u8]) -> Option<(Address, Address, Option<u32>)> {
    if path.len() < 43 {
        return None;
    }
    let token_in = Address::from_slice(&path[..20]);
    let token_out = Address::from_slice(&path[path.len() - 20..]);
    let fee = if path.len() == 43 {
        // Single-hop : fee aux bytes 20-22.
        let fee = ((path[20] as u32) << 16) | ((path[21] as u32) << 8) | (path[22] as u32);
        Some(fee)
    } else {
        None // Multi-hop : plusieurs fees, on ne remonte pas de fee_pips.
    };
    Some((token_in, token_out, fee))
}

/// Tente de décoder une commande UR en swap. Renvoie `None` pour les commandes
/// non-swap (PERMIT2, WRAP_ETH, SWEEP, TRANSFER, etc.).
fn decode_command(cmd: u8, input: &[u8]) -> Option<DecodedSwap> {
    match cmd & CMD_MASK {
        CMD_V3_EXACT_IN => {
            let p = <V3ExactIn as SolValue>::abi_decode(input).ok()?;
            let (token_in, token_out, fee) = parse_v3_path(&p.path)?;
            Some(DecodedSwap::ExactInput {
                protocol: "UniV3",
                token_in,
                token_out,
                amount_in: p.amountIn,
                amount_out_min: p.amountOutMin,
                fee_pips: fee,
                recipient: p.recipient,
            })
        }
        CMD_V3_EXACT_OUT => {
            let p = <V3ExactOut as SolValue>::abi_decode(input).ok()?;
            let (token_in, token_out, fee) = parse_v3_path(&p.path)?;
            // ExactOut : amountInMax = "prêt à dépenser", amountOut = "veut recevoir".
            // On mappe sur ExactInput pour uniformiser (même sens logique).
            Some(DecodedSwap::ExactInput {
                protocol: "UniV3",
                token_in,
                token_out,
                amount_in: p.amountInMax,
                amount_out_min: p.amountOut,
                fee_pips: fee,
                recipient: p.recipient,
            })
        }
        CMD_V2_EXACT_IN => {
            let p = <V2ExactIn as SolValue>::abi_decode(input).ok()?;
            if p.path.len() < 2 {
                return None;
            }
            Some(DecodedSwap::ExactInputPath {
                protocol: "UniV2",
                path: p.path.to_vec(),
                amount_in: p.amountIn,
                amount_out_min: p.amountOutMin,
                recipient: p.recipient,
            })
        }
        CMD_V2_EXACT_OUT => {
            let p = <V2ExactOut as SolValue>::abi_decode(input).ok()?;
            if p.path.len() < 2 {
                return None;
            }
            Some(DecodedSwap::ExactInputPath {
                protocol: "UniV2",
                path: p.path.to_vec(),
                amount_in: p.amountInMax,
                amount_out_min: p.amountOut,
                recipient: p.recipient,
            })
        }
        CMD_EXECUTE_SUB_PLAN => {
            // Le payload est abi.encode(bytes commands, bytes[] inputs) — un SubPlan.
            // On descend d'un niveau et on cherche un swap dans le sous-plan.
            let sub = <SubPlan as SolValue>::abi_decode(input).ok()?;
            first_swap(&sub.commands, &sub.inputs)
        }
        _ => None, // PERMIT2_*, WRAP_ETH, UNWRAP_WETH, SWEEP, SEAPORT, etc.
    }
}

/// Cherche la première commande de swap dans les commandes UR.
/// Retourne `None` si aucun swap décodable (PERMIT2-only, WRAP_ETH seul, etc.).
fn first_swap(commands: &[u8], inputs: &[alloy::primitives::Bytes]) -> Option<DecodedSwap> {
    commands
        .iter()
        .zip(inputs.iter())
        .find_map(|(&cmd, input)| decode_command(cmd, input))
}

pub fn decode(selector: [u8; 4], input: &[u8]) -> DecodedSwap {
    let s = u32::from_be_bytes(selector);
    match s {
        0x24856bc3 => match execute_0Call::abi_decode(input) {
            Ok(c) => {
                first_swap(&c.commands, &c.inputs).unwrap_or(DecodedSwap::UniversalRouterEnvelope {
                    n_commands: c.commands.len(),
                    n_inputs: c.inputs.len(),
                    deadline: None,
                })
            }
            Err(_) => DecodedSwap::Unknown { selector },
        },
        0x3593564c => match execute_1Call::abi_decode(input) {
            Ok(c) => {
                first_swap(&c.commands, &c.inputs).unwrap_or(DecodedSwap::UniversalRouterEnvelope {
                    n_commands: c.commands.len(),
                    n_inputs: c.inputs.len(),
                    deadline: Some(c.deadline),
                })
            }
            Err(_) => DecodedSwap::Unknown { selector },
        },
        _ => DecodedSwap::Unknown { selector },
    }
}
