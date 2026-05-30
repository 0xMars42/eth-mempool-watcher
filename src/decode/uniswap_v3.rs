//! Decodage Uniswap V3 SwapRouter (V1) + SwapRouter02.
//!
//! On gere les selectors single-pool (`exactInputSingle`, `exactOutputSingle`)
//! des deux versions, plus `multicall` (envelope only). Les multi-hop
//! (`exactInput`, `exactOutput`) ont un path packe en bytes — Phase C.2.
//!
//! V0 mapping :
//! - `exactInputSingle` (SwapRouter02 / V1) -> `ExactInput`
//! - `exactOutputSingle` -> `ExactInput` approxime (amount_in = amountInMaximum,
//!   amount_out_min = amountOut). Le sens "cet acteur swap X pour Y" reste
//!   correct pour la detection.

use crate::decode::DecodedSwap;
use alloy::sol;
use alloy::sol_types::SolCall;

const PROTOCOL: &str = "UniV3";

sol! {
    // -- SwapRouter02 (sans deadline dans la struct) ----------------------------

    /// Selector 0x04e45aaf — SwapRouter02 exactInputSingle.
    struct ExactInputSingleParamsV2 {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }
    function exactInputSingleV2(ExactInputSingleParamsV2 params) external payable returns (uint256 amountOut);

    /// Selector 0x5023b4df — SwapRouter02 exactOutputSingle.
    struct ExactOutputSingleParamsV2 {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountOut;
        uint256 amountInMaximum;
        uint160 sqrtPriceLimitX96;
    }
    function exactOutputSingleV2(ExactOutputSingleParamsV2 params) external payable returns (uint256 amountIn);

    // -- SwapRouter V1 (avec deadline dans la struct) ---------------------------

    /// Selector 0x414bf389 — SwapRouter V1 exactInputSingle.
    struct ExactInputSingleParamsV1 {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }
    function exactInputSingleV1(ExactInputSingleParamsV1 params) external payable returns (uint256 amountOut);

    // -- multicall (envelope) ---------------------------------------------------

    /// Selector 0xac9650d8 — multicall qui contient N inner abi-encoded calls.
    function multicall(bytes[] data) external payable returns (bytes[] memory results);
}

// Important : les noms cote Solidity sont ce qui determine le selector. Pour
// avoir les bons selectors on aurait du nommer les 3 variantes "exactInputSingle"
// / "exactOutputSingle" en Solidity. Mais ici elles ont des signatures
// DIFFERENTES (params struct different) donc selector different aussi.
// -> on les nomme avec suffixe V1/V2 et on hardcode les selectors attendus
//   dans le match plutot que de s'appuyer sur la const generee.

// Selectors hardcodes (canoniques sur Ethereum mainnet).
const SEL_EXACT_INPUT_SINGLE_V2: u32 = 0x04e45aaf;
const SEL_EXACT_OUTPUT_SINGLE_V2: u32 = 0x5023b4df;
const SEL_EXACT_INPUT_SINGLE_V1: u32 = 0x414bf389;
const SEL_MULTICALL: u32 = 0xac9650d8;

pub fn decode(selector: [u8; 4], input: &[u8]) -> DecodedSwap {
    let s = u32::from_be_bytes(selector);

    // Pour les calls non-standard nommees ci-dessus, le selector calcule par sol!
    // sur "exactInputSingleV2(...)" est different du vrai 0x04e45aaf. On
    // skip donc abi_decode_signature : on decode directement les bytes apres
    // les 4 premiers (le selector) en parsant les params via la struct.
    // alloy expose abi_decode_raw() pour ca (selector NON inclus).
    let payload = &input[4..];

    match s {
        SEL_EXACT_INPUT_SINGLE_V2 => {
            match <ExactInputSingleParamsV2 as alloy::sol_types::SolValue>::abi_decode(payload) {
                Ok(p) => DecodedSwap::ExactInput {
                    protocol: PROTOCOL,
                    token_in: p.tokenIn,
                    token_out: p.tokenOut,
                    amount_in: p.amountIn,
                    amount_out_min: p.amountOutMinimum,
                    fee_pips: Some(p.fee.to()),
                    recipient: p.recipient,
                },
                Err(_) => DecodedSwap::Unknown { selector },
            }
        }
        SEL_EXACT_OUTPUT_SINGLE_V2 => {
            match <ExactOutputSingleParamsV2 as alloy::sol_types::SolValue>::abi_decode(payload) {
                Ok(p) => DecodedSwap::ExactInput {
                    protocol: PROTOCOL,
                    token_in: p.tokenIn,
                    token_out: p.tokenOut,
                    // Approximation : on reporte amountInMaximum comme "ce que l'acteur
                    // est pret a depenser" et amountOut comme "ce qu'il veut recevoir".
                    // Suffit pour la detection.
                    amount_in: p.amountInMaximum,
                    amount_out_min: p.amountOut,
                    fee_pips: Some(p.fee.to()),
                    recipient: p.recipient,
                },
                Err(_) => DecodedSwap::Unknown { selector },
            }
        }
        SEL_EXACT_INPUT_SINGLE_V1 => {
            match <ExactInputSingleParamsV1 as alloy::sol_types::SolValue>::abi_decode(payload) {
                Ok(p) => DecodedSwap::ExactInput {
                    protocol: PROTOCOL,
                    token_in: p.tokenIn,
                    token_out: p.tokenOut,
                    amount_in: p.amountIn,
                    amount_out_min: p.amountOutMinimum,
                    fee_pips: Some(p.fee.to()),
                    recipient: p.recipient,
                },
                Err(_) => DecodedSwap::Unknown { selector },
            }
        }
        SEL_MULTICALL => match multicallCall::abi_decode(input) {
            Ok(c) => DecodedSwap::Multicall {
                protocol: PROTOCOL,
                n_inner_calls: c.data.len(),
            },
            Err(_) => DecodedSwap::Unknown { selector },
        },
        _ => DecodedSwap::Unknown { selector },
    }
}
