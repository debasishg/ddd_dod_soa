
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
repo.add(OrderId(1), Money(10.0), Status::Completed, 1000);
repo.add(OrderId(2), Money(20.0), Status::Pending, 2000);
repo.add(OrderId(3), Money(30.0), Status::Completed, 3000);

let total_completed = repo.kernel().sum_by_status(Status::Completed);
assert_eq!(total_completed.0, 40.0);

// Zero-copy row mutation:
let k = repo.kernel_mut();
let idx = 1usize;
{ let mut row = k.view_mut(idx); row.set_status(Status::Completed); }
assert_eq!(k.sum_by_status(Status::Completed).0, 60.0);
```

## License

Licensed under either of
- Apache License, Version 2.0
- MIT license
at your option.
