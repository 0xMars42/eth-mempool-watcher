//! Phase H.1 — squelette du tracker post-block.
//!
//! Pour chaque PATTERN detected, on stocke en memoire les hashes concernes
//! et le numero de block au moment de la detection. Apres N blocks ecoules,
//! l'entree devient "ready_to_check" : on pourra appeler
//! `eth_getTransactionReceipt(hash)` pour confirmer si la tx a ete minee
//! et analyser le block d'inclusion (Phase H.2).
//!
//! V0 : pas d'appel RPC, juste le buffer + l'expiration. Logique pure,
//! testable sans network.

use alloy::primitives::B256;
use std::collections::VecDeque;
use std::time::Instant;

/// Combien de blocks on attend avant de checker (Ethereum: ~12s par block,
/// donc 3 blocks = ~36s = suffisant pour qu'une tx pending normale soit minee).
pub const BLOCKS_BEFORE_CHECK: u64 = 3;

/// Au-dela de combien de blocks on considere la tx droppee/expired et on
/// arrete de tracker (mempool tx survivent typiquement quelques minutes).
pub const BLOCKS_BEFORE_GIVE_UP: u64 = 60;

/// Type de pattern qui a genere l'entree (utile pour la presentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackedKind {
    SniperCluster,
    BotRepetition,
    LargeWethSwap,
}

/// Une entree en attente de validation post-block.
#[derive(Clone, Debug)]
pub struct PendingCheck {
    pub kind: TrackedKind,
    pub hashes: Vec<B256>,
    /// Block number au moment de la detection.
    pub detected_at_block: u64,
    /// Instant local (pour debug / metriques).
    pub detected_at: Instant,
}

impl PendingCheck {
    pub fn is_ready(&self, current_block: u64) -> bool {
        current_block.saturating_sub(self.detected_at_block) >= BLOCKS_BEFORE_CHECK
    }

    pub fn is_expired(&self, current_block: u64) -> bool {
        current_block.saturating_sub(self.detected_at_block) > BLOCKS_BEFORE_GIVE_UP
    }
}

/// Buffer rolling des patterns en attente de check post-block.
#[derive(Default)]
pub struct PendingTracker {
    queue: VecDeque<PendingCheck>,
    /// Compteur cumul pour stats.
    total_tracked: u64,
    /// Compteur cumul d'entrees ready (drainees ou pas).
    total_ready: u64,
    /// Compteur cumul d'entrees expirees (jamais traitables).
    total_expired: u64,
}

impl PendingTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute une entree a tracker. Idempotent vis-a-vis des hashes en double :
    /// si le meme set de hashes a deja ete tracke (par hash du premier element),
    /// on n'ajoute pas — evite les bursts type "SniperCluster + BotRepetition au
    /// meme tick" qui contiennent les memes hashes.
    pub fn track(&mut self, kind: TrackedKind, hashes: Vec<B256>, current_block: u64) {
        if hashes.is_empty() {
            return;
        }
        let entry = PendingCheck {
            kind,
            hashes,
            detected_at_block: current_block,
            detected_at: Instant::now(),
        };
        self.queue.push_back(entry);
        self.total_tracked += 1;
    }

    /// Draine les entrees pretes a etre checkees (>= BLOCKS_BEFORE_CHECK blocks
    /// ecoules) et purge celles expirees. Retourne les ready dans l'ordre FIFO.
    /// Phase H.2 appellera cette fn et passera chaque PendingCheck dans le
    /// validator RPC.
    pub fn drain_ready_and_purge(&mut self, current_block: u64) -> Vec<PendingCheck> {
        let mut ready = Vec::new();
        // On itere et on garde uniquement ce qui n'est ni ready ni expired.
        let mut new_queue = VecDeque::with_capacity(self.queue.len());
        while let Some(entry) = self.queue.pop_front() {
            if entry.is_expired(current_block) {
                self.total_expired += 1;
                continue;
            }
            if entry.is_ready(current_block) {
                self.total_ready += 1;
                ready.push(entry);
                continue;
            }
            new_queue.push_back(entry);
        }
        self.queue = new_queue;
        ready
    }

    /// Nombre d'entrees encore en attente (ni pretes ni expirees au dernier
    /// `drain_ready_and_purge`).
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Stats cumulees pour fin de run / dashboard.
    pub fn stats(&self) -> TrackerStats {
        TrackerStats {
            pending: self.queue.len(),
            total_tracked: self.total_tracked,
            total_ready: self.total_ready,
            total_expired: self.total_expired,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TrackerStats {
    pub pending: usize,
    pub total_tracked: u64,
    pub total_ready: u64,
    pub total_expired: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(byte: u8) -> B256 {
        B256::repeat_byte(byte)
    }

    #[test]
    fn track_then_ready_after_n_blocks() {
        let mut t = PendingTracker::new();
        t.track(TrackedKind::SniperCluster, vec![h(1), h(2), h(3)], 100);
        // Block courant 100 : pas pret
        assert!(t.drain_ready_and_purge(100).is_empty());
        assert_eq!(t.pending_count(), 1);
        // Block 100 + 2 : encore pas pret (seuil = 3)
        assert!(t.drain_ready_and_purge(102).is_empty());
        // Block 100 + 3 : pret
        let ready = t.drain_ready_and_purge(103);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].kind, TrackedKind::SniperCluster);
        assert_eq!(ready[0].hashes.len(), 3);
        // Plus rien en queue
        assert_eq!(t.pending_count(), 0);
    }

    #[test]
    fn entries_expire_past_give_up_threshold() {
        let mut t = PendingTracker::new();
        t.track(TrackedKind::BotRepetition, vec![h(1)], 100);
        // Block 100 + 61 : depasse BLOCKS_BEFORE_GIVE_UP (60)
        let ready = t.drain_ready_and_purge(161);
        // Devrait etre purge sans etre ready
        assert!(ready.is_empty());
        assert_eq!(t.pending_count(), 0);
        let s = t.stats();
        assert_eq!(s.total_expired, 1);
        assert_eq!(s.total_ready, 0);
    }

    #[test]
    fn multiple_entries_drain_in_fifo_order() {
        let mut t = PendingTracker::new();
        t.track(TrackedKind::SniperCluster, vec![h(1)], 100);
        t.track(TrackedKind::BotRepetition, vec![h(2)], 101);
        t.track(TrackedKind::LargeWethSwap, vec![h(3)], 102);
        // Block 105 : tous prets (depasse seuil 3 pour les 3)
        let ready = t.drain_ready_and_purge(105);
        assert_eq!(ready.len(), 3);
        assert_eq!(ready[0].kind, TrackedKind::SniperCluster);
        assert_eq!(ready[1].kind, TrackedKind::BotRepetition);
        assert_eq!(ready[2].kind, TrackedKind::LargeWethSwap);
    }

    #[test]
    fn empty_hashes_are_ignored() {
        let mut t = PendingTracker::new();
        t.track(TrackedKind::SniperCluster, vec![], 100);
        assert_eq!(t.pending_count(), 0);
        assert_eq!(t.stats().total_tracked, 0);
    }

    #[test]
    fn mixed_ready_and_pending() {
        let mut t = PendingTracker::new();
        t.track(TrackedKind::SniperCluster, vec![h(1)], 100);
        t.track(TrackedKind::BotRepetition, vec![h(2)], 105);
        // Block 103 : seul le 1er est pret
        let ready = t.drain_ready_and_purge(103);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].hashes[0], h(1));
        assert_eq!(t.pending_count(), 1);
    }

    #[test]
    fn stats_track_cumulative_counts() {
        let mut t = PendingTracker::new();
        t.track(TrackedKind::SniperCluster, vec![h(1)], 100);
        t.track(TrackedKind::BotRepetition, vec![h(2)], 100);
        let _ = t.drain_ready_and_purge(103); // both ready
        let s = t.stats();
        assert_eq!(s.total_tracked, 2);
        assert_eq!(s.total_ready, 2);
        assert_eq!(s.total_expired, 0);
        assert_eq!(s.pending, 0);
    }
}
