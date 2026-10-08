use xcp_core::{ProviderPort, ProviderRequest, Region, SubmitError, VirtualMap};
use xcp_messages::{ProviderCompletion, SynchronizeResult, WriteResult};
use xcp_sxi::{Adapter, AdapterEvent, DiscardReason, ParseEvent, Parser, TxLease};
#[derive(Default)]
struct Port {
    requests: Vec<ProviderRequest>,
    release_busy: bool,
    release_attempts: usize,
}
impl ProviderPort for Port {
    fn try_submit(&mut self, r: ProviderRequest) -> Result<(), SubmitError> {
        if matches!(r, ProviderRequest::ReleaseOutcome { .. }) {
            self.release_attempts += 1;
            if self.release_busy {
                return Err(SubmitError::Busy);
            }
        }
        self.requests.push(r);
        Ok(())
    }
}
fn map() -> VirtualMap<'static> {
    static R: [Region; 1] = [Region::calibration(2, 0x1000, 2)];
    VirtualMap::new(&R).unwrap()
}
// Oracle uses integer SUM8 directly, not the product encoder.
fn frame(counter: u8, pdu: &[u8]) -> Vec<u8> {
    let mut f = vec![pdu.len() as u8, counter];
    f.extend_from_slice(pdu);
    f.push((f.iter().map(|&b| b as u32).sum::<u32>() % 256) as u8);
    f
}
fn feed(a: &mut Adapter, p: &mut Port, t: u64, pdu: &[u8]) {
    for b in frame(0xa5, pdu) {
        a.feed_byte(t, b, p);
    }
}
fn connect<'id>(a: &mut Adapter<'id>, p: &mut Port, t: u64) -> TxLease<'id> {
    feed(a, p, t, &[0xff, 0]);
    assert_eq!(a.service(&map(), t, p), AdapterEvent::ControlProgress);
    let g = match p.requests.pop().unwrap() {
        ProviderRequest::Synchronize { session_generation } => session_generation,
        _ => panic!(),
    };
    assert_eq!(
        a.complete(
            ProviderCompletion::Synchronized {
                session_generation: g,
                result: SynchronizeResult::Ready { service_epoch: 7 }
            },
            p
        ),
        AdapterEvent::ResponseQueued
    );
    a.take_tx().unwrap()
}
fn write_pending(a: &mut Adapter, p: &mut Port) -> xcp_core::ProviderOperation {
    let l = connect(a, p, 100);
    assert!(a.complete_tx(l));
    feed(a, p, 200, &[0xf6, 0, 0, 0, 0, 0x10, 0, 0]);
    a.service(&map(), 200, p);
    let l = a.take_tx().unwrap();
    a.complete_tx(l);
    feed(a, p, 300, &[0xf0, 2, 0xe8, 3]);
    a.service(&map(), 300, p);
    match p.requests.pop().unwrap() {
        ProviderRequest::Apply(r) => r.operation,
        _ => panic!(),
    }
}

#[test]
fn all_splits_exact_arrival_deadlines() {
    let bytes = frame(0, &[0xff, 0]);
    let mut failures = Vec::new();
    for gap in [19999, 20000, 20001] {
        for split in 1..bytes.len() {
            let mut parser = Parser::new();
            for &b in &bytes[..split] {
                assert_eq!(parser.feed_byte(100, b), ParseEvent::Pending);
            }
            let first = parser.feed_byte(100 + gap, bytes[split]);
            let mut frames = usize::from(matches!(first, ParseEvent::Frame(_)));
            for &b in &bytes[split + 1..] {
                frames += usize::from(matches!(
                    parser.feed_byte(100 + gap, b),
                    ParseEvent::Frame(_)
                ));
            }
            let correct = if gap < 20000 {
                frames == 1
            } else {
                first == ParseEvent::Discarded(DiscardReason::Truncated)
                    && frames == 0
                    && parser.is_discarding()
            };
            println!("split={split} gap={gap} first={first:?} frames={frames} correct={correct}");
            if !correct {
                failures.push((split, gap));
            }
        }
    }
    assert!(failures.is_empty(), "deadline violations: {failures:?}");
}
#[test]
fn idle_regression_blocks_queued_success() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let l = connect(&mut a, &mut p, 100);
        assert!(a.complete_tx(l));
        feed(&mut a, &mut p, 200, &[0xfd]);
        a.service(&map(), 200, &mut p);
        assert_eq!(
            a.observe_idle(199, &mut p),
            AdapterEvent::Fenced(DiscardReason::ClockRegression)
        );
        assert!(
            a.take_tx().is_none(),
            "queued success survives reported fence"
        );
    });
}
#[test]
fn idle_wrap_preserves_pending_write_and_requests_resolution() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let operation = write_pending(&mut a, &mut p);
        // Partial next input records the final representable timestamp without admitting a packet.
        a.feed_byte(u64::MAX, 8, &mut p);
        assert_eq!(
            a.observe_idle(0, &mut p),
            AdapterEvent::Fenced(DiscardReason::ClockRegression)
        );
        println!("state={:?}, requests={:?}", a.session().state(), p.requests);
        assert!(
            p.requests.iter().any(
                |r| matches!(r,ProviderRequest::ResolveOrCancel{operation:o} if *o==operation)
            ),
            "clock fault did not fence pending owner work"
        );
    });
}

#[test]
fn fenced_old_send_cannot_complete_later_send_and_resources_move_once() {
    xcp_sxi::with_adapter(|adapter| {
        let mut a = adapter; // Ownership moves without creating a second instance.
        let mut p = Port::default();
        let old = connect(&mut a, &mut p, 100);
        assert!(a.lease_is_current(&old));
        a.lifecycle_fence(200, &mut p);
        assert!(!a.lease_is_current(&old));
        assert_eq!(a.observe_idle(20_200, &mut p), AdapterEvent::Recovered);
        let new = connect(&mut a, &mut p, 20_201);
        assert_ne!(old.frame().counter(), new.frame().counter());
        assert!(!a.lease_is_current(&old));
        assert!(a.lease_is_current(&new));
        assert!(!a.complete_tx(old));
        assert!(a.has_tx());
        assert!(a.lease_is_current(&new));
        assert!(a.complete_tx(new));
        assert!(!a.has_tx());
    });
}

#[test]
fn repeated_durable_fence_service_preserves_run2_partial_boundary() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        a.lifecycle_fence(0, &mut p);
        assert_eq!(a.observe_idle(20_000, &mut p), AdapterEvent::Recovered);
        for b in [2, 0x55, 0xff] {
            a.feed_byte(20_001, b, &mut p);
        }
        let epoch = a.epoch();
        for t in 20_002..20_102 {
            a.service(&map(), t, &mut p);
        }
        assert_eq!(a.epoch(), epoch);
        assert_eq!(a.parser().buffered_len(), 3);
        assert!(p.requests.is_empty());
        assert_eq!(a.poll(40_000, &mut p), AdapterEvent::Pending);
        assert_eq!(
            a.poll(40_001, &mut p),
            AdapterEvent::Fenced(DiscardReason::Truncated)
        );
    });
}

#[test]
fn idle_fault_then_reset_preserves_applied_key_until_actual_release() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let op = write_pending(&mut a, &mut p);
        a.feed_byte(u64::MAX, 8, &mut p);
        assert_eq!(
            a.observe_idle(0, &mut p),
            AdapterEvent::Fenced(DiscardReason::ClockRegression)
        );
        assert_eq!(
            p.requests.as_slice(),
            &[ProviderRequest::ResolveOrCancel { operation: op }]
        );
        a.lifecycle_fence(1, &mut p);
        p.release_busy = true;
        a.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 1,
                },
            },
            &mut p,
        );
        assert!(a.session().has_retirement());
        assert!(a.take_tx().is_none());
        a.lifecycle_fence(2, &mut p);
        assert!(a.session().has_retirement());
        p.release_busy = false;
        a.service(&map(), 3, &mut p);
        assert_eq!(
            p.requests.last(),
            Some(&ProviderRequest::ReleaseOutcome { operation: op })
        );
        assert!(a.session().has_retirement()); // Admission is not Released.
        a.complete(
            ProviderCompletion::Released {
                correlation: op.correlation(),
            },
            &mut p,
        );
        assert!(!a.session().has_retirement());
        assert!(a.take_tx().is_none());
        assert_eq!(a.observe_idle(20_002, &mut p), AdapterEvent::Recovered);
        let current = connect(&mut a, &mut p, 20_003);
        assert!(a.lease_is_current(&current));
        assert!(a.complete_tx(current));
        assert_eq!(a.session().session_generation(), 2);
    });
}

#[test]
fn every_late_split_has_zero_backend_admission_and_requires_idle() {
    let bytes = frame(0xaa, &[0xff, 0]);
    for split in 1..bytes.len() {
        for gap in [19_999, 20_000, 20_001] {
            xcp_sxi::with_adapter(|mut a| {
                let mut p = Port::default();
                for &b in &bytes[..split] {
                    a.feed_byte(100, b, &mut p);
                }
                let first = a.feed_byte(100 + gap, bytes[split], &mut p);
                for &b in &bytes[split + 1..] {
                    a.feed_byte(100 + gap, b, &mut p);
                }
                a.service(&map(), 100 + gap, &mut p);
                if gap < 20_000 {
                    assert!(matches!(
                        p.requests.as_slice(),
                        [ProviderRequest::Synchronize { .. }]
                    ));
                } else {
                    assert_eq!(first, AdapterEvent::Fenced(DiscardReason::Truncated));
                    assert!(p.requests.is_empty());
                    feed(&mut a, &mut p, 101 + gap, &[0xff, 0]);
                    a.service(&map(), 101 + gap, &mut p);
                    assert!(p.requests.is_empty()); // No scanning/recovery on input.
                    assert_eq!(a.observe_idle(20_100 + gap, &mut p), AdapterEvent::Pending);
                    assert_eq!(
                        a.observe_idle(20_101 + gap, &mut p),
                        AdapterEvent::Recovered
                    );
                    let lease = connect(&mut a, &mut p, 20_102 + gap);
                    assert!(a.complete_tx(lease));
                }
            });
        }
    }
}
