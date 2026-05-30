//! Whitelist des routers DEX cibles sur Ethereum L1 mainnet.
//!
//! Un pending tx dont `to` est dans cette liste vaut le coup d'etre analyse
//! (decode swap, simulation Quoter, detection sandwich). Tout le reste du
//! mempool (transfers ETH/ERC20, NFT mints, MEV bots inconnus, etc.) est
//! filtre des Phase B pour ne pas saturer la pipeline.
//!
//! Adresses verifiees au 2026-05-30 via etherscan + docs Uniswap/1inch.

use alloy::primitives::{Address, address};

/// Nom court d'un router pour les logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Router {
    UniswapV2Router02,
    UniswapV3SwapRouter,
    UniswapV3SwapRouter02,
    UniswapUniversalRouter,
    OneInchV6,
}

impl Router {
    pub const fn name(self) -> &'static str {
        match self {
            Router::UniswapV2Router02 => "Uniswap V2 Router02",
            Router::UniswapV3SwapRouter => "Uniswap V3 SwapRouter",
            Router::UniswapV3SwapRouter02 => "Uniswap V3 SwapRouter02",
            Router::UniswapUniversalRouter => "Uniswap Universal Router",
            Router::OneInchV6 => "1inch Router v6",
        }
    }
}

const UNISWAP_V2_ROUTER02: Address = address!("7a250d5630B4cF539739dF2C5dAcb4c659F2488D");
const UNISWAP_V3_SWAP_ROUTER: Address = address!("E592427A0AEce92De3Edee1F18E0157C05861564");
const UNISWAP_V3_SWAP_ROUTER_02: Address = address!("68b3465833fb72A70ecDF485E0e4C7bD8665Fc45");
const UNISWAP_UNIVERSAL_ROUTER: Address = address!("66a9893cC07D91D95644AEDD05D03f95e1dBA8Af");
const ONEINCH_V6: Address = address!("111111125421cA6dc452d289314280a0f8842A65");

/// Renvoie le `Router` correspondant a une adresse, ou None si l'adresse
/// n'est pas dans la whitelist.
pub fn lookup(to: Address) -> Option<Router> {
    match to {
        UNISWAP_V2_ROUTER02 => Some(Router::UniswapV2Router02),
        UNISWAP_V3_SWAP_ROUTER => Some(Router::UniswapV3SwapRouter),
        UNISWAP_V3_SWAP_ROUTER_02 => Some(Router::UniswapV3SwapRouter02),
        UNISWAP_UNIVERSAL_ROUTER => Some(Router::UniswapUniversalRouter),
        ONEINCH_V6 => Some(Router::OneInchV6),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_routers_resolve() {
        assert_eq!(lookup(UNISWAP_V2_ROUTER02), Some(Router::UniswapV2Router02));
        assert_eq!(
            lookup(UNISWAP_V3_SWAP_ROUTER),
            Some(Router::UniswapV3SwapRouter)
        );
        assert_eq!(
            lookup(UNISWAP_V3_SWAP_ROUTER_02),
            Some(Router::UniswapV3SwapRouter02)
        );
        assert_eq!(
            lookup(UNISWAP_UNIVERSAL_ROUTER),
            Some(Router::UniswapUniversalRouter)
        );
        assert_eq!(lookup(ONEINCH_V6), Some(Router::OneInchV6));
    }

    #[test]
    fn unknown_address_returns_none() {
        // WETH n'est pas un router, doit retourner None.
        let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        assert_eq!(lookup(weth), None);
    }

    #[test]
    fn router_names_are_stable() {
        // Si on touche aux noms, c'est conscient (Phase C les utilise dans les logs).
        assert_eq!(
            Router::UniswapV3SwapRouter02.name(),
            "Uniswap V3 SwapRouter02"
        );
        assert_eq!(Router::OneInchV6.name(), "1inch Router v6");
    }
}
