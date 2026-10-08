A resource is initialized once inside `with_adapter`, then moved into its owner.
The scope can contain a persistent event loop; reset fences that same value.

```rust
xcp_sxi::with_adapter(|adapter| {
    let owned = adapter;
    assert!(!owned.has_tx());
});
```

Consumed completion authority cannot be reused, even during a later send:

```compile_fail,E0382
fn duplicate<'id>(a: &mut xcp_sxi::Adapter<'id>, old: xcp_sxi::TxLease<'id>) {
    a.complete_tx(old);
    a.complete_tx(old);
}
```

A lease is neither Copy nor Clone:

```compile_fail,E0277
fn copy<T: Copy>() {}
copy::<xcp_sxi::TxLease<'static>>();
```

```compile_fail,E0599
fn duplicate(lease: xcp_sxi::TxLease<'_>) { let _ = lease.clone(); }
```

An adapter cannot duplicate its lifetime's ownership authority:

```compile_fail,E0277
fn copy<T: Copy>() {}
copy::<xcp_sxi::Adapter<'static>>();
```

```compile_fail,E0599
xcp_sxi::with_adapter(|a| { let _ = a.clone(); });
```

Private fields prevent forging a token even if the current counter is known:

```compile_fail,E0451
fn forge<'id>(frame: xcp_sxi::Frame) -> xcp_sxi::TxLease<'id> {
    xcp_sxi::TxLease { send: 1, frame, brand: core::marker::PhantomData }
}
```

Fresh nested instances have distinct invariant brands, even when both use
send1. A real lease cannot pass another instance's current-send test or completion:

```compile_fail,E0521
xcp_sxi::with_adapter(|mut a| {
    xcp_sxi::with_adapter(|b| {
        if let Some(lease) = a.take_tx() { b.lease_is_current(&lease); }
    });
});
```

```compile_fail,E0521
xcp_sxi::with_adapter(|mut a| {
    xcp_sxi::with_adapter(|mut b| {
        if let Some(lease) = a.take_tx() { b.complete_tx(lease); }
    });
});
```

Nor can a prior instance escape its scope to collide with a later instance.
There is no finite instance namespace to wrap/exhaust:

```compile_fail
let previous = xcp_sxi::with_adapter(|mut a| a.take_tx());
xcp_sxi::with_adapter(|mut b| { if let Some(lease) = previous { b.complete_tx(lease); } });
```

Reconstruction of an existing brand is not an available lifecycle operation:

```compile_fail,E0277
xcp_sxi::with_adapter(|mut a| { a = Default::default(); });
```

The adapter itself cannot escape to be reconstructed or replaced in a later scope:

```compile_fail
let _escaped = xcp_sxi::with_adapter(|a| a);
```
