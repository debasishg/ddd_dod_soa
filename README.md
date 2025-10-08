
# ddd_dod_soa

A small library that demonstrates how to reconcile Domain-Driven Design (DDD) with
data-oriented design (DoD) using a **Struct-of-Arrays (SoA)** kernel and **zero-copy AoS-like views**.

## Highlights

- **SoA kernel** (`OrderSoA`) stores columns contiguously for cache-friendly scans.
- **AoS facade** via `OrderView` / `OrderMut` gives intention-revealing domain-style access with **no copying**.
- **Repository** (`OrderStore`) exposes a clean DDD-like API. Internally uses `Arc` and copy-on-write.
- **Sharded store** to reduce false sharing and scale writes.
- **Kernels** operate directly on columns (e.g., `sum_by_status`, `filter_indices`).

## Example

```rust
use ddd_dod_soa::*;

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
```

## License

Licensed under either of
- Apache License, Version 2.0
- MIT license
at your option.
