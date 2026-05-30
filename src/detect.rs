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

/// WETH canonique Ethereum mainnet.
pub const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
/// USDC natif Circle.
pub const USDC: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
/// USDT (Tether).
pub const USDT: Address = address!("dAC17F958D2ee523a2206206994597C13D831ec7");
/// DAI (MakerDAO).
pub const DAI: Address = address!("6B175474E89094C44Da98b954EedeAC495271d0F");

/// Tokens "quote" du marche : utilises comme reference de prix, pas comme
/// cible de sniping. Exclus du `token_out` de SniperCluster pour eviter les
/// faux-positifs (tout vendeur de memecoin pour WETH/stables compterait sinon).
pub const QUOTE_TOKENS: [Address; 4] = [WETH, USDC, USDT, DAI];

fn is_quote_token(a: Address) -> bool {
    QUOTE_TOKENS.contains(&a)
}

// --- Seuils v0 ---

const CLUSTER_MIN_SWAPS: usize = 3;
const CLUSTER_WINDOW: Duration = Duration::from_secs(30);

const REPETITION_MIN_SWAPS: usize = 2;
const REPETITION_WINDOW: Duration = Duration::from_secs(10);

/// 0.5 ETH = 5 * 10^17 wei.
const LARGE_SWAP_WETH_WEI: u128 = 500_000_000_000_000_000;

/// Details d'un swap decode (token_in/out + amount). Absent pour les
/// observations "from-only" issues d'envelopes (Universal Router, multicall)
/// qui permettent BotRepetition mais ni SniperCluster ni LargeWethSwap.
#[derive(Clone, Debug)]
pub struct SwapDetails {
    pub token_in: Address,
    pub token_out: Address,
    /// Quantite d'entree (wei pour WETH-in, raw token units sinon).
    pub amount_in: U256,
}

/// Une observation a alimenter au [`Detector`].
#[derive(Clone, Debug)]
pub struct Observation {
    pub from: Address,
    pub hash: B256,
    /// `Some(...)` quand on a decode le swap, `None` pour UR/multicall envelope.
    pub swap: Option<SwapDetails>,
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

        // 2. Large WETH swap : seulement si on a les details du swap.
        if let Some(s) = &obs.swap
            && s.token_in == WETH
            && s.amount_in >= U256::from(LARGE_SWAP_WETH_WEI)
        {
            detections.push(Detection::LargeWethSwap {
                from: obs.from,
                token_out: s.token_out,
                amount_in_wei: s.amount_in,
                hash: obs.hash,
            });
        }

        // 3. On ajoute apres pour que la detection prenne en compte l'entrante.
        self.history.push_back((now, obs.clone()));

        // 4. Sniper cluster : seulement si le token_out n'est PAS un quote token
        //    (= eviter les faux-positifs ou tout le monde vend pour WETH/USDC).
        if let Some(s_in) = &obs.swap
            && !is_quote_token(s_in.token_out)
        {
            let target_token_out = s_in.token_out;
            let cluster_cutoff = now - CLUSTER_WINDOW;
            let cluster: Vec<&(Instant, Observation)> = self
                .history
                .iter()
                .filter(|(t, o)| {
                    *t >= cluster_cutoff
                        && o.swap
                            .as_ref()
                            .is_some_and(|s| s.token_out == target_token_out)
                })
                .collect();
            if cluster.len() >= CLUSTER_MIN_SWAPS {
                detections.push(Detection::SniperCluster {
                    token_out: target_token_out,
                    n_swaps: cluster.len(),
                    sample_hashes: cluster.iter().rev().take(3).map(|(_, o)| o.hash).collect(),
                });
            }
        }

        // 5. Bot repetition : ne depend que de `from`, marche meme sans swap details.
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
            hash: hash(h),
            swap: Some(SwapDetails {
                token_in,
                token_out: addr(token_out),
                amount_in: U256::from(amount),
            }),
        }
    }

    /// Obs "from-only" (cas UR envelope) : pas de swap details.
    fn obs_no_swap(from: u8, h: u8) -> Observation {
        Observation {
            from: addr(from),
            hash: hash(h),
            swap: None,
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

    // --- Tests des fixes post-live-run --------------------------------------

    fn obs_full(
        from: u8,
        token_in: Address,
        token_out: Address,
        amount: u128,
        h: u8,
    ) -> Observation {
        Observation {
            from: addr(from),
            hash: hash(h),
            swap: Some(SwapDetails {
                token_in,
                token_out,
                amount_in: U256::from(amount),
            }),
        }
    }

    /// FIX FAUX-POSITIF : 3 swaps vers WETH ne doivent PAS declencher SniperCluster
    /// (WETH est quote token : "3 personnes vendent pour WETH" = bruit, pas sniping).
    #[test]
    fn sniper_cluster_excludes_weth_as_token_out() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs_full(1, addr(0xAB), WETH, 1, 1), t);
        let _ = d.observe_at(
            obs_full(2, addr(0xCD), WETH, 1, 2),
            t + Duration::from_secs(5),
        );
        let det = d.observe_at(
            obs_full(3, addr(0xEF), WETH, 1, 3),
            t + Duration::from_secs(10),
        );
        assert!(
            !det.iter()
                .any(|d| matches!(d, Detection::SniperCluster { .. })),
            "WETH comme token_out doit etre exclu, got {det:?}"
        );
    }

    /// Idem USDC : exclu.
    #[test]
    fn sniper_cluster_excludes_usdc_as_token_out() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs_full(1, addr(0xAB), USDC, 1, 1), t);
        let _ = d.observe_at(
            obs_full(2, addr(0xCD), USDC, 1, 2),
            t + Duration::from_secs(5),
        );
        let det = d.observe_at(
            obs_full(3, addr(0xEF), USDC, 1, 3),
            t + Duration::from_secs(10),
        );
        assert!(
            !det.iter()
                .any(|d| matches!(d, Detection::SniperCluster { .. })),
            "USDC comme token_out doit etre exclu, got {det:?}"
        );
    }

    /// FIX BOT-REPETITION : 2 obs from-only (UR envelope) doivent declencher
    /// BotRepetition meme sans swap details.
    #[test]
    fn bot_repetition_fires_on_envelope_only() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs_no_swap(42, 1), t);
        let det = d.observe_at(obs_no_swap(42, 2), t + Duration::from_secs(3));
        let rep = det
            .iter()
            .find(|d| matches!(d, Detection::BotRepetition { .. }))
            .expect("BotRepetition doit fire sur from-only");
        if let Detection::BotRepetition { n_swaps, from, .. } = rep {
            assert_eq!(*n_swaps, 2);
            assert_eq!(*from, addr(42));
        }
    }

    /// SniperCluster ne fire jamais sur des obs from-only (pas de token_out).
    #[test]
    fn sniper_cluster_skips_envelope_observations() {
        let mut d = Detector::new();
        let t = Instant::now();
        let _ = d.observe_at(obs_no_swap(1, 1), t);
        let _ = d.observe_at(obs_no_swap(2, 2), t + Duration::from_secs(1));
        let det = d.observe_at(obs_no_swap(3, 3), t + Duration::from_secs(2));
        assert!(
            !det.iter()
                .any(|d| matches!(d, Detection::SniperCluster { .. })),
            "envelope obs ne devraient pas creer de cluster, got {det:?}"
        );
    }
}
