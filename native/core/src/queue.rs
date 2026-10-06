//! The three-region playback queue — a faithful port of `src/player/Queue.ts`.
//!
//! Regions, in play order:
//!
//! 1. **played** — history, last entry is the current track
//! 2. **manual** — what the user explicitly queued (`Play next` / `Add to queue`)
//! 3. **automatic** — the rest of whatever list playback started from
//!
//! Shuffle only touches the automatic region and remembers the original order
//! so it can be restored. Region moves are rejected cross-region, exactly like
//! `Queue.move()` — the queue panel's drag reorder depends on that.

use crate::model::TrackId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Played,
    Current,
    Manual,
    Automatic,
}

#[derive(Clone, Copy, Debug)]
pub struct QueueRow {
    pub id: TrackId,
    pub region: Region,
}

#[derive(Default)]
pub struct Queue {
    played: Vec<TrackId>,
    manual: Vec<TrackId>,
    automatic: Vec<TrackId>,
    /// The un-shuffled automatic region while shuffle is on.
    original_automatic: Option<Vec<TrackId>>,
    shuffled: bool,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current track.
    pub fn current(&self) -> Option<TrackId> {
        self.played.last().copied()
    }

    /// Everything before the current track (history, oldest first).
    pub fn history(&self) -> &[TrackId] {
        &self.played[..self.played.len().saturating_sub(1)]
    }

    pub fn manual(&self) -> &[TrackId] {
        &self.manual
    }

    pub fn automatic(&self) -> &[TrackId] {
        &self.automatic
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffled
    }

    /// Start a new queue from `list` at `start_index`. Everything before the
    /// start becomes history, so `prev` works immediately.
    pub fn set_list(&mut self, list: &[TrackId], start_index: usize) {
        self.played.clear();
        self.manual.clear();
        self.automatic.clear();
        self.original_automatic = None;
        self.shuffled = false;
        if list.is_empty() {
            return;
        }
        let start = start_index.min(list.len() - 1);
        self.played.extend_from_slice(&list[..=start]);
        self.automatic.extend_from_slice(&list[start + 1..]);
    }

    /// Advance. `None` when the queue is exhausted (repeat policy is the
    /// caller's — see `AppState`).
    pub fn next(&mut self) -> Option<TrackId> {
        let id = if !self.manual.is_empty() {
            self.manual.remove(0)
        } else if !self.automatic.is_empty() {
            let id = self.automatic.remove(0);
            if let Some(orig) = &mut self.original_automatic {
                orig.retain(|&t| t != id);
            }
            id
        } else {
            return None;
        };
        self.played.push(id);
        Some(id)
    }

    /// Go back one. The track being left is discarded (queue semantics: it
    /// already played once and `next` does not revisit it).
    pub fn prev(&mut self) -> Option<TrackId> {
        if self.played.len() <= 1 {
            return self.current();
        }
        self.played.pop();
        self.current()
    }

    /// Jump to an arbitrary position in the flattened view (queue panel click).
    pub fn jump_to(&mut self, index: usize) -> Option<TrackId> {
        let rows = self.rows();
        let row = rows.get(index)?;
        match row.region {
            Region::Played | Region::Current => {
                // Truncate history forward of the target, drop the old future.
                while self.played.len() > index + 1 {
                    self.played.pop();
                }
                // The manual/automatic regions stay; user jumped within history.
            }
            Region::Manual => {
                let id = self.manual.remove(index - self.played.len());
                self.played.push(id);
            }
            Region::Automatic => {
                let offset = index - self.played.len() - self.manual.len();
                let id = self.automatic.remove(offset);
                if let Some(orig) = &mut self.original_automatic {
                    orig.retain(|&t| t != id);
                }
                self.played.push(id);
            }
        }
        self.current()
    }

    pub fn add_to_queue(&mut self, id: TrackId) {
        self.manual.push(id);
    }

    pub fn play_next(&mut self, id: TrackId) {
        self.manual.insert(0, id);
    }

    /// The flattened, panel-shaped view.
    pub fn rows(&self) -> Vec<QueueRow> {
        let mut out = Vec::with_capacity(self.played.len() + self.manual.len() + self.automatic.len());
        let n = self.played.len();
        for (i, &id) in self.played.iter().enumerate() {
            out.push(QueueRow {
                id,
                region: if i + 1 == n { Region::Current } else { Region::Played },
            });
        }
        for &id in &self.manual {
            out.push(QueueRow { id, region: Region::Manual });
        }
        for &id in &self.automatic {
            out.push(QueueRow { id, region: Region::Automatic });
        }
        out
    }

    /// Flattened position of the current track, if present in the view.
    pub fn current_index(&self) -> usize {
        self.played.len().saturating_sub(1)
    }

    /// Remove the row at a flattened index. Returns the removed track.
    /// Removing the current track is refused (`None`) — Zuno ends the queue at
    /// a track instead (`end_at`) rather than deleting the playing row.
    pub fn remove_at(&mut self, index: usize) -> Option<TrackId> {
        let rows = self.rows();
        let row = rows.get(index)?;
        match row.region {
            Region::Current => None,
            Region::Played => {
                if index < self.played.len() - 1 {
                    Some(self.played.remove(index))
                } else {
                    None
                }
            }
            Region::Manual => {
                let i = index - self.played.len();
                Some(self.manual.remove(i))
            }
            Region::Automatic => {
                let i = index - self.played.len() - self.manual.len();
                let id = self.automatic.remove(i);
                if let Some(orig) = &mut self.original_automatic {
                    orig.retain(|&t| t != id);
                }
                Some(id)
            }
        }
    }

    /// "Stop after this track" — truncate the automatic region past `index`.
    pub fn end_at(&mut self, index: usize) {
        let rows = self.rows();
        if let Some(row) = rows.get(index) {
            if row.region == Region::Automatic {
                let i = index - self.played.len() - self.manual.len();
                self.automatic.truncate(i + 1);
                if let Some(orig) = &mut self.original_automatic {
                    orig.truncate(i + 1);
                }
            }
        }
    }

    /// Move within a region only — cross-region moves are rejected, matching
    /// `Queue.move()` (drag reorder in the panel is region-scoped).
    pub fn move_row(&mut self, from: usize, to: usize) -> bool {
        if from == to {
            return true; // no-op
        }
        let rows = self.rows();
        let (Some(a), Some(b)) = (rows.get(from), rows.get(to)) else {
            return false;
        };
        if a.region != b.region || matches!(a.region, Region::Current) {
            return false;
        }
        match a.region {
            Region::Played => {
                let src = from.min(self.played.len() - 2);
                let dst = to.min(self.played.len() - 2);
                let id = self.played.remove(src);
                self.played.insert(dst, id);
            }
            Region::Manual => {
                let src = from - self.played.len();
                let dst = to - self.played.len();
                let id = self.manual.remove(src);
                let dst = dst.min(self.manual.len());
                self.manual.insert(dst, id);
            }
            Region::Automatic => {
                let src = from - self.played.len() - self.manual.len();
                let dst = to - self.played.len() - self.manual.len();
                let id = self.automatic.remove(src);
                let dst = dst.min(self.automatic.len());
                self.automatic.insert(dst, id);
            }
            Region::Current => unreachable!(),
        }
        true
    }

    /// Shuffle only the automatic region, remembering the order.
    pub fn toggle_shuffle(&mut self) {
        if self.shuffled {
            if let Some(orig) = self.original_automatic.take() {
                self.automatic = orig;
            }
            self.shuffled = false;
        } else {
            self.original_automatic = Some(self.automatic.clone());
            let mut r = crate::rng::Rng::new(0xC0FFEE);
            r.shuffle(&mut self.automatic);
            self.shuffled = true;
        }
    }

    pub fn clear(&mut self) {
        self.played.clear();
        self.manual.clear();
        self.automatic.clear();
        self.original_automatic = None;
        self.shuffled = false;
    }

    pub fn total_remaining(&self) -> usize {
        self.manual.len() + self.automatic.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(n: u32) -> Vec<TrackId> {
        (0..n).collect()
    }

    #[test]
    fn set_list_partitions_regions() {
        let mut q = Queue::new();
        q.set_list(&l(10), 3);
        assert_eq!(q.current(), Some(3));
        assert_eq!(q.history(), &[0, 1, 2]);
        assert_eq!(q.automatic(), &[4, 5, 6, 7, 8, 9]);
        assert!(q.manual().is_empty());
    }

    #[test]
    fn manual_region_outranks_automatic() {
        let mut q = Queue::new();
        q.set_list(&l(10), 0);
        q.add_to_queue(77);
        q.play_next(88);
        assert_eq!(q.next(), Some(88));
        assert_eq!(q.next(), Some(77));
        assert_eq!(q.next(), Some(1));
    }

    #[test]
    fn prev_walks_history() {
        let mut q = Queue::new();
        q.set_list(&l(5), 2);
        assert_eq!(q.prev(), Some(1));
        assert_eq!(q.prev(), Some(0));
        // At the very start, prev stays put.
        assert_eq!(q.prev(), Some(0));
    }

    #[test]
    fn shuffle_touches_only_automatic_and_restores() {
        let mut q = Queue::new();
        q.set_list(&l(40), 0);
        q.toggle_shuffle();
        assert!(q.is_shuffled());
        let before: Vec<_> = q.automatic().to_vec();
        q.toggle_shuffle();
        assert_eq!(q.automatic(), &(0u32..40).collect::<Vec<_>>()[1..]);
        assert_eq!(q.automatic().len(), before.len());
    }

    #[test]
    fn cross_region_moves_rejected() {
        let mut q = Queue::new();
        q.set_list(&l(10), 2);
        q.add_to_queue(99);
        // row 3 is manual (0,1 are played, 2 is current, 3 is manual 99)
        assert!(!q.move_row(3, 9)); // manual -> automatic: refused
        assert!(q.move_row(3, 3)); // no-op same index ok
    }

    #[test]
    fn remove_refuses_current_row() {
        let mut q = Queue::new();
        q.set_list(&l(10), 5);
        assert_eq!(q.remove_at(5), None); // current
        // Removing a history row shifts the flattened indices: rows are now
        // 0-4 history+current (ids 1..5), 5-8 automatic (ids 6..9).
        assert_eq!(q.remove_at(0), Some(0)); // history
        assert_eq!(q.remove_at(8), Some(9)); // automatic end
    }

    #[test]
    fn end_at_truncates_upcoming() {
        let mut q = Queue::new();
        q.set_list(&l(20), 0);
        // rows: 0 current, 1..19 automatic
        q.end_at(5);
        assert_eq!(q.automatic(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn jump_from_queue_panel() {
        let mut q = Queue::new();
        q.set_list(&l(20), 2);
        // rows: 0-1 history, 2 current, 3..19 automatic (17 items).
        let rows = q.rows();
        assert_eq!(rows.len(), 20);
        assert_eq!(q.jump_to(12), Some(12));
        assert_eq!(q.current(), Some(12));
        assert_eq!(q.automatic().len(), 16);
    }
}
