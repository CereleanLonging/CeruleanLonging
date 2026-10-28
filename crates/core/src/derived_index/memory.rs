//! Capacity accounting for retained encoded snapshots and index proofs. This is
//! not a process RSS limit: authoritative state, decoded builds and allocator
//! bookkeeping have separate lifetimes. Refusal never fails an event commit.
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
pub(super) const GLOBAL: usize = 128 * 1024 * 1024;
pub(super) const SNAPSHOT: usize = 16 * 1024 * 1024;
pub(super) const CHECKPOINT: usize = 32 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;
#[derive(Default)]
struct Usage {
    used: usize,
    peak: usize,
}
pub(super) struct Budget {
    limit: usize,
    usage: Mutex<Usage>,
}
impl Budget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            usage: Mutex::default(),
        })
    }
    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Lease, String> {
        let mut usage = self.usage.lock().unwrap_or_else(|p| p.into_inner());
        if bytes > self.limit.saturating_sub(usage.used) {
            return Err("derived memory capacity reached; ledger fallback active".into());
        }
        usage.used += bytes;
        usage.peak = usage.peak.max(usage.used);
        Ok(Lease {
            budget: self.clone(),
            bytes,
        })
    }
    #[cfg(test)]
    pub fn stats(&self) -> (usize, usize) {
        let u = self.usage.lock().unwrap_or_else(|p| p.into_inner());
        (u.used, u.peak)
    }
}
pub(super) struct Lease {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Lease {
    pub(super) fn resize(&mut self, bytes: usize) -> Result<(), String> {
        if bytes > self.bytes {
            self.absorb(self.budget.reserve(bytes - self.bytes)?);
        } else {
            let mut usage = self.budget.usage.lock().unwrap_or_else(|p| p.into_inner());
            usage.used -= self.bytes - bytes;
            self.bytes = bytes;
        }
        Ok(())
    }
    fn absorb(&mut self, mut other: Self) {
        assert!(Arc::ptr_eq(&self.budget, &other.budget));
        self.bytes += other.bytes;
        other.bytes = 0;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.budget
            .usage
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .used -= self.bytes;
    }
}
pub(super) struct Encoded {
    data: Vec<u8>,
    lease: Lease,
    limit: usize,
}
impl Encoded {
    pub fn new(value: &impl Serialize, budget: &Arc<Budget>, limit: usize) -> Result<Self, String> {
        let mut result = Self {
            data: vec![],
            lease: budget.reserve(0)?,
            limit,
        };
        serde_json::to_writer(&mut result, value)
            .map_err(|e| format!("derived snapshot unavailable: {e}"))?;
        Ok(result)
    }
    pub fn decode<T: DeserializeOwned>(self) -> Result<T, String> {
        serde_json::from_slice(&self.data).map_err(|e| e.to_string())
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}
impl Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let wanted = self
            .data
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("snapshot length overflow"))?;
        if wanted > self.limit {
            return Err(std::io::Error::other("derived snapshot exceeds byte limit"));
        }
        if wanted > self.data.capacity() {
            let capacity = wanted.div_ceil(CHUNK).saturating_mul(CHUNK).min(self.limit);
            let old_capacity = self.data.capacity();
            let mut extra = self
                .lease
                .budget
                .reserve(capacity - old_capacity)
                .map_err(std::io::Error::other)?;
            self.data
                .try_reserve_exact(capacity - self.data.len())
                .map_err(std::io::Error::other)?;
            if self.data.capacity() > capacity {
                extra.absorb(
                    self.lease
                        .budget
                        .reserve(self.data.capacity() - capacity)
                        .map_err(std::io::Error::other)?,
                );
            }
            self.lease.absorb(extra);
        }
        self.data.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod lease_resize_tests {
    use super::*;

    #[test]
    fn growth_refusal_keeps_original_reservation_and_shrink_drop_release_it() {
        let budget = Budget::new(100);
        let mut first = budget.reserve(40).unwrap();
        let second = budget.reserve(50).unwrap();
        assert!(first.resize(51).is_err());
        assert_eq!(budget.stats().0, 90);
        first.resize(45).unwrap();
        assert_eq!(budget.stats().0, 95);
        first.resize(5).unwrap();
        assert_eq!(budget.stats().0, 55);
        drop(first);
        assert_eq!(budget.stats().0, 50);
        drop(second);
        assert_eq!(budget.stats(), (0, 95));
    }
}
