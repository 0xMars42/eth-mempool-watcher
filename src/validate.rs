//! Phase H.2 — validation post-block des patterns trackees.
//!
//! Pour chaque hash dans une [`crate::track::PendingCheck`] prete, on appelle
//! `eth_getTransactionReceipt(hash)` et on classifie l'outcome :
//!
//! - **MinedSuccess** : tx incluse, status=true (snipe/swap a probablement
//!   reussi — H.3 affinera en lisant les events Transfer pour le profit reel)
//! - **MinedReverted** : tx incluse, status=false (le bot a paye le gas mais
//!   le swap a fail — typiquement slippage "Too much requested" sur Uni V3,
//!   = il a perdu une race MEV)
//! - **NotMined** : aucun receipt = tx droppee du mempool sans inclusion
//!
//! Cette classification a elle seule debloque une narrative MEV forte :
//! quel % de tentatives reussissent, combien de gas brule sur des fails,
//! quels bots ont le pire taux d'echec...

use alloy::primitives::B256;
use alloy::providers::Provider;
use alloy::rpc::types::TransactionReceipt;
use eyre::Result;

/// Outcome de validation d'une tx individuelle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// `get_transaction_receipt` a retourne `None` — tx jamais incluse.
    NotMined,
    /// Tx incluse, `status` du receipt vaut `true`.
    MinedSuccess { block_number: u64 },
    /// Tx incluse mais `status` vaut `false` (execution_reverted).
    /// Sur Uni V3 c'est typiquement le slippage "Too much requested" =
    /// le bot a perdu sa race MEV.
    MinedReverted { block_number: u64 },
}

impl ValidationOutcome {
    /// Construit depuis un `Option<TransactionReceipt>`. Logique pure,
    /// testable sans network.
    pub fn from_receipt(maybe_receipt: Option<&TransactionReceipt>) -> Self {
        match maybe_receipt {
            None => ValidationOutcome::NotMined,
            Some(r) => {
                let block_number = r.block_number.unwrap_or(0);
                if r.status() {
                    ValidationOutcome::MinedSuccess { block_number }
                } else {
                    ValidationOutcome::MinedReverted { block_number }
                }
            }
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ValidationOutcome::NotMined => "NotMined",
            ValidationOutcome::MinedSuccess { .. } => "MinedSuccess",
            ValidationOutcome::MinedReverted { .. } => "MinedReverted",
        }
    }
}

/// Valide un hash via RPC.
pub async fn validate_hash<P: Provider>(provider: &P, hash: B256) -> Result<ValidationOutcome> {
    let receipt = provider.get_transaction_receipt(hash).await?;
    Ok(ValidationOutcome::from_receipt(receipt.as_ref()))
}

/// Valide tous les hashes d'un set (un PendingCheck) et retourne les outcomes
/// dans l'ordre. Si un appel RPC echoue, on le log et on continue avec
/// `NotMined` par defaut — on ne veut pas bloquer le binaire sur un timeout.
pub async fn validate_hashes<P: Provider>(
    provider: &P,
    hashes: &[B256],
) -> Vec<(B256, ValidationOutcome)> {
    let mut out = Vec::with_capacity(hashes.len());
    for h in hashes {
        let outcome = match validate_hash(provider, *h).await {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(hash = %h, err = %e, "RPC validate_hash failed, default NotMined");
                ValidationOutcome::NotMined
            }
        };
        out.push((*h, outcome));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_label_is_stable() {
        assert_eq!(ValidationOutcome::NotMined.label(), "NotMined");
        assert_eq!(
            ValidationOutcome::MinedSuccess { block_number: 1 }.label(),
            "MinedSuccess"
        );
        assert_eq!(
            ValidationOutcome::MinedReverted { block_number: 1 }.label(),
            "MinedReverted"
        );
    }

    #[test]
    fn from_receipt_none_yields_not_mined() {
        assert_eq!(
            ValidationOutcome::from_receipt(None),
            ValidationOutcome::NotMined
        );
    }

    // Test de la construction success/reverted via receipts synthetiques :
    // on s'appuie sur le fait que TransactionReceipt::status() retourne le
    // bool `status` du receipt. Ecrire un receipt synthetique demande de
    // remplir beaucoup de champs (alloy::rpc::types::TransactionReceipt est
    // generique). On skip pour Phase H.2 et on valide live.
}
