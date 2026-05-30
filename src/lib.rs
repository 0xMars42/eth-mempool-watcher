//! eth-mempool-watcher — racine de la librairie.
//!
//! Modules :
//! - [`routers`] : whitelist des routers DEX sur Ethereum L1 + lookup nom.
//! - [`decode`]  : decodage du calldata par router (Uni V2/V3/UR).

pub mod decode;
pub mod routers;
