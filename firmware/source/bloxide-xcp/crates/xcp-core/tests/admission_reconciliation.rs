//! Public Session/ProviderPort handoff tests: Unavailable records possible owned
//! admission; Busy never records ownership. No duplicated protocol model.
use xcp_core::*;

#[derive(Default)]
struct Port {
    owned: Vec<ProviderRequest>,
    attempts: Vec<ProviderRequest>,
    fail: Option<SubmitError>,
}
impl ProviderPort for Port {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        self.attempts.push(request);
        let result = self.fail.map_or(Ok(()), Err);
        if result != Err(SubmitError::Busy) {
            self.owned.push(request);
        }
        result
    }
}
fn send(s: &mut Session, p: &mut Port, b: &[u8]) -> Dispatch {
    let regions = [Region::calibration(1, 0x1000, 2)];
    s.handle_packet(
        &VirtualMap::new(&regions).unwrap(),
        &Packet::try_from_slice(b).unwrap(),
        0,
        p,
    )
}
fn ready() -> (Session, Port) {
    let (mut s, mut p) = (Session::new(), Port::default());
    reconnect(&mut s, &mut p, 1);
    send(&mut s, &mut p, &[0xf6, 0, 0, 0, 0, 0x10, 0, 0]);
    p.owned.clear();
    p.attempts.clear();
    (s, p)
}
fn reconnect(s: &mut Session, p: &mut Port, generation: u32) {
    p.fail = None;
    assert_eq!(send(s, p, &[0xff, 0]), Dispatch::Deferred);
    assert_eq!(
        p.owned.last(),
        Some(&ProviderRequest::Synchronize {
            session_generation: generation
        })
    );
    assert!(matches!(
        s.complete(
            ProviderCompletion::Synchronized {
                session_generation: generation,
                result: SynchronizeResult::Ready { service_epoch: 7 }
            },
            p
        ),
        Dispatch::Respond(_)
    ));
}
fn apply(s: &mut Session, p: &mut Port, uncertain: bool) -> ProviderOperation {
    p.fail = uncertain.then_some(SubmitError::Unavailable);
    assert_eq!(
        send(s, p, &[0xf0, 2, 0xe8, 3]),
        if uncertain {
            Dispatch::Fenced
        } else {
            Dispatch::Deferred
        }
    );
    let ProviderRequest::Apply(a) = *p.owned.last().unwrap() else {
        panic!()
    };
    a.operation
}
fn silent(d: Dispatch) {
    assert!(!matches!(d, Dispatch::Respond(_)), "{d:?}");
}

#[test]
fn every_uncertain_initial_control_waits_for_definitive_completion() {
    for kind in 0..3 {
        let (mut s, mut p) = ready();
        p.fail = Some(SubmitError::Unavailable);
        let cmd = match kind {
            0 => &[0xff, 0][..],
            1 => &[0xf5, 2][..],
            _ => &[0xfe][..],
        };
        assert_eq!(send(&mut s, &mut p, cmd), Dispatch::Fenced);
        let request = p.owned[0];
        let (uncertain, definitive) = match request {
            ProviderRequest::Synchronize { session_generation } => (
                ProviderCompletion::Synchronized {
                    session_generation,
                    result: SynchronizeResult::Uncertain,
                },
                ProviderCompletion::Synchronized {
                    session_generation,
                    result: SynchronizeResult::Ready { service_epoch: 99 },
                },
            ),
            ProviderRequest::Read(r) => (
                ProviderCompletion::Read {
                    correlation: r.correlation,
                    result: ReadResult::Uncertain,
                },
                ProviderCompletion::Read {
                    correlation: r.correlation,
                    result: ReadResult::Busy,
                },
            ),
            ProviderRequest::Quiesce { correlation } => (
                ProviderCompletion::Quiesced {
                    correlation,
                    result: QuiesceResult::Uncertain,
                },
                ProviderCompletion::Quiesced {
                    correlation,
                    result: QuiesceResult::Quiesced,
                },
            ),
            _ => panic!(),
        };
        for _ in 0..4 {
            silent(s.complete(uncertain, &mut p));
            silent(s.transport_lost(&mut p));
            assert_eq!(send(&mut s, &mut p, &[0xff, 0]), Dispatch::Fenced);
            assert_eq!(p.attempts.len(), 1);
        }
        silent(s.complete(definitive, &mut p));
        assert_eq!(s.service_epoch(), Some(7)); // old Ready did not install epoch99
        assert_eq!(s.mta(), Some(0x1000));
        // Busy after reconciliation remains nonadmission, never an old wire reply.
        p.fail = Some(SubmitError::Busy);
        assert_eq!(send(&mut s, &mut p, &[0xff, 0]), Dispatch::Fenced);
        assert_eq!(p.owned.len(), 1);
        reconnect(&mut s, &mut p, if kind == 0 { 3 } else { 2 });
        silent(s.complete(definitive, &mut p));
        assert_ne!(s.state(), SessionState::Ready);
    }
}

#[test]
fn uncertain_resolve_then_busy_preserves_key_and_late_terminal_truth() {
    for result in [
        WriteResult::Applied {
            changed: true,
            active_revision: 1,
        },
        WriteResult::Applied {
            changed: false,
            active_revision: 0,
        },
        WriteResult::Rejected {
            reason: RejectionReason::Expired,
        },
        WriteResult::Cancelled,
        WriteResult::Retired,
        WriteResult::StaleOperation,
        WriteResult::Busy,
    ] {
        let (mut s, mut p) = ready();
        let op = apply(&mut s, &mut p, true);
        assert_eq!(s.service(20_000, &mut p).dispatch, Dispatch::Fenced);
        assert_eq!(
            p.owned.last(),
            Some(&ProviderRequest::ResolveOrCancel { operation: op })
        );
        p.fail = Some(SubmitError::Busy);
        for _ in 0..4 {
            silent(s.complete(
                ProviderCompletion::Read {
                    correlation: Correlation {
                        session_generation: 99,
                        sequence: 0,
                    },
                    result: ReadResult::Busy,
                },
                &mut p,
            ));
            silent(s.service(30_000, &mut p).dispatch);
            silent(send(&mut s, &mut p, &[0xff, 0]));
        }
        assert_eq!(p.owned.len(), 2); // exactly one initial Apply, one possibly owned resolve
        silent(s.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result,
            },
            &mut p,
        ));
        let retained = matches!(
            result,
            WriteResult::Applied { .. } | WriteResult::Rejected { .. } | WriteResult::Cancelled
        );
        assert_eq!(s.has_retirement(), retained);
        if retained {
            p.fail = None;
            silent(s.service(40_000, &mut p).dispatch);
            assert_eq!(
                p.owned.last(),
                Some(&ProviderRequest::ReleaseOutcome { operation: op })
            );
            silent(s.complete(
                ProviderCompletion::Released {
                    correlation: op.correlation(),
                },
                &mut p,
            ));
        }
        assert!(!s.has_retirement());
        reconnect(&mut s, &mut p, 2);
        send(&mut s, &mut p, &[0xf6, 0, 0, 0, 0, 0x10, 0, 0]);
        let next = apply(&mut s, &mut p, false);
        assert_ne!(next, op);
        assert_eq!(next.session_generation, 2);
    }
}

#[test]
fn uncertain_release_then_busy_accepts_late_matching_release_without_reopening() {
    let (mut s, mut p) = ready();
    let op = apply(&mut s, &mut p, false);
    p.fail = Some(SubmitError::Unavailable);
    assert_eq!(
        s.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 1
                }
            },
            &mut p
        ),
        Dispatch::Fenced
    );
    assert!(s.has_retirement());
    assert!(p
        .owned
        .iter()
        .skip(1)
        .all(|r| *r == ProviderRequest::ReleaseOutcome { operation: op }));
    p.fail = Some(SubmitError::Busy);
    let owned = p.owned.len();
    for _ in 0..4 {
        silent(s.transport_lost(&mut p));
        silent(s.service(20_000, &mut p).dispatch);
        silent(s.complete(
            ProviderCompletion::Released {
                correlation: Correlation {
                    session_generation: 99,
                    sequence: 0,
                },
            },
            &mut p,
        ));
        assert!(s.has_retirement());
        silent(send(&mut s, &mut p, &[0xff, 0]));
    }
    assert_eq!(p.owned.len(), owned);
    assert_eq!(
        s.complete(
            ProviderCompletion::Released {
                correlation: op.correlation()
            },
            &mut p
        ),
        Dispatch::Ignored
    );
    assert!(!s.has_retirement());
    assert_ne!(s.state(), SessionState::Ready);
    silent(send(&mut s, &mut p, &[0xff])); // malformed CONNECT cannot revive a wire reply
    reconnect(&mut s, &mut p, 2);
}

#[test]
fn release_with_only_busy_admission_cannot_clear_retirement() {
    let (mut s, mut p) = ready();
    let op = apply(&mut s, &mut p, false);
    p.fail = Some(SubmitError::Busy);
    assert!(matches!(
        s.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result: WriteResult::Cancelled
            },
            &mut p
        ),
        Dispatch::Respond(_)
    ));
    assert_eq!(
        s.complete(
            ProviderCompletion::Released {
                correlation: op.correlation()
            },
            &mut p
        ),
        Dispatch::Fenced
    );
    assert!(s.has_retirement());
    p.fail = None;
    let _ = s.service(30_000, &mut p).dispatch;
    assert_eq!(
        s.complete(
            ProviderCompletion::Released {
                correlation: op.correlation()
            },
            &mut p
        ),
        Dispatch::Ignored
    );
    assert!(!s.has_retirement());
}

#[test]
fn initial_busy_proves_nonadmission_and_preserves_identity_for_all_variants() {
    for cmd in [
        &[0xff, 0][..],
        &[0xf5, 2][..],
        &[0xfe][..],
        &[0xf0, 2, 1, 0][..],
    ] {
        let (mut s, mut p) = ready();
        p.fail = Some(SubmitError::Busy);
        assert!(matches!(send(&mut s, &mut p, cmd), Dispatch::Respond(_)));
        assert!(p.owned.is_empty());
        let attempted = p.attempts[0];
        p.fail = None;
        assert_eq!(send(&mut s, &mut p, cmd), Dispatch::Deferred);
        assert_eq!(p.owned, [attempted]);
    }
}

#[test]
fn service_provenance_is_per_attempt_and_preserves_all_terminal_release_truth() {
    for outcome in [
        WriteResult::Applied {
            changed: true,
            active_revision: 42,
        },
        WriteResult::Applied {
            changed: false,
            active_revision: 0,
        },
        WriteResult::Rejected {
            reason: RejectionReason::Expired,
        },
        WriteResult::Rejected {
            reason: RejectionReason::UnsupportedPolicy,
        },
        WriteResult::Cancelled,
    ] {
        let (mut s, mut p) = ready();
        let op = apply(&mut s, &mut p, false);
        p.fail = Some(SubmitError::Busy);
        s.complete(
            ProviderCompletion::Write {
                correlation: op.correlation(),
                result: outcome,
            },
            &mut p,
        );
        s.transport_lost(&mut p);
        for failure in [
            Some(SubmitError::Busy),
            Some(SubmitError::Unavailable),
            Some(SubmitError::Busy),
            Some(SubmitError::Unavailable),
            None,
        ] {
            p.fail = failure;
            let count = p.attempts.len();
            let result = s.service(20_000, &mut p);
            assert_eq!(result.dispatch, Dispatch::Fenced);
            assert_eq!(result.new_fault, failure == Some(SubmitError::Unavailable));
            assert_eq!(
                &p.attempts[count..],
                &[ProviderRequest::ReleaseOutcome { operation: op }]
            );
            assert!(s.has_retirement());
            assert_eq!(
                s.mta(),
                Some(if matches!(outcome, WriteResult::Applied { .. }) {
                    0x1002
                } else {
                    0x1000
                })
            );
        }
        let count = p.attempts.len();
        p.fail = Some(SubmitError::Unavailable);
        assert!(!s.service(30_000, &mut p).new_fault); // confirmed release is not resubmitted
        assert_eq!(p.attempts.len(), count);
        assert!(s.has_retirement());
        assert_eq!(
            s.complete(
                ProviderCompletion::Released {
                    correlation: op.correlation()
                },
                &mut p
            ),
            Dispatch::Ignored
        );
        assert!(!s.has_retirement());
        assert!(!s.service(40_000, &mut p).new_fault); // durable fence, no work
    }
}

#[test]
fn service_resolve_fault_provenance_does_not_latch_or_change_deadline() {
    let (mut s, mut p) = ready();
    assert_eq!(
        s.service(0, &mut p),
        ServiceResult {
            dispatch: Dispatch::Ignored,
            new_fault: false
        }
    );
    let op = apply(&mut s, &mut p, false);
    p.fail = Some(SubmitError::Unavailable);
    assert_eq!(
        s.service(9_999, &mut p),
        ServiceResult {
            dispatch: Dispatch::Deferred,
            new_fault: false
        }
    );
    assert_eq!(p.attempts.len(), 1);
    for (i, failure) in [
        Some(SubmitError::Unavailable),
        Some(SubmitError::Busy),
        Some(SubmitError::Unavailable),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        p.fail = failure;
        let count = p.attempts.len();
        let result = s.service(10_000 + i as u64, &mut p);
        assert_eq!(result.dispatch, Dispatch::Fenced);
        assert_eq!(result.new_fault, failure == Some(SubmitError::Unavailable));
        assert_eq!(
            &p.attempts[count..],
            &[ProviderRequest::ResolveOrCancel { operation: op }]
        );
    }
    let count = p.attempts.len();
    p.fail = Some(SubmitError::Unavailable);
    assert!(!s.service(20_000, &mut p).new_fault);
    assert_eq!(p.attempts.len(), count);
    assert_eq!(s.mta(), Some(0x1000));
    assert!(!s.has_retirement());
}
