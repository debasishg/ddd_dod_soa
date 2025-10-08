//! DDD-friendly façade over a zero‑copy SoA kernel (sketch)
//!
//! This crate demonstrates how to keep a domain-friendly API (AoS-like) while storing data in a
//! cache-friendly Struct-of-Arrays (SoA). It avoids copying by exposing zero-copy views that
//! borrow into the columnar storage.
//!
//! NOTE: This is a pedagogical sketch; harden with indices, generational arenas, error types,
//! and proper concurrency primitives for production use.

use std::fmt;
use std::sync::Arc;

// ---------- Domain language (types & invariants) ----------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrderId(pub u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Completed,
    Cancelled,
}

#[derive(Copy, Clone, Debug)]
pub struct Money(pub f64);

impl Money {
    pub fn zero() -> Self {
        Money(0.0)
    }
    pub fn add(self, other: Money) -> Money {
        Money(self.0 + other.0)
    }
}

// ---------- SoA storage (kernel) ----------

#[derive(Default, Clone)]
pub struct OrderSoA {
    ids: Vec<OrderId>,
    amounts: Vec<f64>,     // Money column
    statuses: Vec<Status>, // Status column
    timestamps: Vec<u64>,  // epoch millis
}

impl fmt::Debug for OrderSoA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrderSoA")
            .field("len", &self.len())
            .finish()
    }
}

impl OrderSoA {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            ids: Vec::with_capacity(cap),
            amounts: Vec::with_capacity(cap),
            statuses: Vec::with_capacity(cap),
            timestamps: Vec::with_capacity(cap),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append a new row; returns its row index (a stable handle until removal/compaction).
    pub fn push(&mut self, id: OrderId, amount: Money, status: Status, ts: u64) -> usize {
        self.ids.push(id);
        self.amounts.push(amount.0);
        self.statuses.push(status);
        self.timestamps.push(ts);
        self.len() - 1
    }

    /// Zero-copy read-only view (no AoS materialization).
    pub fn view(&self, idx: usize) -> OrderView<'_> {
        OrderView { soa: self, idx }
    }

    /// Zero-copy mutable view (writes go back to columns).
    pub fn view_mut(&mut self, idx: usize) -> OrderMut<'_> {
        OrderMut {
            ids: &mut self.ids,
            amounts: &mut self.amounts,
            statuses: &mut self.statuses,
            timestamps: &mut self.timestamps,
            idx,
        }
    }

    /// Iterate zero-copy views.
    pub fn iter(&self) -> impl Iterator<Item = OrderView<'_>> {
        (0..self.len()).map(|i| self.view(i))
    }

    // -------- Hot-path kernels operating directly on columns (SoA) --------

    /// Sum amounts for a given status.
    pub fn sum_by_status(&self, status: Status) -> Money {
        let mut acc = 0.0;
        let n = self.len();
        // Tight loop over two columns; branch is predictable if status is common.
        for i in 0..n {
            // SAFETY: i < n for all columns; we keep columns the same length.
            if unsafe { *self.statuses.get_unchecked(i) } == status {
                acc += unsafe { *self.amounts.get_unchecked(i) };
            }
        }
        Money(acc)
    }

    /// Filter to indices where amount >= threshold and status matches.
    pub fn filter_indices(&self, min_amount: Money, status: Status) -> Vec<usize> {
        let mut out = Vec::new();
        let n = self.len();
        for i in 0..n {
            if self.amounts[i] >= min_amount.0 && self.statuses[i] == status {
                out.push(i);
            }
        }
        out
    }

    /// Compact in-place by retaining rows whose predicate returns true. Keeps columns aligned.
    pub fn retain<F: Fn(OrderView<'_>) -> bool>(&mut self, f: F) {
        let mut write = 0usize;
        for read in 0..self.len() {
            if f(self.view(read)) {
                if write != read {
                    self.ids[write] = self.ids[read];
                    self.amounts[write] = self.amounts[read];
                    self.statuses[write] = self.statuses[read];
                    self.timestamps[write] = self.timestamps[read];
                }
                write += 1;
            }
        }
        self.ids.truncate(write);
        self.amounts.truncate(write);
        self.statuses.truncate(write);
        self.timestamps.truncate(write);
    }
}

// ---------- Zero-copy row views (AoS façade without allocation) ----------

#[derive(Copy, Clone)]
pub struct OrderView<'a> {
    soa: &'a OrderSoA,
    idx: usize,
}
impl<'a> OrderView<'a> {
    #[inline]
    pub fn id(&self) -> OrderId {
        self.soa.ids[self.idx]
    }
    #[inline]
    pub fn amount(&self) -> Money {
        Money(self.soa.amounts[self.idx])
    }
    #[inline]
    pub fn status(&self) -> Status {
        self.soa.statuses[self.idx]
    }
    #[inline]
    pub fn timestamp(&self) -> u64 {
        self.soa.timestamps[self.idx]
    }
}

pub struct OrderMut<'a> {
    ids: &'a mut [OrderId],
    amounts: &'a mut [f64],
    statuses: &'a mut [Status],
    timestamps: &'a mut [u64],
    idx: usize,
}
impl<'a> OrderMut<'a> {
    #[inline]
    pub fn set_amount(&mut self, m: Money) {
        self.amounts[self.idx] = m.0;
    }
    #[inline]
    pub fn set_status(&mut self, s: Status) {
        self.statuses[self.idx] = s;
    }
    #[inline]
    pub fn set_timestamp(&mut self, t: u64) {
        self.timestamps[self.idx] = t;
    }
    #[inline]
    pub fn id(&self) -> OrderId {
        self.ids[self.idx]
    }
}

// ---------- Repository-like façade (DDD-friendly API) ----------

#[derive(Clone, Default)]
pub struct OrderStore {
    inner: Arc<OrderSoA>,
}

impl OrderStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OrderSoA::default()),
        }
    }

    /// Append via copy-on-write on the Arc (cheap shared reads, safe mutation).
    pub fn add(&mut self, id: OrderId, amount: Money, status: Status, ts: u64) -> usize {
        let owned = Arc::make_mut(&mut self.inner);
        owned.push(id, amount, status, ts)
    }

    /// Zero-copy query returning views.
    pub fn find_by_status(&self, s: Status) -> impl Iterator<Item = OrderView<'_>> {
        (0..self.inner.len())
            .map(|i| self.inner.view(i))
            .filter(move |v| v.status() == s)
    }

    /// Expose kernel for batch ops.
    pub fn kernel(&self) -> &OrderSoA {
        &self.inner
    }
    pub fn kernel_mut(&mut self) -> &mut OrderSoA {
        Arc::make_mut(&mut self.inner)
    }
}

// ---------- Sharding to reduce false sharing & improve write scalability ----------

// DIY cache padding (uses alignment to make each element start at a cache line)
#[repr(align(64))]
pub struct CachePadded<T>(pub T);

pub struct ShardedOrderStore {
    shards: Vec<CachePadded<OrderSoA>>,
}

impl ShardedOrderStore {
    pub fn with_shards(n: usize, cap_per: usize) -> Self {
        let mut shards = Vec::with_capacity(n);
        for _ in 0..n {
            shards.push(CachePadded(OrderSoA::with_capacity(cap_per)));
        }
        Self { shards }
    }

    #[inline]
    fn shard_idx(&self, id: OrderId) -> usize {
        (id.0 as usize) % self.shards.len()
    }

    pub fn add(&mut self, id: OrderId, amount: Money, status: Status, ts: u64) -> (usize, usize) {
        let si = self.shard_idx(id);
        let row = self.shards[si].0.push(id, amount, status, ts);
        (si, row)
    }

    pub fn sum_by_status(&self, status: Status) -> Money {
        self.shards
            .iter()
            .map(|s| s.0.sum_by_status(status))
            .fold(Money::zero(), |a, b| a.add(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sketch_usage() {
        let mut repo = OrderStore::new();
        let _ = repo.add(OrderId(1), Money(10.0), Status::Completed, 1000);
        let _ = repo.add(OrderId(2), Money(20.0), Status::Pending, 2000);
        let _ = repo.add(OrderId(3), Money(30.0), Status::Completed, 3000);

        // DDD-style querying via views (zero-copy):
        let total = repo
            .find_by_status(Status::Completed)
            .fold(Money::zero(), |acc, v| Money(acc.0 + v.amount().0));
        assert_eq!(total.0, 40.0);

        // Kernel access for batch ops:
        let kernel_total = repo.kernel().sum_by_status(Status::Completed);
        assert_eq!(kernel_total.0, 40.0);

        // Mutate a row via zero-copy mutable view:
        let k = repo.kernel_mut();
        let idx = 1usize; // suppose we tracked it externally
        {
            let mut row = k.view_mut(idx);
            row.set_status(Status::Completed);
        }
        assert_eq!(k.sum_by_status(Status::Completed).0, 60.0);
    }
}
