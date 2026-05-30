//! Decodage Uniswap Universal Router (UR v2).
//!
//! Le UR utilise un encoding custom : `execute(bytes commands, bytes[] inputs)`
//! ou commands est une sequence d'opcodes 1-byte chacun, et inputs[i] est
//! l'ABI-encoded calldata pour la i-eme commande.
//!
//! V0 : on decode l'enveloppe (compte les commands, decompte inputs, extrait
//! le deadline si la version avec deadline). Le decodage de chaque command
//! individuelle (V3_SWAP_EXACT_IN, V2_SWAP_EXACT_IN, PERMIT2_PERMIT, etc.)
//! est Phase C.2.

use crate::decode::DecodedSwap;
use alloy::sol;
use alloy::sol_types::SolCall;

sol! {
    // Overload : meme nom Solidity, sol! disambigue cote Rust avec suffix _N.
    // Les selectors sont calcules sur la signature reelle (`execute(...)`).

    /// Selector 0x24856bc3 — sans deadline. sol! genere `execute_0Call`.
    function execute(bytes commands, bytes[] inputs) external payable;

    /// Selector 0x3593564c — avec deadline (la plus commune). sol! genere
    /// `execute_1Call`. Le nom Solidity reste `execute` dans les deux cas,
    /// donc les selectors keccak sont corrects.
    function execute(bytes commands, bytes[] inputs, uint256 deadline) external payable;
}

pub fn decode(selector: [u8; 4], input: &[u8]) -> DecodedSwap {
    let s = u32::from_be_bytes(selector);
    match s {
        0x24856bc3 => match execute_0Call::abi_decode(input) {
            Ok(c) => DecodedSwap::UniversalRouterEnvelope {
                n_commands: c.commands.len(),
                n_inputs: c.inputs.len(),
                deadline: None,
            },
            Err(_) => DecodedSwap::Unknown { selector },
        },
        0x3593564c => match execute_1Call::abi_decode(input) {
            Ok(c) => DecodedSwap::UniversalRouterEnvelope {
                n_commands: c.commands.len(),
                n_inputs: c.inputs.len(),
                deadline: Some(c.deadline),
            },
            Err(_) => DecodedSwap::Unknown { selector },
        },
        _ => DecodedSwap::Unknown { selector },
    }
}
