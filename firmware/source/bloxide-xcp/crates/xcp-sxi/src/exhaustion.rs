use super::*;
use xcp_core::{ProviderRequest, Region, SubmitError};
use xcp_messages::{SynchronizeResult, WriteResult};

#[derive(Default)]
struct Port {
    last: Option<ProviderRequest>,
}
impl ProviderPort for Port {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        self.last = Some(request);
        Ok(())
    }
}
fn response<'id>(a: &mut Adapter<'id>, p: &mut Port) -> AdapterEvent {
    a.handle_dispatch(
        Dispatch::Respond(Packet::try_from_slice(&[0xff]).unwrap()),
        p,
    )
}

#[test]
fn final_send_is_valid_then_exhaustion_is_terminal() {
    with_adapter(|mut a| {
        let mut p = Port::default();
        a.next_send = u64::MAX - 1;
        assert_eq!(response(&mut a, &mut p), AdapterEvent::ResponseQueued);
        let last = a.take_tx().unwrap();
        assert!(a.lease_is_current(&last));
        assert!(a.complete_tx(last));
        assert_eq!(
            response(&mut a, &mut p),
            AdapterEvent::Fenced(DiscardReason::AdmissionFull)
        );
        assert_eq!(a.next_send, u64::MAX);
        assert!(a.exhausted);
        assert!(a.take_tx().is_none());
        a.lifecycle_fence(0, &mut p);
        a.observe_idle(20_000, &mut p);
        for b in [2, 0, 0xff, 0, 1] {
            a.feed_byte(20_001, b, &mut p);
        }
        a.service(&VirtualMap::new(&[]).unwrap(), 20_002, &mut p);
        assert!(p.last.is_none());
        assert!(!a.has_rx());
        assert!(!a.has_tx());
    });
}

#[test]
fn epoch_exhaustion_invalidates_active_send_without_wrap() {
    with_adapter(|mut a| {
        let mut p = Port::default();
        response(&mut a, &mut p);
        let old = a.take_tx().unwrap();
        a.epoch = u64::MAX;
        a.lifecycle_fence(0, &mut p);
        assert!(a.exhausted);
        assert_eq!(a.epoch(), u64::MAX);
        assert!(!a.lease_is_current(&old));
        assert!(!a.complete_tx(old));
        a.lifecycle_fence(1, &mut p);
        assert_eq!(a.epoch(), u64::MAX);
    });
}

#[test]
fn send_exhaustion_preserves_applied_retirement_and_control() {
    with_adapter(|mut a| {
        let mut p = Port::default();
        let regions = [Region::calibration(2, 0x1000, 2)];
        let map = VirtualMap::new(&regions).unwrap();
        let packet = |bytes: &[u8]| Packet::try_from_slice(bytes).unwrap();
        a.session
            .handle_packet(&map, &packet(&[0xff, 0]), 0, &mut p);
        let generation = match p.last.take().unwrap() {
            ProviderRequest::Synchronize { session_generation } => session_generation,
            _ => panic!(),
        };
        a.complete(
            ProviderCompletion::Synchronized {
                session_generation: generation,
                result: SynchronizeResult::Ready { service_epoch: 7 },
            },
            &mut p,
        );
        let first = a.take_tx().unwrap();
        a.complete_tx(first);
        a.session
            .handle_packet(&map, &packet(&[0xf6, 0, 0, 0, 0, 0x10, 0, 0]), 1, &mut p);
        a.session
            .handle_packet(&map, &packet(&[0xf0, 2, 1, 0]), 2, &mut p);
        let op = match p.last.take().unwrap() {
            ProviderRequest::Apply(r) => r.operation,
            _ => panic!(),
        };
        a.next_send = u64::MAX;
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
        assert!(a.exhausted);
        assert!(a.session().has_retirement());
        assert_eq!(
            p.last,
            Some(ProviderRequest::ReleaseOutcome { operation: op })
        );
        a.lifecycle_fence(3, &mut p);
        a.service(&map, 4, &mut p);
        assert!(a.session().has_retirement());
        a.complete(
            ProviderCompletion::Released {
                correlation: op.correlation(),
            },
            &mut p,
        );
        assert!(!a.session().has_retirement());
        assert!(a.take_tx().is_none());
    });
}
