//! Unavailable explicitly means uncertain delivery (provider.rs), unlike Busy.
//! This provider records the owned command, then loses its admission acknowledgement.
// Preserve the independent normative assertion expressions.
#![allow(clippy::manual_contains)]
use xcp_core::*;
#[derive(Default)]
struct Port {
    requests: Vec<ProviderRequest>,
    uncertain_next: bool,
}
impl ProviderPort for Port {
    fn try_submit(&mut self, r: ProviderRequest) -> Result<(), SubmitError> {
        self.requests.push(r);
        if core::mem::take(&mut self.uncertain_next) {
            Err(SubmitError::Unavailable)
        } else {
            Ok(())
        }
    }
}
static REGIONS: [Region; 1] = [Region::calibration(1, 0x1000, 2)];
fn send(s: &mut Session, p: &mut Port, b: &[u8]) -> Dispatch {
    s.handle_packet(
        &VirtualMap::new(&REGIONS).unwrap(),
        &Packet::try_from_slice(b).unwrap(),
        0,
        p,
    )
}
fn ready(s: &mut Session, p: &mut Port) {
    assert_eq!(send(s, p, &[0xff, 0]), Dispatch::Deferred);
    assert!(matches!(
        s.complete(
            ProviderCompletion::Synchronized {
                session_generation: 1,
                result: SynchronizeResult::Ready { service_epoch: 7 }
            },
            p
        ),
        Dispatch::Respond(_)
    ));
    p.requests.clear();
}
#[test]
fn uncertain_connect_admission_must_block_replacement_until_reconciled() {
    let (mut s, mut p) = (Session::new(), Port::default());
    p.uncertain_next = true;
    assert_eq!(send(&mut s, &mut p, &[0xff, 0]), Dispatch::Fenced);
    let old = p.requests[0];
    let d = send(&mut s, &mut p, &[0xff, 0]);
    assert_eq!(
        d,
        Dispatch::Fenced,
        "uncertain request {old:?} was replaced; requests={:?}",
        p.requests
    );
    assert_eq!(p.requests.len(), 1);
}
#[test]
fn delayed_ready_after_uncertain_admission_must_not_authorize_a_new_attempt() {
    let (mut s, mut p) = (Session::new(), Port::default());
    p.uncertain_next = true;
    send(&mut s, &mut p, &[0xff, 0]);
    let old = match p.requests[0] {
        ProviderRequest::Synchronize { session_generation } => session_generation,
        _ => unreachable!(),
    };
    send(&mut s, &mut p, &[0xff, 0]);
    let d = s.complete(
        ProviderCompletion::Synchronized {
            session_generation: old,
            result: SynchronizeResult::Ready { service_epoch: 7 },
        },
        &mut p,
    );
    assert!(
        !matches!(d, Dispatch::Respond(_)),
        "stale Ready reactivated {:?}: {d:?}; requests={:?}",
        s.state(),
        p.requests
    );
    assert_ne!(s.state(), SessionState::Ready);
}
#[test]
fn uncertain_apply_admission_must_retain_exact_key_and_resolve() {
    let (mut s, mut p) = (Session::new(), Port::default());
    ready(&mut s, &mut p);
    send(&mut s, &mut p, &[0xf6, 0, 0, 0, 0, 0x10, 0, 0]);
    p.uncertain_next = true;
    assert_eq!(send(&mut s, &mut p, &[0xf0, 2, 0xe8, 3]), Dispatch::Fenced);
    let op = match p.requests[0] {
        ProviderRequest::Apply(a) => a.operation,
        _ => unreachable!(),
    };
    let _ = s.service(20_000, &mut p).dispatch;
    assert!(
        p.requests
            .iter()
            .any(|r| *r == ProviderRequest::ResolveOrCancel { operation: op }),
        "uncertain Apply key disappeared: {op:?}; requests={:?}",
        p.requests
    );
    assert_eq!(send(&mut s, &mut p, &[0xff, 0]), Dispatch::Fenced);
}
#[test]
fn late_applied_after_uncertain_admission_must_be_recorded_and_released_silently() {
    let (mut s, mut p) = (Session::new(), Port::default());
    ready(&mut s, &mut p);
    send(&mut s, &mut p, &[0xf6, 0, 0, 0, 0, 0x10, 0, 0]);
    p.uncertain_next = true;
    send(&mut s, &mut p, &[0xf0, 2, 0xe8, 3]);
    let op = match p.requests[0] {
        ProviderRequest::Apply(a) => a.operation,
        _ => unreachable!(),
    };
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
    assert!(
        s.has_retirement(),
        "known late Applied was discarded instead of recorded/released: {op:?}"
    );
    assert!(p
        .requests
        .iter()
        .any(|r| *r == ProviderRequest::ReleaseOutcome { operation: op }));
}
#[test]
fn uncertain_read_and_quiesce_admission_must_not_allow_connect_to_replace_work() {
    let mut observations = Vec::new();
    for cmd in [&[0xfe][..], &[0xf5, 2][..]] {
        let (mut s, mut p) = (Session::new(), Port::default());
        ready(&mut s, &mut p);
        send(&mut s, &mut p, &[0xf6, 0, 0, 0, 0, 0x10, 0, 0]);
        p.uncertain_next = true;
        assert_eq!(send(&mut s, &mut p, cmd), Dispatch::Fenced);
        let old = p.requests[0];
        let result = send(&mut s, &mut p, &[0xff, 0]);
        observations.push((old, result, p.requests.len()));
    }
    assert!(
        observations
            .iter()
            .all(|(_, result, count)| *result == Dispatch::Fenced && *count == 1),
        "unreconciled control/read replaced: {observations:?}"
    );
}
