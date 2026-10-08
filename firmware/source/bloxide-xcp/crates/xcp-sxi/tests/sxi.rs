use xcp_core::{
    command, ProviderOperation, ProviderPort, ProviderRequest, Region, SessionState, SubmitError,
    VirtualMap,
};
use xcp_messages::{
    Packet, ProviderCompletion, QuiesceResult, ReadData, ReadResult, SynchronizeResult, WriteResult,
};
use xcp_sxi::{
    Adapter, AdapterEvent, CounterObservation, DiscardReason, Encoder, ParseEvent, Parser,
    INTER_BYTE_TIMEOUT_US, MAX_FRAME,
};

#[derive(Default)]
struct Port {
    pending: Option<ProviderRequest>,
    submissions: usize,
}

impl ProviderPort for Port {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        if self.pending.is_some() {
            Err(SubmitError::Busy)
        } else {
            self.pending = Some(request);
            self.submissions += 1;
            Ok(())
        }
    }
}

impl Port {
    fn take(&mut self) -> ProviderRequest {
        self.pending.take().expect("provider request")
    }
}

fn feed_parser(parser: &mut Parser, start_us: u64, bytes: &[u8]) -> Vec<ParseEvent> {
    bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| parser.feed_byte(start_us + index as u64, *byte))
        .collect()
}

fn feed_adapter(
    adapter: &mut Adapter,
    provider: &mut Port,
    start_us: u64,
    bytes: &[u8],
) -> AdapterEvent {
    let mut event = AdapterEvent::Pending;
    for (index, byte) in bytes.iter().enumerate() {
        event = adapter.feed_byte(start_us + index as u64, *byte, provider);
    }
    event
}

fn map() -> VirtualMap<'static> {
    static REGIONS: [Region; 2] = [
        Region::read_only(1, 0, 32),
        Region::calibration(2, 0x1000, 2),
    ];
    VirtualMap::new(&REGIONS).unwrap()
}

fn connect(adapter: &mut Adapter, provider: &mut Port, now_us: u64) {
    assert_eq!(
        feed_adapter(adapter, provider, now_us, &[0x02, 0x00, 0xff, 0x00, 0x01]),
        AdapterEvent::FrameQueued
    );
    assert_eq!(
        adapter.service(&map(), now_us + 5, provider),
        AdapterEvent::ControlProgress
    );
    let generation = match provider.take() {
        ProviderRequest::Synchronize { session_generation } => session_generation,
        other => panic!("unexpected request {other:?}"),
    };
    assert_eq!(
        adapter.complete(
            ProviderCompletion::Synchronized {
                session_generation: generation,
                result: SynchronizeResult::Ready { service_epoch: 7 },
            },
            provider,
        ),
        AdapterEvent::ResponseQueued
    );
    let response = adapter.take_tx().unwrap();
    assert_eq!(
        response.frame().as_slice(),
        &[0x08, 0x00, 0xff, 0x01, 0x00, 0x08, 0x08, 0x00, 0x01, 0x01, 0x1a]
    );
    assert!(adapter.complete_tx(response));
}

#[test]
fn literal_normative_vectors_match_encoder() {
    let connect = Packet::try_from_slice(&[0xff, 0x00]).unwrap();
    assert_eq!(
        xcp_sxi::Frame::try_from_parts(0, &connect).as_slice(),
        &[0x02, 0x00, 0xff, 0x00, 0x01]
    );
    assert_eq!(
        xcp_sxi::Frame::try_from_parts(0xaa, &connect).as_slice(),
        &[0x02, 0xaa, 0xff, 0x00, 0xab]
    );
    let response = Packet::try_from_slice(&[0xff, 1, 0, 8, 8, 0, 1, 1]).unwrap();
    assert_eq!(
        xcp_sxi::Frame::try_from_parts(0, &response).as_slice(),
        &[0x08, 0x00, 0xff, 0x01, 0x00, 0x08, 0x08, 0x00, 0x01, 0x01, 0x1a]
    );
}

#[test]
fn response_counter_is_owned_independently_from_request_counter() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        assert_eq!(
            feed_adapter(
                &mut adapter,
                &mut provider,
                0,
                &[0x02, 0xaa, 0xff, 0x00, 0xab]
            ),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 5, &mut provider),
            AdapterEvent::ControlProgress
        );
        let generation = match provider.take() {
            ProviderRequest::Synchronize { session_generation } => session_generation,
            _ => unreachable!(),
        };
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Synchronized {
                    session_generation: generation,
                    result: SynchronizeResult::Ready { service_epoch: 1 },
                },
                &mut provider,
            ),
            AdapterEvent::ResponseQueued
        );
        let response = adapter.take_tx().unwrap();
        assert_eq!(response.frame().counter(), 0);
        assert_ne!(response.frame().counter(), 0xaa);
    });
}

#[test]
fn encoder_covers_all_lengths_and_wraps_without_echo_coupling() {
    let mut encoder = Encoder::new();
    for length in 1..=8 {
        let bytes = [0xa5; 8];
        let frame = encoder.encode(&Packet::try_from_slice(&bytes[..length]).unwrap());
        assert_eq!(frame.len(), length + 3);
        assert_eq!(frame.counter(), (length - 1) as u8);
        assert!(frame.len() <= MAX_FRAME);
    }
    for _ in 8..=255 {
        let _ = encoder.encode(&Packet::try_from_slice(&[0xfd]).unwrap());
    }
    assert_eq!(encoder.next_counter(), 0);
    assert_eq!(
        encoder
            .encode(&Packet::try_from_slice(&[0xfc]).unwrap())
            .counter(),
        0
    );
}

#[test]
fn every_split_and_coalesced_frames_are_accepted() {
    let frames: [&[u8]; 3] = [
        &[0x01, 0xfe, 0xfd, 0xfc],
        &[0x02, 0xff, 0xff, 0x00, 0x00],
        &[
            0x08, 0x00, 0xff, 0x01, 0x00, 0x08, 0x08, 0x00, 0x01, 0x01, 0x1a,
        ],
    ];
    for frame in frames {
        for split in 1..frame.len() {
            let mut parser = Parser::new();
            let first = feed_parser(&mut parser, 0, &frame[..split]);
            assert!(first.iter().all(|event| *event == ParseEvent::Pending));
            let second = feed_parser(&mut parser, 100, &frame[split..]);
            assert!(matches!(second.last(), Some(ParseEvent::Frame(_))));
        }
    }

    let mut parser = Parser::new();
    let bytes = [0x02, 0x00, 0xff, 0x00, 0x01, 0x01, 0x01, 0xfd, 0xff];
    let events = feed_parser(&mut parser, 0, &bytes);
    let packets: Vec<_> = events
        .into_iter()
        .filter_map(|event| match event {
            ParseEvent::Frame(frame) => Some(frame.packet),
            _ => None,
        })
        .collect();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].as_slice(), &[0xff, 0x00]);
    assert_eq!(packets[1].as_slice(), &[0xfd]);
}

#[test]
fn counter_repeat_gap_and_wrap_are_diagnostic_only() {
    let mut parser = Parser::new();
    let cases = [
        (
            [0x01, 0xfe, 0xfd, 0xfc],
            CounterObservation::First { observed: 0xfe },
        ),
        (
            [0x01, 0xff, 0xfd, 0xfd],
            CounterObservation::InOrder { observed: 0xff },
        ),
        (
            [0x01, 0x00, 0xfd, 0xfe],
            CounterObservation::InOrder { observed: 0x00 },
        ),
        (
            [0x01, 0x00, 0xfd, 0xfe],
            CounterObservation::Repeat { observed: 0x00 },
        ),
        (
            [0x01, 0x05, 0xfd, 0x03],
            CounterObservation::Gap {
                expected: 0x01,
                observed: 0x05,
            },
        ),
    ];
    for (index, (bytes, expected)) in cases.into_iter().enumerate() {
        let events = feed_parser(&mut parser, index as u64 * 100, &bytes);
        match events.last().unwrap() {
            ParseEvent::Frame(frame) => assert_eq!(frame.counter_observation, expected),
            other => panic!("unexpected event {other:?}"),
        }
    }
}

#[test]
fn malformed_and_uart_input_require_explicit_idle() {
    for bad_len in [0, 9] {
        let mut parser = Parser::new();
        assert_eq!(
            parser.feed_byte(10, bad_len),
            ParseEvent::Discarded(DiscardReason::InvalidLength { observed: bad_len })
        );
        assert!(parser.is_discarding());
        for byte in [0x02, 0x00, 0xff, 0x00, 0x01] {
            assert_eq!(parser.feed_byte(11, byte), ParseEvent::Pending);
        }
        assert_eq!(parser.observe_idle(20_010), ParseEvent::Pending);
        assert_eq!(parser.observe_idle(20_011), ParseEvent::Recovered);
    }

    let mut checksum = Parser::new();
    let events = feed_parser(&mut checksum, 0, &[0x02, 0x00, 0xff, 0x00, 0x00]);
    assert_eq!(
        events.last(),
        Some(&ParseEvent::Discarded(DiscardReason::Checksum))
    );
    assert!(checksum.is_discarding());

    let mut uart = Parser::new();
    assert_eq!(
        uart.uart_error(42),
        ParseEvent::Discarded(DiscardReason::Uart)
    );
    assert!(uart.is_discarding());
}

#[test]
fn exact_idle_boundary_and_clock_overflow_fail_closed() {
    let mut parser = Parser::new();
    assert_eq!(parser.feed_byte(100, 8), ParseEvent::Pending);
    assert_eq!(
        parser.poll(100 + INTER_BYTE_TIMEOUT_US - 1),
        ParseEvent::Pending
    );
    assert_eq!(
        parser.poll(100 + INTER_BYTE_TIMEOUT_US),
        ParseEvent::Discarded(DiscardReason::Truncated)
    );
    assert_eq!(
        parser.observe_idle(100 + INTER_BYTE_TIMEOUT_US),
        ParseEvent::Recovered
    );

    let mut overflow = Parser::new();
    assert_eq!(overflow.feed_byte(u64::MAX, 2), ParseEvent::Pending);
    assert_eq!(
        overflow.feed_byte(0, 0),
        ParseEvent::Discarded(DiscardReason::ClockRegression)
    );
    assert_eq!(
        overflow.observe_idle(INTER_BYTE_TIMEOUT_US - 1),
        ParseEvent::Pending
    );
    assert_eq!(
        overflow.observe_idle(INTER_BYTE_TIMEOUT_US),
        ParseEvent::Recovered
    );
}

#[test]
fn malformed_frame_never_reaches_session_or_provider() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 0, &[0x00]),
            AdapterEvent::Fenced(DiscardReason::InvalidLength { observed: 0 })
        );
        assert_eq!(provider.submissions, 0);
        assert!(!adapter.has_rx());
        assert!(!adapter.has_tx());
        for byte in [0x02, 0x00, 0xff, 0x00, 0x01] {
            assert_eq!(
                adapter.feed_byte(1, byte, &mut provider),
                AdapterEvent::Pending
            );
        }
        assert_eq!(provider.submissions, 0);
    });
}

#[test]
fn adapter_partial_frame_expires_before_any_backend_admission() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 10, &[0x02, 0x55, 0xff]),
            AdapterEvent::Pending
        );
        assert_eq!(
            adapter.service(&map(), 100, &mut provider),
            AdapterEvent::Pending
        );
        assert_eq!(provider.submissions, 0);
        assert_eq!(
            adapter.poll(10 + 2 + INTER_BYTE_TIMEOUT_US, &mut provider),
            AdapterEvent::Fenced(DiscardReason::Truncated)
        );
        assert_eq!(provider.submissions, 0);
    });
}

#[test]
fn taken_response_is_invalidated_by_loss_and_cannot_complete() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        assert_eq!(
            feed_adapter(
                &mut adapter,
                &mut provider,
                0,
                &[0x02, 0x00, 0xff, 0x00, 0x01]
            ),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 5, &mut provider),
            AdapterEvent::ControlProgress
        );
        let generation = match provider.take() {
            ProviderRequest::Synchronize { session_generation } => session_generation,
            _ => unreachable!(),
        };
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Synchronized {
                    session_generation: generation,
                    result: SynchronizeResult::Ready { service_epoch: 9 },
                },
                &mut provider,
            ),
            AdapterEvent::ResponseQueued
        );
        let lease = adapter.take_tx().unwrap();
        assert!(adapter.lease_is_current(&lease));
        adapter.transport_lost(20, &mut provider);
        assert!(!adapter.lease_is_current(&lease));
        assert!(!adapter.complete_tx(lease));
        assert!(!adapter.has_tx());
        assert_eq!(adapter.session().state(), SessionState::Recovery);
    });
}

#[test]
fn retained_write_truth_survives_transport_fence() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        connect(&mut adapter, &mut provider, 0);

        let mut request_counter = 1;
        let mut host_encoder = Encoder::new();
        let _ = host_encoder.encode(&Packet::try_from_slice(&[command::CONNECT, 0]).unwrap());
        let set_mta = host_encoder
            .encode(&Packet::try_from_slice(&[command::SET_MTA, 0, 0, 0, 0, 0x10, 0, 0]).unwrap());
        assert_eq!(set_mta.counter(), request_counter);
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 100, set_mta.as_slice()),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 120, &mut provider),
            AdapterEvent::ResponseQueued
        );
        let set_reply = adapter.take_tx().unwrap();
        assert_eq!(set_reply.frame().as_slice(), &[0x01, 0x01, 0xff, 0x01]);
        assert!(adapter.complete_tx(set_reply));

        request_counter += 1;
        let download = host_encoder
            .encode(&Packet::try_from_slice(&[command::DOWNLOAD, 2, 0x34, 0x12]).unwrap());
        assert_eq!(download.counter(), request_counter);
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 200, download.as_slice()),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 220, &mut provider),
            AdapterEvent::ControlProgress
        );
        let operation = match provider.take() {
            ProviderRequest::Apply(request) => request.operation,
            other => panic!("unexpected request {other:?}"),
        };
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Write {
                    correlation: operation.correlation(),
                    result: WriteResult::Applied {
                        changed: true,
                        active_revision: 1
                    },
                },
                &mut provider,
            ),
            AdapterEvent::ResponseQueued
        );
        assert!(matches!(
            provider.pending,
            Some(ProviderRequest::ReleaseOutcome { .. })
        ));
        assert!(adapter.session().has_retirement());
        let success = adapter.take_tx().unwrap();
        adapter.transport_lost(300, &mut provider);
        assert!(!adapter.lease_is_current(&success));
        assert!(adapter.session().has_retirement());

        let released = match provider.take() {
            ProviderRequest::ReleaseOutcome { operation } => operation,
            other => panic!("unexpected request {other:?}"),
        };
        assert_eq!(released, operation);
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Released {
                    correlation: released.correlation()
                },
                &mut provider,
            ),
            AdapterEvent::Pending
        );
        assert!(!adapter.session().has_retirement());
        assert!(!adapter.has_tx());
    });
}

#[test]
fn response_backpressure_fences_old_reply_and_bounds_rx() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        connect(&mut adapter, &mut provider, 0);

        let status = [0x01, 0x01, 0xfd, 0xff];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 100, &status),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 104, &mut provider),
            AdapterEvent::ResponseQueued
        );
        assert!(adapter.has_tx());
        let synch = [0x01, 0x02, 0xfc, 0xff];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 200, &synch),
            AdapterEvent::Fenced(DiscardReason::AdmissionFull)
        );
        assert!(!adapter.has_tx());
        assert_eq!(adapter.session().state(), SessionState::Recovery);
        assert_eq!(adapter.diagnostics().rx_backpressure, 1);
    });
}

#[test]
fn extra_command_during_write_fences_before_late_success() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        connect(&mut adapter, &mut provider, 0);

        let set_mta = [0x08, 0x01, 0xf6, 0, 0, 0, 0, 0x10, 0, 0, 0x0f];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 100, &set_mta),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 120, &mut provider),
            AdapterEvent::ResponseQueued
        );
        let reply = adapter.take_tx().unwrap();
        assert!(adapter.complete_tx(reply));

        let download = [0x04, 0x02, 0xf0, 2, 1, 0, 0xf9];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 200, &download),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 220, &mut provider),
            AdapterEvent::ControlProgress
        );
        let operation = match provider.take() {
            ProviderRequest::Apply(request) => request.operation,
            _ => unreachable!(),
        };
        let extra = [0x01, 0x03, 0xfd, 0x01];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 230, &extra),
            AdapterEvent::Fenced(DiscardReason::AdmissionFull)
        );
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Write {
                    correlation: operation.correlation(),
                    result: WriteResult::Applied {
                        changed: true,
                        active_revision: 1
                    },
                },
                &mut provider,
            ),
            AdapterEvent::Fenced(DiscardReason::AdmissionFull)
        );
        assert!(!adapter.has_tx());
        assert!(adapter.session().has_retirement());
    });
}

#[test]
fn flood_cannot_starve_retained_write_reconciliation() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        connect(&mut adapter, &mut provider, 0);

        let set_mta = [0x08, 0x01, 0xf6, 0, 0, 0, 0, 0x10, 0, 0, 0x0f];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 100, &set_mta),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 120, &mut provider),
            AdapterEvent::ResponseQueued
        );
        let reply = adapter.take_tx().unwrap();
        assert!(adapter.complete_tx(reply));
        let download = [0x04, 0x02, 0xf0, 2, 1, 0, 0xf9];
        assert_eq!(
            feed_adapter(&mut adapter, &mut provider, 200, &download),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 220, &mut provider),
            AdapterEvent::ControlProgress
        );
        let operation = match provider.take() {
            ProviderRequest::Apply(request) => request.operation,
            _ => unreachable!(),
        };

        for index in 0..4096 {
            let _ = adapter.feed_byte(300 + index, 0xaa, &mut provider);
        }
        assert_eq!(provider.submissions, 3);
        let resolved = match provider.take() {
            ProviderRequest::ResolveOrCancel { operation } => operation,
            other => panic!("unexpected request {other:?}"),
        };
        assert_eq!(resolved, operation);
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Write {
                    correlation: operation.correlation(),
                    result: WriteResult::Applied {
                        changed: true,
                        active_revision: 1,
                    },
                },
                &mut provider,
            ),
            AdapterEvent::Fenced(DiscardReason::AdmissionFull)
        );
        assert!(adapter.session().has_retirement());
        assert!(!adapter.has_tx());
        assert!(matches!(
            provider.pending,
            Some(ProviderRequest::ReleaseOutcome { .. })
        ));
    });
}

#[test]
fn lifecycle_fence_invalidates_partially_staged_response() {
    xcp_sxi::with_adapter(|mut adapter| {
        let mut provider = Port::default();
        assert_eq!(
            feed_adapter(
                &mut adapter,
                &mut provider,
                0,
                &[0x02, 0x00, 0xff, 0x00, 0x01]
            ),
            AdapterEvent::FrameQueued
        );
        assert_eq!(
            adapter.service(&map(), 5, &mut provider),
            AdapterEvent::ControlProgress
        );
        let generation = match provider.take() {
            ProviderRequest::Synchronize { session_generation } => session_generation,
            _ => unreachable!(),
        };
        assert_eq!(
            adapter.complete(
                ProviderCompletion::Synchronized {
                    session_generation: generation,
                    result: SynchronizeResult::Ready { service_epoch: 9 },
                },
                &mut provider,
            ),
            AdapterEvent::ResponseQueued
        );
        let staged = adapter.take_tx().unwrap();
        let prefix = &staged.frame().as_slice()[..3];
        assert_eq!(prefix, &[0x08, 0x00, 0xff]);
        adapter.lifecycle_fence(50, &mut provider);
        assert!(!adapter.lease_is_current(&staged));
        assert!(!adapter.complete_tx(staged));
        assert_eq!(adapter.session().state(), SessionState::Recovery);
    });
}

#[test]
fn fixed_types_stay_small_and_have_no_drop_work() {
    assert_eq!(core::mem::size_of::<xcp_sxi::Frame>(), 12);
    assert!(core::mem::size_of::<Parser>() <= 40);
    assert!(core::mem::size_of::<Adapter>() <= 384);
    assert!(!core::mem::needs_drop::<xcp_sxi::Frame>());
    assert!(!core::mem::needs_drop::<Parser>());
    assert!(!core::mem::needs_drop::<Adapter>());
}

// Keep all completion variants linked at this consumer boundary. These calls
// also assert that xcp-sxi introduces no transport-specific completion type.
#[test]
fn completion_types_remain_transport_neutral() {
    let _ = ProviderCompletion::Read {
        correlation: xcp_messages::Correlation {
            session_generation: 1,
            sequence: 2,
        },
        result: ReadResult::Data(ReadData::new([0; 7], 1).unwrap()),
    };
    let _ = ProviderCompletion::Quiesced {
        correlation: xcp_messages::Correlation {
            session_generation: 1,
            sequence: 3,
        },
        result: QuiesceResult::Quiesced,
    };
    let _ = ProviderOperation {
        service_epoch: 1,
        session_generation: 1,
        sequence: 4,
    };
}
