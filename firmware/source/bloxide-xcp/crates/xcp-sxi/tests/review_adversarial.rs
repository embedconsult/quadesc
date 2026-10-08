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
fn arrival_at_deadline_must_discard_without_a_preceding_poll() {
    for gap in [20_000, 20_001, 1_000_000] {
        let mut p = Parser::new();
        assert_eq!(p.feed_byte(100, 2), ParseEvent::Pending);
        let result = p.feed_byte(100 + gap, 0);
        println!(
            "gap={gap}, arrival={result:?}, buffered={}",
            p.buffered_len()
        );
        assert_eq!(
            result,
            ParseEvent::Discarded(DiscardReason::Truncated),
            "expired partial accepted"
        );
    }
}
#[test]
fn idle_clock_regression_must_invalidate_taken_response() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let l = connect(&mut a, &mut p, 100);
        let epoch = a.epoch();
        let event = a.observe_idle(99, &mut p);
        println!(
            "event={event:?}, epoch_before={epoch}, epoch_after={}, state={:?}, lease_current={}",
            a.epoch(),
            a.session().state(),
            a.lease_is_current(&l)
        );
        assert_eq!(event, AdapterEvent::Fenced(DiscardReason::ClockRegression));
        assert!(
            !a.lease_is_current(&l),
            "reported fence left obsolete success sendable"
        );
    });
}
#[test]
fn ready_packet_flood_must_not_starve_retirement_retry() {
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
        let l = a.take_tx().unwrap();
        a.complete_tx(l);
        p.release_busy = false;
        for i in 0..100 {
            feed(&mut a, &mut p, 400 + i, &[0xfd]);
            a.service(&map(), 400 + i, &mut p);
            let l = a.take_tx().unwrap();
            a.complete_tx(l);
        }
        println!(
            "after 100 commands: release_attempts={}, retirement={}",
            p.release_attempts,
            a.session().has_retirement()
        );
        assert!(
            p.release_attempts > 1,
            "fair service never retried now-available control port"
        );
        assert_eq!(p.release_attempts, 2);
        assert_eq!(
            p.requests.as_slice(),
            &[ProviderRequest::ReleaseOutcome { operation: op }]
        );
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
#[test]
fn tx_backpressure_must_not_starve_retirement_retry() {
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
        p.release_busy = false;
        let l = a.take_tx().unwrap();
        for i in 0..100 {
            a.service(&map(), 400 + i, &mut p);
        }
        println!(
            "blocked TX: release_attempts={}, retirement={}",
            p.release_attempts,
            a.session().has_retirement()
        );
        assert!(p.release_attempts > 1);
        assert_eq!(p.release_attempts, 2);
        assert_eq!(
            p.requests.as_slice(),
            &[ProviderRequest::ReleaseOutcome { operation: op }]
        );
        assert!(a.session().has_retirement());
        assert!(a.lease_is_current(&l));
        a.complete(
            ProviderCompletion::Released {
                correlation: op.correlation(),
            },
            &mut p,
        );
        assert!(!a.session().has_retirement());
        assert!(a.lease_is_current(&l));
        assert!(a.complete_tx(l));
    });
}
#[test]
fn partial_frame_must_not_delay_owner_deadline() {
    xcp_sxi::with_adapter(|mut a| {
        let mut p = Port::default();
        let op = write_pending(&mut a, &mut p);
        a.feed_byte(301, 8, &mut p); // Next partial frame need not reach packet admission.
        a.service(&map(), 10_299, &mut p);
        assert!(p.requests.is_empty());
        let event = a.service(&map(), 10_300, &mut p);
        println!(
            "at owner deadline: event={event:?}, state={:?}, requests={:?}",
            a.session().state(),
            p.requests
        );
        assert!(p
            .requests
            .iter()
            .any(|r| matches!(r, ProviderRequest::ResolveOrCancel { .. })));
        assert_eq!(
            p.requests.as_slice(),
            &[ProviderRequest::ResolveOrCancel { operation: op }]
        );
        assert_eq!(a.parser().buffered_len(), 1);
        assert!(!a.has_tx()); // No fabricated rejection or cancellation.
    });
}
#[test]
fn independent_sum8_lengths_counters_splits_and_corruption() {
    for len in 1..=8 {
        for ctr in [0, 1, 0xaa, 0xfe, 0xff] {
            let pdu: Vec<_> = (0..len)
                .map(|i| [0, 0xff, 0x55, 0xaa, 0x7d, 0x7e, 0x80, 1][i])
                .collect();
            let bytes = frame(ctr, &pdu);
            for split in 0..=bytes.len() {
                let mut parser = Parser::new();
                let mut count = 0;
                for (i, &b) in bytes.iter().enumerate() {
                    let e = parser.feed_byte(if i < split { 100 } else { 20_099 }, b);
                    if let ParseEvent::Frame(f) = e {
                        assert_eq!(f.packet.as_slice(), pdu);
                        assert_eq!(f.counter, ctr);
                        count += 1;
                    }
                }
                assert_eq!(count, 1);
            }
            for pos in 1..bytes.len() {
                let mut bad = bytes.clone();
                bad[pos] ^= 1;
                let mut parser = Parser::new();
                for &b in &bad {
                    assert!(!matches!(parser.feed_byte(100, b), ParseEvent::Frame(_)));
                }
                assert!(parser.is_discarding());
                for &b in &bytes {
                    assert_eq!(parser.feed_byte(200, b), ParseEvent::Pending);
                }
                assert_eq!(parser.observe_idle(20_199), ParseEvent::Pending);
                assert_eq!(parser.observe_idle(20_200), ParseEvent::Recovered);
            }
        }
    }
}
