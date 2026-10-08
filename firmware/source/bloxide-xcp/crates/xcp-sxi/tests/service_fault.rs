// Shared fixture helpers copied from the inspected correction; tests below independently authored.
use xcp_core::{ProviderPort, ProviderRequest, Region, SubmitError, VirtualMap};
use xcp_messages::{ProviderCompletion, SynchronizeResult, WriteResult};
use xcp_sxi::{Adapter, AdapterEvent, TxLease};
#[derive(Default)]
struct Port {
    requests: Vec<ProviderRequest>,
    release_busy: bool,
    release_unavailable: bool,
    release_attempts: usize,
    resolve_fail: Option<SubmitError>,
    attempts: Vec<ProviderRequest>,
}
impl ProviderPort for Port {
    fn try_submit(&mut self, r: ProviderRequest) -> Result<(), SubmitError> {
        self.attempts.push(r);
        if matches!(r, ProviderRequest::ResolveOrCancel { .. }) {
            if let Some(error) = self.resolve_fail {
                return Err(error);
            }
        }
        if matches!(r, ProviderRequest::ReleaseOutcome { .. }) {
            self.release_attempts += 1;
            if self.release_unavailable {
                return Err(SubmitError::Unavailable);
            }
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
fn new_service_unavailability_must_fence_pre_fault_partial_connect() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let op = write_pending(&mut a, &mut p);
        p.release_busy = true;
        a.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 42,
                },
            },
            &mut p,
        );
        let old = a.take_tx().unwrap();
        a.lifecycle_fence(400, &mut p);
        assert!(!a.lease_is_current(&old));
        assert_eq!(a.observe_idle(20_400, &mut p), AdapterEvent::Recovered);
        // Start a new CONNECT while release control remains backpressured.
        for b in [2, 0, 0xff] {
            a.feed_byte(20_401, b, &mut p);
        }
        let epoch = a.epoch();
        let attempts = p.release_attempts;
        p.release_busy = false;
        p.release_unavailable = true;
        let event = a.service(&map(), 20_402, &mut p);
        println!("new unavailable: event={event:?} epoch_before={epoch} epoch_after={} buffered={} release_attempts_before={attempts} after={} retirement={}",a.epoch(),a.parser().buffered_len(),p.release_attempts,a.session().has_retirement());
        let fault_fenced =
            a.epoch() > epoch && a.parser().is_discarding() && a.parser().buffered_len() == 0;
        // The uncertain request can have actually been delivered: genuine matching Released.
        p.release_unavailable = false;
        a.complete(
            ProviderCompletion::Released {
                correlation: op.correlation(),
            },
            &mut p,
        );
        assert!(!a.session().has_retirement());
        for b in [0, 1] {
            a.feed_byte(20_403, b, &mut p);
        }
        let dispatch = a.service(&map(), 20_403, &mut p);
        println!(
            "suffix without new idle: dispatch={dispatch:?} state={:?} requests={:?}",
            a.session().state(),
            p.requests
        );
        assert!(
            fault_fenced,
            "new provider unavailability was mistaken for an old durable service fence"
        );
        assert!(
            !p.requests
                .iter()
                .any(|r| matches!(r, ProviderRequest::Synchronize { .. })),
            "pre-fault CONNECT suffix admitted after new fault without idle"
        );
    });
}

#[test]
fn repeated_busy_is_not_a_new_fault_and_must_preserve_partial_connect() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let op = write_pending(&mut a, &mut p);
        p.release_busy = true;
        a.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 42,
                },
            },
            &mut p,
        );
        let old = a.take_tx().unwrap();
        a.lifecycle_fence(400, &mut p);
        assert_eq!(a.observe_idle(20_400, &mut p), AdapterEvent::Recovered);
        for b in [2, 0, 0xff] {
            a.feed_byte(20_401, b, &mut p);
        }
        let epoch = a.epoch();
        let attempts = p.release_attempts;
        assert_eq!(a.service(&map(), 20_402, &mut p), AdapterEvent::Pending);
        assert_eq!(p.release_attempts, attempts + 1);
        assert_eq!(a.epoch(), epoch);
        assert_eq!(a.parser().buffered_len(), 3);
        p.release_busy = false;
        a.service(&map(), 20_403, &mut p);
        assert_eq!(
            p.requests.pop(),
            Some(ProviderRequest::ReleaseOutcome { operation: op })
        );
        assert!(a.session().has_retirement());
        assert_eq!(a.parser().buffered_len(), 3);
        a.complete(
            ProviderCompletion::Released {
                correlation: op.correlation(),
            },
            &mut p,
        );
        assert!(!a.session().has_retirement());
        assert!(!a.complete_tx(old));
        for b in [0, 1] {
            a.feed_byte(20_404, b, &mut p);
        }
        assert_eq!(
            a.service(&map(), 20_404, &mut p),
            AdapterEvent::ControlProgress
        );
        assert!(matches!(
            p.requests.as_slice(),
            [ProviderRequest::Synchronize {
                session_generation: 2
            }]
        ));
        println!("Busy control retains partial boundary; accepted release plus actual Released permits CONNECT");
    });
}

#[test]
fn fresh_resolve_faults_revoke_each_recovered_boundary_without_resubmitting_apply() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let op = write_pending(&mut a, &mut p);
        p.resolve_fail = Some(SubmitError::Busy);
        a.lifecycle_fence(400, &mut p);
        for t in [20_400, 40_403] {
            assert_eq!(a.observe_idle(t, &mut p), AdapterEvent::Recovered);
            for b in [2, 0, 0xff] {
                a.feed_byte(t + 1, b, &mut p);
            }
            let epoch = a.epoch();
            let count = p.attempts.len();
            assert_eq!(a.service(&map(), t + 2, &mut p), AdapterEvent::Pending);
            assert_eq!(a.epoch(), epoch);
            assert_eq!(a.parser().buffered_len(), 3);
            p.resolve_fail = Some(SubmitError::Unavailable);
            assert!(matches!(
                a.service(&map(), t + 3, &mut p),
                AdapterEvent::Fenced(_)
            ));
            assert_eq!(a.epoch(), epoch + 1);
            assert!(a.parser().is_discarding());
            assert_eq!(a.parser().buffered_len(), 0);
            assert_eq!(
                &p.attempts[count..],
                &[ProviderRequest::ResolveOrCancel { operation: op }; 2]
            );
            p.resolve_fail = Some(SubmitError::Busy);
        }
        // A late real Applied remains truth even after two fresh admission faults.
        p.release_busy = true;
        assert!(matches!(
            a.complete(
                ProviderCompletion::Write {
                    correlation: op.correlation(),
                    result: WriteResult::Applied {
                        changed: true,
                        active_revision: 42
                    },
                },
                &mut p
            ),
            AdapterEvent::Fenced(_)
        ));
        assert_eq!(a.session().mta(), Some(0x1002));
        assert!(a.session().has_retirement());
        assert!(a.take_tx().is_none());
        p.release_busy = false;
        a.service(&map(), 40_410, &mut p);
        assert_eq!(
            p.requests.pop(),
            Some(ProviderRequest::ReleaseOutcome { operation: op })
        );
        assert!(a.session().has_retirement());
        a.complete(
            ProviderCompletion::Released {
                correlation: op.correlation(),
            },
            &mut p,
        );
        assert!(!a.session().has_retirement());
        assert_eq!(
            p.attempts
                .iter()
                .filter(|r| matches!(r, ProviderRequest::Apply(_)))
                .count(),
            1
        );
    });
}

#[test]
fn release_fault_revokes_queued_rx_and_held_tx_and_needs_real_release() {
    for held in [false, true] {
        xcp_sxi::with_adapter(|mut a| {
            let mut p = Port::default();
            let op = write_pending(&mut a, &mut p);
            p.release_busy = true;
            a.complete(
                ProviderCompletion::Write {
                    correlation: op.correlation(),
                    result: WriteResult::Applied {
                        changed: false,
                        active_revision: 0,
                    },
                },
                &mut p,
            );
            let lease = a.take_tx().unwrap();
            if !held {
                assert!(a.complete_tx(lease));
            } else {
                p.release_unavailable = true;
                let epoch = a.epoch();
                let count = p.release_attempts;
                assert!(matches!(
                    a.service(&map(), 401, &mut p),
                    AdapterEvent::Fenced(_)
                ));
                assert_eq!(a.epoch(), epoch + 1);
                assert_eq!(p.release_attempts, count + 1);
                assert!(!a.complete_tx(lease));
                assert!(a.session().has_retirement());
                return;
            }
            feed(&mut a, &mut p, 400, &[0xfd]);
            assert!(a.has_rx());
            p.release_unavailable = true;
            let epoch = a.epoch();
            let count = p.release_attempts;
            assert!(matches!(
                a.service(&map(), 401, &mut p),
                AdapterEvent::Fenced(_)
            ));
            assert_eq!(a.epoch(), epoch + 1);
            assert_eq!(p.release_attempts, count + 1);
            assert!(!a.has_rx());
            assert!(!a.has_tx());
            assert!(a.session().has_retirement());
            a.complete(
                ProviderCompletion::Released {
                    correlation: op.correlation(),
                },
                &mut p,
            );
            assert!(!a.session().has_retirement());
        });
    }
}
