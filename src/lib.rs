//! eth-mempool-watcher — racine de la librairie.
//!
//! Modules :
//! - [`routers`] : whitelist des routers DEX sur Ethereum L1 + lookup nom.
//! - [`decode`]  : decodage du calldata par router (Uni V2/V3/UR).
//! - [`detect`]  : detection heuristique de patterns MEV (sniper cluster, bot
//!   repetition, large WETH swap).

pub mod decode;
pub mod detect;
pub mod routers;
pub mod track;
pub mod validate;
