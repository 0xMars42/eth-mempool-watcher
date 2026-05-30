//! Decodage des transactions DEX router pending.
//!
//! Chaque sous-module gere un router specifique. Le module racine expose
//! [`DecodedSwap`] : un enum riche qui unifie le format des swaps decodes
//! pour le log et la Phase E (sandwich detection).

pub mod uniswap_v2;
pub mod uniswap_v3;
pub mod universal_router;

use alloy::primitives::{Address, U256};

/// Resultat du decodage d'un calldata router DEX.
///
/// `Unknown` ne signifie pas une erreur : juste qu'on n'a pas encore declare
/// le selector en sol! ; on le log avec son selector hex pour reperer les
/// patterns dignes d'etre ajoutes.
#[derive(Clone, Debug)]
pub enum DecodedSwap {
    /// Uniswap V2/V3 style "exactInput" : on fixe ce qu'on met, on garantit
    /// un minimum recu.
    ExactInput {
        protocol: &'static str,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        amount_out_min: U256,
        /// Fee tier en pips (Uni V3 seulement, None pour Uni V2).
        fee_pips: Option<u32>,
        recipient: Address,
    },
    /// Multi-hop V2 swap, avec path (au moins 2 tokens).
    ExactInputPath {
        protocol: &'static str,
        path: Vec<Address>,
        amount_in: U256,
        amount_out_min: U256,
        recipient: Address,
    },
    /// Universal Router : on a decode l'outer `execute()` mais pas
    /// chaque command individuelle (Phase C.2).
    UniversalRouterEnvelope {
        n_commands: usize,
        n_inputs: usize,
        deadline: Option<U256>,
    },
    /// Uniswap V3 multicall : enveloppe pour N inner calls. On compte juste
    /// le nombre d'inner calls en v0 ; decoder chacune sera Phase C.2.
    Multicall {
        protocol: &'static str,
        n_inner_calls: usize,
    },
    /// Selector connu mais pas decode encore (addLiquidity, etc.)
    Unknown { selector: [u8; 4] },
}

impl DecodedSwap {
    /// Label court pour les logs : `"UniV2 ExactInput WETH->X"` etc.
    pub fn short_label(&self) -> String {
        match self {
            DecodedSwap::ExactInput {
                protocol,
                token_in,
                token_out,
                ..
            } => format!("{protocol} ExactInput {token_in:#x}->{token_out:#x}"),
            DecodedSwap::ExactInputPath { protocol, path, .. } => {
                let first = path.first().map(|a| format!("{a:#x}")).unwrap_or_default();
                let last = path.last().map(|a| format!("{a:#x}")).unwrap_or_default();
                format!(
                    "{protocol} ExactInputPath {first}->...->{last} ({} hops)",
                    path.len()
                )
            }
            DecodedSwap::UniversalRouterEnvelope {
                n_commands,
                n_inputs,
                ..
            } => format!("UniversalRouter execute ({n_commands} cmds, {n_inputs} inputs)"),
            DecodedSwap::Multicall {
                protocol,
                n_inner_calls,
            } => format!("{protocol} multicall ({n_inner_calls} inner calls)"),
            DecodedSwap::Unknown { selector } => format!(
                "Unknown 0x{:02x}{:02x}{:02x}{:02x}",
                selector[0], selector[1], selector[2], selector[3]
            ),
        }
    }
}

/// Decode un calldata en fonction du router cible.
pub fn decode(router: crate::routers::Router, input: &[u8]) -> DecodedSwap {
    use crate::routers::Router as R;
    if input.len() < 4 {
        return DecodedSwap::Unknown {
            selector: [0, 0, 0, 0],
        };
    }
    let selector: [u8; 4] = [input[0], input[1], input[2], input[3]];
    match router {
        R::UniswapV2Router02 => uniswap_v2::decode(selector, input),
        R::UniswapV3SwapRouter | R::UniswapV3SwapRouter02 => uniswap_v3::decode(selector, input),
        R::UniswapUniversalRouter => universal_router::decode(selector, input),
        R::OneInchV6 => DecodedSwap::Unknown { selector },
    }
}
