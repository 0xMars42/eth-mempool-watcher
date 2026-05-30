//! Detection heuristique de patterns MEV dans le flux mempool.
//!
//! Trois heuristiques v0, logiques pures (testables sans network) :
//!
//! 1. **Sniper cluster** : N swaps vers le meme `token_out` en moins de
//!    `CLUSTER_WINDOW` -> bots coordonnes sur un memecoin.
//! 2. **Bot repetition** : meme `from` envoie K swaps en moins de
//!    `REPETITION_WINDOW` -> wallet probablement automatise.
//! 3. **Large WETH swap** : `token_in == WETH` et amount > seuil -> swap
//!    suffisamment gros pour etre une cible de sandwich.
//!
//! Phase E.1 ajoutera la vraie detection sandwich (tx_A_buy + tx_victim +
//! tx_A_sell sur meme paire) quand on aura plus de signal.

use alloy::primitives::{Address, B256, U256, address};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// WETH canonique Ethereum mainnet (utilise pour la regle large-swap).
pub const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

// --- Seuils v0 (tweakable via Detector::with_thresholds) ---

const CLUSTER_MIN_SWAPS: usize = 3;
const CLUSTER_WINDOW: Duration = Duration::from_secs(30);

const REPETITION_MIN_SWAPS: usize = 2;
const REPETITION_WINDOW: Duration = Duration::from_secs(10);

/// 0.5 ETH = 5 * 10^17 wei.
const LARGE_SWAP_WETH_WEI: u128 = 500_000_000_000_000_000;

/// Tout swap suffisamment decode pour alimenter la detection.
#[derive(Clone, Debug)]
pub struct Observation {
    pub from: Address,
    pub token_in: Address,
    pub token_out: Address,
    /// Quantite d'entree (en wei pour WETH, en raw token units sinon).
    pub amount_in: U256,
    pub hash: B256,
}

/// Une detection emise par [`Detector::observe`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    /// Multiple swaps vers le meme `token_out` en peu de temps.
    SniperCluster {
        token_out: Address,
        n_swaps: usize,
        sample_hashes: Vec<B256>,
    },
    /// Une meme `from` repete des swaps tres vite.
    BotRepetition {
        from: Address,
        n_swaps: usize,
        sample_hashes: Vec<B256>,
    },
    /// Swap WETH-in suffisamment gros pour etre une cible sandwich.
    LargeWethSwap {
        from: Address,
        token_out: Address,
        amount_in_wei: U256,
        hash: B256,
    },
}

/// Etat interne du detecteur : buffer rolling d'observations + helpers.
pub struct Detector {
    /// Fenetre la plus large utilisee (= max des windows), au-dela on prune.
    max_window: Duration,
    history: VecDeque<(Instant, Observation)>,
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Detector {
    pub fn new() -> Self {
        Self {
            max_window: CLUSTER_WINDOW.max(REPETITION_WINDOW),
            history: VecDeque::new(),
        }
    }

    /// Ingere une observation et retourne les detections declenchees a cet
    /// instant (cluster ou repetition decouvertes apres ajout, et large-swap
    /// pour cette obs).
    pub fn observe(&mut self, obs: Observation) -> Vec<Detection> {
        self.observe_at(obs, Instant::now())
    }

    /// Variante avec horloge injectee, pour les tests deterministes.
    pub fn observe_at(&mut self, obs: Observation, now: Instant) -> Vec<Detection> {
        // 1. Prune ce qui est plus vieux que max_window.
        let cutoff = now.checked_sub(self.max_window);
        if let Some(cutoff) = cutoff {
            while let Some((t, _)) = self.history.front() {
                if *t < cutoff {
                    self.history.pop_front();
                } else {
                    break;
                }
            }
        }

        let mut detections = Vec::new();

        // 2. Large WETH swap : detection unique sur l'observation entrante.
        if obs.token_in == WETH && obs.amount_in >= U256::from(LARGE_SWAP_WETH_WEI) {
            detections.push(Detection::LargeWethSwap {
                from: obs.from,
                token_out: obs.token_out,
                amount_in_wei: obs.amount_in,
                hash: obs.hash,
            });
        }

        // 3. On ajoute apres pour que la detection prenne en compte l'entrante.
        self.history.push_back((now, obs.clone()));

        // 4. Sniper cluster : compter les obs vers obs.token_out dans CLUSTER_WINDOW.
        let cluster_cutoff = now - CLUSTER_WINDOW;
        let cluster: Vec<&(Instant, Observation)> = self
            .history
            .iter()
            .filter(|(t, o)| *t >= cluster_cutoff && o.token_out == obs.token_out)
            .collect();
        if cluster.len() >= CLUSTER_MIN_SWAPS {
            detections.push(Detection::SniperCluster {
                token_out: obs.token_out,
                n_swaps: cluster.len(),
                sample_hashes: cluster.iter().rev().take(3).map(|(_, o)| o.hash).collect(),
            });
        }

        // 5. Bot repetition : compter par `from` dans REPETITION_WINDOW.
        let rep_cutoff = now - REPETITION_WINDOW;
        let mut per_from: HashMap<Address, Vec<&Observation>> = HashMap::new();
        for (t, o) in &self.history {
            if *t >= rep_cutoff {
                per_from.entry(o.from).or_default().push(o);
            }
        }
        if let Some(list) = per_from.get(&obs.from)
            && list.len() >= REPETITION_MIN_SWAPS
        {
            detections.push(Detection::BotRepetition {
                from: obs.from,
                n_swaps: list.len(),
                sample_hashes: list.iter().rev().take(3).map(|o| o.hash).collect(),
            });
        }

        detections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address::repeat_byte(byte)
    }

    fn hash(byte: u8) -> B256 {
        B256::repeat_byte(byte)
    }

    fn obs(from: u8, token_in: Address, token_out: u8, amount: u128, h: u8) -> Observation {
        Observation {
            from: addr(from),
            token_in,
            token_out: addr(token_out),
            amount_in: U256::from(amount),
            hash: hash(h),
        }
    }

    #[test]
    fn empty_observation_yields_no_detection() {
        let mut d = Detector::new();
        let t = Instant::now();
        let det = d.observe_at(obs(1, addr(0xFF), 2, 1, 0xAA), t);
        assert!(det.is_empty(), "got {det:?}");
    }

    #[test]
    fn two_swaps_same_token_out_no_cluster() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs(1, addr(0xFF), 0xAB, 1, 1), t);
        let det = d.observe_at(obs(2, addr(0xFF), 0xAB, 1, 2), t + Duration::from_secs(1));
        // 2 swaps != cluster (min 3) — mais le second declenche peut-etre bot rep si meme from. Ici from differs, so vide.
        assert!(
            !det.iter()
                .any(|d| matches!(d, Detection::SniperCluster { .. })),
            "got {det:?}"
        );
    }

    #[test]
    fn three_swaps_same_token_out_in_window_triggers_cluster() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs(1, addr(0xFF), 0xAB, 1, 1), t);
        let _ = d.observe_at(obs(2, addr(0xFF), 0xAB, 1, 2), t + Duration::from_secs(5));
        let det = d.observe_at(obs(3, addr(0xFF), 0xAB, 1, 3), t + Duration::from_secs(10));
        let cluster = det
            .iter()
            .find(|d| matches!(d, Detection::SniperCluster { .. }))
            .expect("doit detecter le cluster");
        if let Detection::SniperCluster {
            n_swaps, token_out, ..
        } = cluster
        {
            assert_eq!(*n_swaps, 3);
            assert_eq!(*token_out, addr(0xAB));
        }
    }

    #[test]
    fn cluster_resets_after_window() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs(1, addr(0xFF), 0xAB, 1, 1), t);
        let _ = d.observe_at(obs(2, addr(0xFF), 0xAB, 1, 2), t + Duration::from_secs(5));
        // 3eme bien apres -> les 2 premieres sont prunees
        let det = d.observe_at(obs(3, addr(0xFF), 0xAB, 1, 3), t + Duration::from_secs(60));
        assert!(
            !det.iter()
                .any(|d| matches!(d, Detection::SniperCluster { .. })),
            "got {det:?}"
        );
    }

    #[test]
    fn same_from_twice_triggers_bot_repetition() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs(42, addr(0xFF), 0xAB, 1, 1), t);
        let det = d.observe_at(obs(42, addr(0xFF), 0xCD, 1, 2), t + Duration::from_secs(2));
        let rep = det
            .iter()
            .find(|d| matches!(d, Detection::BotRepetition { .. }))
            .expect("doit detecter la repetition");
        if let Detection::BotRepetition { n_swaps, from, .. } = rep {
            assert_eq!(*n_swaps, 2);
            assert_eq!(*from, addr(42));
        }
    }

    #[test]
    fn large_weth_swap_triggers_immediately() {
        let mut d = Detector::new();
        let t = Instant::now();
        // 1 ETH = 10^18 wei > seuil
        let det = d.observe_at(obs(7, WETH, 0xAB, 1_000_000_000_000_000_000, 9), t);
        let large = det
            .iter()
            .find(|d| matches!(d, Detection::LargeWethSwap { .. }))
            .expect("doit detecter le large swap");
        if let Detection::LargeWethSwap { amount_in_wei, .. } = large {
            assert_eq!(*amount_in_wei, U256::from(1_000_000_000_000_000_000u128));
        }
    }

    #[test]
    fn small_weth_swap_no_large_detection() {
        let mut d = Detector::new();
        let t = Instant::now();
        // 0.1 ETH < seuil 0.5
        let det = d.observe_at(obs(7, WETH, 0xAB, 100_000_000_000_000_000, 9), t);
        assert!(
            !det.iter()
                .any(|d| matches!(d, Detection::LargeWethSwap { .. })),
            "got {det:?}"
        );
    }
}
