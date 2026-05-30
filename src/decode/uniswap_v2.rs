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

    /// Selector 0xb6f9de95 — variante fee-on-transfer, meme ABI.
    function swapExactETHForTokensSupportingFeeOnTransferTokens(
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external payable;

    /// Selector 0x18cbafe5
    function swapExactTokensForETH(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory amounts);

    /// Selector 0x791ac947 — variante fee-on-transfer, meme ABI.
    function swapExactTokensForETHSupportingFeeOnTransferTokens(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external;

    /// Selector 0x38ed1739
    function swapExactTokensForTokens(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory amounts);

    /// Selector 0x5c11d795 — variante fee-on-transfer, meme ABI.
    function swapExactTokensForTokensSupportingFeeOnTransferTokens(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external;
}

pub fn decode(selector: [u8; 4], input: &[u8]) -> DecodedSwap {
    let s = u32::from_be_bytes(selector);
    match s {
        // ETH-in swaps : amountIn = msg.value (recupere cote main.rs via tx.value)
        0x7ff36ab5 => match swapExactETHForTokensCall::abi_decode(input) {
            Ok(c) => path_swap(
                c.path.to_vec(),
                alloy::primitives::U256::ZERO,
                c.amountOutMin,
                c.to,
            ),
            Err(_) => DecodedSwap::Unknown { selector },
        },
        0xb6f9de95 => {
            match swapExactETHForTokensSupportingFeeOnTransferTokensCall::abi_decode(input) {
                Ok(c) => path_swap(
                    c.path.to_vec(),
                    alloy::primitives::U256::ZERO,
                    c.amountOutMin,
                    c.to,
                ),
                Err(_) => DecodedSwap::Unknown { selector },
            }
        }
        // Token-in swaps : amountIn dans calldata
        0x18cbafe5 => match swapExactTokensForETHCall::abi_decode(input) {
            Ok(c) => path_swap(c.path.to_vec(), c.amountIn, c.amountOutMin, c.to),
            Err(_) => DecodedSwap::Unknown { selector },
        },
        0x791ac947 => {
            match swapExactTokensForETHSupportingFeeOnTransferTokensCall::abi_decode(input) {
                Ok(c) => path_swap(c.path.to_vec(), c.amountIn, c.amountOutMin, c.to),
                Err(_) => DecodedSwap::Unknown { selector },
            }
        }
        0x38ed1739 => match swapExactTokensForTokensCall::abi_decode(input) {
            Ok(c) => path_swap(c.path.to_vec(), c.amountIn, c.amountOutMin, c.to),
            Err(_) => DecodedSwap::Unknown { selector },
        },
        0x5c11d795 => {
            match swapExactTokensForTokensSupportingFeeOnTransferTokensCall::abi_decode(input) {
                Ok(c) => path_swap(c.path.to_vec(), c.amountIn, c.amountOutMin, c.to),
                Err(_) => DecodedSwap::Unknown { selector },
            }
        }
        _ => DecodedSwap::Unknown { selector },
    }
}

fn path_swap(
    path: Vec<alloy::primitives::Address>,
    amount_in: alloy::primitives::U256,
    amount_out_min: alloy::primitives::U256,
    recipient: alloy::primitives::Address,
) -> DecodedSwap {
    DecodedSwap::ExactInputPath {
        protocol: PROTOCOL,
        path,
        amount_in,
        amount_out_min,
        recipient,
    }
}
