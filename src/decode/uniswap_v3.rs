//! Decodage Uniswap V3 SwapRouter / SwapRouter02.
//!
//! V0 : on couvre `exactInputSingle` (selector 0x04e45aaf de SwapRouter02,
//! le plus commun en pratique mempool). Les autres (exactInput,
//! exactOutputSingle, exactOutput, multicall) viendront en Phase C.2.

use crate::decode::DecodedSwap;
use alloy::sol;
use alloy::sol_types::SolCall;

const PROTOCOL: &str = "UniV3";

sol! {
    /// SwapRouter02 exactInputSingle, selector 0x04e45aaf.
    /// Note : params est un tuple, sol! genere une struct compatible.
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }
    function exactInputSingle(ExactInputSingleParams params) external payable returns (uint256 amountOut);
}

pub fn decode(selector: [u8; 4], input: &[u8]) -> DecodedSwap {
    let s = u32::from_be_bytes(selector);
    match s {
        0x04e45aaf => match exactInputSingleCall::abi_decode(input) {
            Ok(c) => {
                let p = c.params;
                // fee_pips est uint24 dans alloy = Uint<24, 1>. .to::<u32>() est safe car 24 < 32.
                let fee_pips: u32 = p.fee.to();
                DecodedSwap::ExactInput {
                    protocol: PROTOCOL,
                    token_in: p.tokenIn,
                    token_out: p.tokenOut,
                    amount_in: p.amountIn,
                    amount_out_min: p.amountOutMinimum,
                    fee_pips: Some(fee_pips),
                    recipient: p.recipient,
                }
            }
            Err(_) => DecodedSwap::Unknown { selector },
        },
        _ => DecodedSwap::Unknown { selector },
    }
}
