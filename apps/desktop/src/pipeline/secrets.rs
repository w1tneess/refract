//! Passwords entered for locked PDFs, kept apart from job records (Stage 2.3.5).
//!
//! Job records are cloned into every UI update and may be logged, so they never hold a password.
//! Passwords live here as [`SecretPassword`]s (redacted `Debug`, zeroed on drop), keyed by job id.
//! Only merge-pdf members are stored between attempts, because a batch may need several files
//! unlocked one by one; single-file unlocks use the password once and drop it.
//!
//! Limits: this clears Refract's own copies. It cannot erase copies held by the UI toolkit's
//! input widget, the OS (clipboard, IME), freed allocator pages or `lopdf`'s decryption buffers.

use refract_engine::pdf::SecretPassword;
use std::collections::HashMap;

/// Passwords by job id, with insertion order so the most recent one can be found.
#[derive(Default)]
pub struct JobSecretStore {
    by_job: HashMap<i32, (u64, SecretPassword)>,
    next_seq: u64,
}

impl JobSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store `password` for `job_id`, replacing (and zeroing) any earlier one.
    pub fn insert(&mut self, job_id: i32, password: SecretPassword) {
        self.next_seq += 1;
        self.by_job.insert(job_id, (self.next_seq, password));
    }

    /// Take the passwords for `job_ids` out of the store, in the same order (None where absent).
    /// The caller runs the job without holding the store, then puts back what is still needed.
    pub fn take(&mut self, job_ids: &[i32]) -> Vec<Option<(u64, SecretPassword)>> {
        job_ids.iter().map(|id| self.by_job.remove(id)).collect()
    }

    /// Put back a password taken with [`take`](Self::take), keeping its original order.
    pub fn restore(&mut self, job_id: i32, entry: (u64, SecretPassword)) {
        self.by_job.insert(job_id, entry);
    }

    /// Keep only passwords whose job passes `keep` (still exists and still awaits a password);
    /// the rest are removed and zeroed. This is the store's single lifetime rule.
    pub fn retain_jobs(&mut self, keep: impl Fn(i32) -> bool) {
        self.by_job.retain(|id, _| keep(*id));
    }

    #[cfg(test)]
    pub fn contains(&self, job_id: i32) -> bool {
        self.by_job.contains_key(&job_id)
    }

    fn len(&self) -> usize {
        self.by_job.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_job.is_empty()
    }
}

impl std::fmt::Debug for JobSecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JobSecretStore {{ {} password(s), [REDACTED] }}",
            self.len()
        )
    }
}

/// Indexes of the passwords in `taken` (from [`JobSecretStore::take`]), newest first.
pub fn newest_first(taken: &[Option<(u64, SecretPassword)>]) -> Vec<usize> {
    let mut order: Vec<(u64, usize)> = taken
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.as_ref().map(|(seq, _)| (*seq, i)))
        .collect();
    order.sort_by(|a, b| b.cmp(a));
    order.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &str = "hunter2-very-secret";

    #[test]
    fn debug_output_never_contains_the_password() {
        let mut store = JobSecretStore::new();
        store.insert(1, SecretPassword::new(PLAIN));
        let shown = format!("{:?} {:?}", store, SecretPassword::new(PLAIN));
        assert!(!shown.contains(PLAIN), "{shown}");
        assert!(shown.contains("1 password(s)"));
    }

    #[test]
    fn take_restore_remove_and_clear() {
        let mut store = JobSecretStore::new();
        store.insert(1, SecretPassword::new("a"));
        store.insert(2, SecretPassword::new("b"));
        store.insert(3, SecretPassword::new("c"));

        let mut taken = store.take(&[2, 9, 1]);
        assert_eq!(store.len(), 1);
        assert!(taken[1].is_none());
        assert_eq!(taken[0].as_ref().unwrap().1.as_str(), "b");
        // 2 was entered after 1.
        assert_eq!(newest_first(&taken), vec![0, 2]);

        let entry = taken[0].take().unwrap();
        store.restore(2, entry);
        assert!(store.contains(2) && store.contains(3) && !store.contains(1));

        store.retain_jobs(|id| id != 3);
        assert!(store.contains(2) && !store.contains(3));
        store.retain_jobs(|_| false);
        assert!(store.is_empty());
    }

    #[test]
    fn replacing_a_password_keeps_one_entry_and_newest_order() {
        let mut store = JobSecretStore::new();
        store.insert(1, SecretPassword::new("old"));
        store.insert(2, SecretPassword::new("x"));
        store.insert(1, SecretPassword::new("new"));
        assert_eq!(store.len(), 2);
        let taken = store.take(&[1, 2]);
        assert_eq!(taken[0].as_ref().unwrap().1.as_str(), "new");
        assert_eq!(newest_first(&taken), vec![0, 1]);
    }
}
