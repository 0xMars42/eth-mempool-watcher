//! Decodage Uniswap V2 Router02.
//!
//! On gere les 3 swaps spot les plus communs. Les variantes
//! `*SupportingFeeOnTransferTokens` ont le meme ABI (path/amounts), elles
//! seront ajoutees plus tard si besoin.

use crate::decode::DecodedSwap;
use alloy::sol;
use alloy::sol_types::SolCall;

const PROTOCOL: &str = "UniV2";

sol! {
    /// Selector 0x7ff36ab5
    function swapExactETHForTokens(
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external payable returns (uint256[] memory amounts);

    /// Selector 0x18cbafe5
    function swapExactTokensForETH(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory amounts);

    /// Selector 0x38ed1739
    function swapExactTokensForTokens(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory amounts);
}

pub fn decode(selector: [u8; 4], input: &[u8]) -> DecodedSwap {
    let s = u32::from_be_bytes(selector);
    match s {
        // swapExactETHForTokens : amountIn = msg.value, lu cote main.rs (tx.value)
        0x7ff36ab5 => match swapExactETHForTokensCall::abi_decode(input) {
            Ok(c) => DecodedSwap::ExactInputPath {
                protocol: PROTOCOL,
                path: c.path.to_vec(),
                amount_in: alloy::primitives::U256::ZERO, // dans tx.value
                amount_out_min: c.amountOutMin,
                recipient: c.to,
            },
            Err(_) => DecodedSwap::Unknown { selector },
        },
        // swapExactTokensForETH
        0x18cbafe5 => match swapExactTokensForETHCall::abi_decode(input) {
            Ok(c) => DecodedSwap::ExactInputPath {
                protocol: PROTOCOL,
                path: c.path.to_vec(),
                amount_in: c.amountIn,
                amount_out_min: c.amountOutMin,
                recipient: c.to,
            },
            Err(_) => DecodedSwap::Unknown { selector },
        },
        // swapExactTokensForTokens
        0x38ed1739 => match swapExactTokensForTokensCall::abi_decode(input) {
            Ok(c) => DecodedSwap::ExactInputPath {
                protocol: PROTOCOL,
                path: c.path.to_vec(),
                amount_in: c.amountIn,
                amount_out_min: c.amountOutMin,
                recipient: c.to,
            },
            Err(_) => DecodedSwap::Unknown { selector },
        },
        _ => DecodedSwap::Unknown { selector },
    }
}
