use xcp_core::{
    command, Dispatch, ErrorCode, Packet, ProviderCompletion, ProviderOperation, ProviderPort,
    ProviderRequest, QuiesceResult, ReadData, ReadResult, Region, RejectionReason, Session,
    SessionState, SubmitError, SynchronizeResult, VirtualMap, WriteResult, APPLY_DEADLINE_US,
};

const REGIONS: [Region; 5] = [
    Region::read_only(1, 0x0000, 32),
    Region::read_only(2, 0x0040, 4),
    Region::calibration(0x1001, 0x1000, 2),
    Region::calibration(0x1002, 0x1002, 2),
    Region::read_only(0x2001, 0x2000, 4),
];

#[derive(Default)]
struct Port {
    requests: Vec<ProviderRequest>,
    fail_next: Option<SubmitError>,
}

impl Port {
    fn take(&mut self) -> ProviderRequest {
        self.requests.remove(0)
    }
}

impl ProviderPort for Port {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        if let Some(error) = self.fail_next.take() {
            Err(error)
        } else {
            self.requests.push(request);
            Ok(())
        }
    }
}

fn pdu(bytes: &[u8]) -> Packet {
    Packet::try_from_slice(bytes).unwrap()
}

fn response(dispatch: Dispatch) -> Vec<u8> {
    match dispatch {
        Dispatch::Respond(packet) => packet.as_slice().to_vec(),
        other => panic!("expected response, got {other:?}"),
    }
}

fn connect(session: &mut Session, port: &mut Port) -> Vec<u8> {
    let map = VirtualMap::new(&REGIONS).unwrap();
    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::CONNECT, 0]), 0, port),
        Dispatch::Deferred
    );
    let generation = match port.take() {
        ProviderRequest::Synchronize { session_generation } => session_generation,
        other => panic!("unexpected request: {other:?}"),
    };
    response(session.complete(
        ProviderCompletion::Synchronized {
            session_generation: generation,
            result: SynchronizeResult::Ready { service_epoch: 7 },
        },
        port,
    ))
}

fn set_mta(session: &mut Session, port: &mut Port, address: u32) -> Vec<u8> {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let a = address.to_le_bytes();
    response(session.handle_packet(
        &map,
        &pdu(&[command::SET_MTA, 0, 0, 0, a[0], a[1], a[2], a[3]]),
        0,
        port,
    ))
}

#[test]
fn connect_advertises_exact_s0_and_basic_commands_match_vectors() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();

    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::GET_STATUS]), 0, &mut port),
        Dispatch::Ignored
    );
    assert_eq!(
        connect(&mut session, &mut port),
        [0xFF, 1, 0, 8, 8, 0, 1, 1]
    );
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(
        response(session.handle_packet(&map, &pdu(&[command::GET_STATUS]), 0, &mut port)),
        [0xFF, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        response(session.handle_packet(&map, &pdu(&[command::SYNCH]), 0, &mut port)),
        [0xFE, ErrorCode::CommandSynch as u8]
    );
}

#[test]
fn public_reference_command_and_error_ids_are_pinned_as_literals() {
    assert_eq!(command::CONNECT, 0xFF);
    assert_eq!(command::DISCONNECT, 0xFE);
    assert_eq!(command::GET_STATUS, 0xFD);
    assert_eq!(command::SYNCH, 0xFC);
    assert_eq!(command::SET_MTA, 0xF6);
    assert_eq!(command::UPLOAD, 0xF5);
    assert_eq!(command::DOWNLOAD, 0xF0);
    assert_eq!(ErrorCode::CommandSynch as u8, 0x00);
    assert_eq!(ErrorCode::CommandBusy as u8, 0x10);
    assert_eq!(ErrorCode::CommandUnknown as u8, 0x20);
    assert_eq!(ErrorCode::CommandSyntax as u8, 0x21);
    assert_eq!(ErrorCode::OutOfRange as u8, 0x22);
    assert_eq!(ErrorCode::WriteProtected as u8, 0x23);
    assert_eq!(ErrorCode::AccessDenied as u8, 0x24);
    assert_eq!(ErrorCode::ModeNotValid as u8, 0x27);
    assert_eq!(ErrorCode::Generic as u8, 0x31);
}

#[test]
fn runtime_types_are_fixed_size_copy_data_without_drop_work() {
    assert!(core::mem::size_of::<Packet>() <= 16);
    assert!(core::mem::size_of::<ProviderRequest>() <= 128);
    assert!(core::mem::size_of::<Session>() <= 256);
    assert!(!core::mem::needs_drop::<Packet>());
    assert!(!core::mem::needs_drop::<ProviderRequest>());
    assert!(!core::mem::needs_drop::<Session>());
}

#[test]
fn every_unlisted_single_byte_command_is_unknown_while_connected() {
    for command_id in u8::MIN..=u8::MAX {
        if cfg!(feature = "calibration-store") && command_id == 0xf9 { continue; }
        if [
            command::CONNECT,
            command::DISCONNECT,
            command::GET_STATUS,
            command::SYNCH,
            command::SET_MTA,
            command::UPLOAD,
            command::DOWNLOAD,
        ]
        .contains(&command_id)
        {
            continue;
        }
        let map = VirtualMap::new(&REGIONS).unwrap();
        let mut session = Session::new();
        let mut port = Port::default();
        connect(&mut session, &mut port);
        assert_eq!(
            response(session.handle_packet(&map, &pdu(&[command_id]), 0, &mut port)),
            [0xFE, ErrorCode::CommandUnknown as u8],
            "command {command_id:#04x}"
        );
    }
}

#[test]
fn malformed_requests_and_reserved_fields_are_syntax_errors() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);

    for bytes in [
        &[command::GET_STATUS, 0][..],
        &[command::SYNCH, 0],
        &[command::DISCONNECT, 0],
        &[command::SET_MTA, 0, 0],
        &[command::SET_MTA, 1, 0, 0, 0, 0, 0, 0],
        &[command::SET_MTA, 0, 1, 0, 0, 0, 0, 0],
        &[command::UPLOAD],
        &[command::DOWNLOAD],
        &[command::DOWNLOAD, 2, 0],
    ] {
        assert_eq!(
            response(session.handle_packet(&map, &pdu(bytes), 0, &mut port)),
            [0xFE, ErrorCode::CommandSyntax as u8],
            "request {bytes:02x?}"
        );
    }
    assert_eq!(session.mta(), None);
    assert!(port.requests.is_empty());
}

#[test]
fn invalid_counts_extensions_and_complete_scalar_widths_are_out_of_range() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);

    let bad_extension = [command::SET_MTA, 0, 0, 1, 0, 0, 0, 0];
    assert_eq!(
        response(session.handle_packet(&map, &pdu(&bad_extension), 0, &mut port)),
        [0xFE, ErrorCode::OutOfRange as u8]
    );
    for request in [
        &[command::UPLOAD, 0][..],
        &[command::UPLOAD, 8],
        &[command::DOWNLOAD, 0],
        &[command::DOWNLOAD, 1, 0],
        &[command::DOWNLOAD, 3, 0, 0, 0],
    ] {
        assert_eq!(
            response(session.handle_packet(&map, &pdu(request), 0, &mut port)),
            [0xFE, ErrorCode::OutOfRange as u8]
        );
    }
}

#[test]
fn supported_u32_download_still_requires_valid_mta() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new(); let mut port = Port::default();
    connect(&mut session, &mut port);
    assert_eq!(response(session.handle_packet(&map,&pdu(&[command::DOWNLOAD,4,0,0,0,0]),0,&mut port)),[0xfe,ErrorCode::AccessDenied as u8]);
}

#[test]
fn upload_uses_descriptor_offsets_and_advances_only_after_success() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    assert_eq!(set_mta(&mut session, &mut port, 5), [0xFF]);

    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::UPLOAD, 7]), 0, &mut port),
        Dispatch::Deferred
    );
    let read = match port.take() {
        ProviderRequest::Read(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(read.descriptor_id, 1);
    assert_eq!(read.offset, 5);
    assert_eq!(read.length, 7);
    let data = ReadData::new([1, 2, 3, 4, 5, 6, 7], 7).unwrap();
    assert_eq!(
        response(session.complete(
            ProviderCompletion::Read {
                correlation: read.correlation,
                result: ReadResult::Data(data)
            },
            &mut port
        )),
        [0xFF, 1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(session.mta(), Some(12));

    set_mta(&mut session, &mut port, 31);
    assert_eq!(
        response(session.handle_packet(&map, &pdu(&[command::UPLOAD, 2]), 0, &mut port)),
        [0xFE, ErrorCode::AccessDenied as u8]
    );
    assert_eq!(session.mta(), Some(31));
}

#[test]
fn download_waits_for_applied_then_advances_and_releases() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);

    assert_eq!(
        session.handle_packet(
            &map,
            &pdu(&[command::DOWNLOAD, 2, 0xE8, 0x03]),
            50,
            &mut port
        ),
        Dispatch::Deferred
    );
    assert_eq!(session.mta(), Some(0x1000));
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(apply.operation.service_epoch, 7);
    assert_eq!(apply.descriptor_id, 0x1001);
    assert_eq!(apply.encoded_value, [0xE8, 0x03, 0, 0]);
    assert_eq!(apply.expires_at_us, 50 + APPLY_DEADLINE_US);

    assert_eq!(
        response(session.complete(
            ProviderCompletion::Write {
                correlation: apply.operation.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 1
                }
            },
            &mut port
        )),
        [0xFF]
    );
    assert_eq!(session.mta(), Some(0x1002));
    assert!(session.has_retirement());
    assert_eq!(
        port.take(),
        ProviderRequest::ReleaseOutcome {
            operation: apply.operation
        }
    );

    set_mta(&mut session, &mut port, 0x1000);
    assert_eq!(
        response(session.handle_packet(
            &map,
            &pdu(&[command::DOWNLOAD, 2, 0xE8, 0x03]),
            100,
            &mut port
        )),
        [0xFE, ErrorCode::CommandBusy as u8]
    );
    assert_eq!(
        session.complete(
            ProviderCompletion::Released {
                correlation: apply.operation.correlation()
            },
            &mut port
        ),
        Dispatch::Ignored
    );
    assert!(!session.has_retirement());
}

#[test]
fn release_admission_saturation_retains_outcome_and_retries_without_reapply() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 10, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };

    port.fail_next = Some(SubmitError::Busy);
    assert_eq!(
        response(session.complete(
            ProviderCompletion::Write {
                correlation: apply.operation.correlation(),
                result: WriteResult::Applied {
                    changed: false,
                    active_revision: 0
                }
            },
            &mut port
        )),
        [0xFF]
    );
    assert!(session.has_retirement());
    assert!(port.requests.is_empty());

    assert_eq!(session.service(11, &mut port).dispatch, Dispatch::Ignored);
    assert_eq!(
        port.take(),
        ProviderRequest::ReleaseOutcome {
            operation: apply.operation
        }
    );
    assert_eq!(session.mta(), Some(0x1002));
    assert_eq!(session.state(), SessionState::Ready);
}

#[test]
fn write_access_failures_never_submit_or_advance_mta() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);

    for (address, error) in [
        (0, ErrorCode::WriteProtected),
        (0x1001, ErrorCode::AccessDenied),
        (0x9999, ErrorCode::AccessDenied),
    ] {
        set_mta(&mut session, &mut port, address);
        assert_eq!(
            response(session.handle_packet(
                &map,
                &pdu(&[command::DOWNLOAD, 2, 1, 0]),
                0,
                &mut port
            )),
            [0xFE, error as u8]
        );
        assert_eq!(session.mta(), Some(address));
        assert!(port.requests.is_empty());
    }
}

#[test]
fn provider_rejection_maps_truthfully_and_preserves_mta() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    for (reason, error) in [
        (RejectionReason::BadEncoding, ErrorCode::OutOfRange),
        (RejectionReason::Bounds, ErrorCode::OutOfRange),
        (RejectionReason::CrossField, ErrorCode::OutOfRange),
        (RejectionReason::ReadOnly, ErrorCode::WriteProtected),
        (RejectionReason::UnknownVariable, ErrorCode::AccessDenied),
        (RejectionReason::Expired, ErrorCode::Generic),
        (RejectionReason::WrongLifecycle, ErrorCode::ModeNotValid),
        (RejectionReason::RevisionMismatch, ErrorCode::CommandBusy),
        (RejectionReason::OutputBusy, ErrorCode::CommandBusy),
        (RejectionReason::OutputUnavailable, ErrorCode::CommandBusy),
    ] {
        let mut session = Session::new();
        let mut port = Port::default();
        connect(&mut session, &mut port);
        set_mta(&mut session, &mut port, 0x1000);
        assert_eq!(
            session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port),
            Dispatch::Deferred
        );
        let apply = match port.take() {
            ProviderRequest::Apply(request) => request,
            other => panic!("unexpected request: {other:?}"),
        };
        assert_eq!(
            response(session.complete(
                ProviderCompletion::Write {
                    correlation: apply.operation.correlation(),
                    result: WriteResult::Rejected { reason }
                },
                &mut port
            )),
            [0xFE, error as u8]
        );
        assert_eq!(session.mta(), Some(0x1000));
    }
}

#[test]
fn nonretained_owner_busy_is_retryable_without_release_or_mta_change() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(
        response(session.complete(
            ProviderCompletion::Write {
                correlation: apply.operation.correlation(),
                result: WriteResult::Busy
            },
            &mut port
        )),
        [0xFE, ErrorCode::CommandBusy as u8]
    );
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(session.mta(), Some(0x1000));
    assert!(!session.has_retirement());
    assert!(port.requests.is_empty());
}

#[test]
fn unselected_provider_policy_is_released_and_fenced_without_a_wire_error() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(
        session.complete(
            ProviderCompletion::Write {
                correlation: apply.operation.correlation(),
                result: WriteResult::Rejected {
                    reason: RejectionReason::UnsupportedPolicy
                }
            },
            &mut port
        ),
        Dispatch::Fenced
    );
    assert_eq!(session.state(), SessionState::Recovery);
    assert_eq!(
        port.take(),
        ProviderRequest::ReleaseOutcome {
            operation: apply.operation
        }
    );
}

#[test]
fn deadline_resolves_and_applied_after_deadline_is_still_success() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 20, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };

    assert_eq!(
        session.service(apply.expires_at_us, &mut port).dispatch,
        Dispatch::Deferred
    );
    assert_eq!(session.state(), SessionState::Resolving);
    assert_eq!(
        port.take(),
        ProviderRequest::ResolveOrCancel {
            operation: apply.operation
        }
    );
    assert_eq!(
        response(session.complete(
            ProviderCompletion::Write {
                correlation: apply.operation.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 1
                }
            },
            &mut port
        )),
        [0xFF]
    );
}

#[test]
fn transport_loss_and_stale_outcomes_never_fabricate_errors() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(session.transport_lost(&mut port), Dispatch::Fenced);
    assert_eq!(
        port.take(),
        ProviderRequest::ResolveOrCancel {
            operation: apply.operation
        }
    );
    assert_eq!(
        session.complete(
            ProviderCompletion::Write {
                correlation: apply.operation.correlation(),
                result: WriteResult::Applied {
                    changed: true,
                    active_revision: 1
                }
            },
            &mut port
        ),
        Dispatch::Fenced
    );
    assert_eq!(session.state(), SessionState::Recovery);

    let mut stale_session = Session::new();
    let mut stale_port = Port::default();
    connect(&mut stale_session, &mut stale_port);
    set_mta(&mut stale_session, &mut stale_port, 0x1000);
    stale_session.handle_packet(
        &map,
        &pdu(&[command::DOWNLOAD, 2, 1, 0]),
        0,
        &mut stale_port,
    );
    let stale_apply = match stale_port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(
        stale_session.complete(
            ProviderCompletion::Write {
                correlation: stale_apply.operation.correlation(),
                result: WriteResult::StaleOperation
            },
            &mut stale_port
        ),
        Dispatch::Fenced
    );
}

#[test]
fn command_while_write_pending_faults_and_requests_resolution() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::GET_STATUS]), 1, &mut port),
        Dispatch::Fenced
    );
    assert_eq!(session.state(), SessionState::Resolving);
    assert_eq!(
        port.take(),
        ProviderRequest::ResolveOrCancel {
            operation: apply.operation
        }
    );
}

#[test]
fn admission_failure_is_busy_before_state_change_or_fences_if_unavailable() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);

    port.fail_next = Some(SubmitError::Busy);
    assert_eq!(
        response(session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port)),
        [0xFE, ErrorCode::CommandBusy as u8]
    );
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(session.mta(), Some(0x1000));

    port.fail_next = Some(SubmitError::Unavailable);
    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port),
        Dispatch::Fenced
    );
    // Uncertain admission retains the write, so the durable fence resolves it.
    assert_eq!(session.state(), SessionState::Resolving);
    assert_eq!(session.mta(), Some(0x1000));
    assert_eq!(
        session.service(20_000, &mut port).dispatch,
        Dispatch::Fenced
    );
    assert!(matches!(
        port.take(),
        ProviderRequest::ResolveOrCancel { .. }
    ));
    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::CONNECT, 0]), 20_001, &mut port),
        Dispatch::Fenced
    );
}

#[test]
fn disconnect_is_positive_only_after_quiesce() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    assert_eq!(
        session.handle_packet(&map, &pdu(&[command::DISCONNECT]), 0, &mut port),
        Dispatch::Deferred
    );
    let correlation = match port.take() {
        ProviderRequest::Quiesce { correlation } => correlation,
        other => panic!("unexpected request: {other:?}"),
    };
    assert_eq!(
        response(session.complete(
            ProviderCompletion::Quiesced {
                correlation,
                result: QuiesceResult::Quiesced
            },
            &mut port
        )),
        [0xFF]
    );
    assert_eq!(session.state(), SessionState::Disconnected);
    assert_eq!(session.mta(), None);
}

#[test]
fn mismatched_completion_fences_session() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    set_mta(&mut session, &mut port, 0x1000);
    session.handle_packet(&map, &pdu(&[command::DOWNLOAD, 2, 1, 0]), 0, &mut port);
    let apply = match port.take() {
        ProviderRequest::Apply(request) => request,
        other => panic!("unexpected request: {other:?}"),
    };
    let wrong = ProviderOperation {
        sequence: apply.operation.sequence + 1,
        ..apply.operation
    };
    assert_eq!(
        session.complete(
            ProviderCompletion::Write {
                correlation: wrong.correlation(),
                result: WriteResult::Cancelled
            },
            &mut port
        ),
        Dispatch::Fenced
    );
    assert_eq!(session.state(), SessionState::Resolving);
    assert_eq!(
        port.take(),
        ProviderRequest::ResolveOrCancel {
            operation: apply.operation
        }
    );
}

#[cfg(not(feature = "calibration-store"))]
#[test]
fn restricted_p_set_request_stays_unknown_in_immutable_s0_for_valid_and_malformed_packets() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let mut session = Session::new();
    let mut port = Port::default();
    connect(&mut session, &mut port);
    for request in [
        &[0xf9, 1, 0, 0][..],
        &[0xf9][..],
        &[0xf9, 1][..],
        &[0xf9, 2, 0, 0][..],
    ] {
        assert_eq!(
            response(session.handle_packet(&map, &pdu(request), 0, &mut port)),
            [0xfe, 0x20],
            "S0 packet {request:02x?}"
        );
    }
    assert!(port.requests.is_empty());
}

#[cfg(feature = "calibration-store")]
#[test]
fn calibration_save_waits_for_verified_correlated_completion() {
    let map=VirtualMap::new(&REGIONS).unwrap();
    for verified in [false,true] {
        let (mut s,mut p)=(Session::new(),Port::default());connect(&mut s,&mut p);
        for bytes in [&[0xf9,1][..], &[0xf9,2,0,0][..]] {
            assert!(matches!(s.handle_packet(&map,&pdu(bytes),0,&mut p),Dispatch::Respond(_)));
            assert!(p.requests.is_empty());
        }
        assert_eq!(s.handle_packet(&map,&pdu(&[0xf9,1,0,0]),0,&mut p),Dispatch::Deferred);
        assert!(s.target_awaiting_owner());
        let ProviderRequest::StoreCalibration { correlation }=p.take() else { panic!() };
        let result=response(s.complete(ProviderCompletion::CalibrationStored { correlation,verified },&mut p));
        assert_eq!(result,if verified {vec![0xff]} else {vec![0xfe,0x31]});
        assert_eq!(response(s.handle_packet(&map,&pdu(&[0xfd]),0,&mut p)),[0xff,0,0,0,0,0]);
    }
}
#[cfg(feature = "calibration-store")]
#[test]
fn lost_save_response_never_reports_success() {
    let map=VirtualMap::new(&REGIONS).unwrap(); let (mut s,mut p)=(Session::new(),Port::default());connect(&mut s,&mut p);
    assert_eq!(s.handle_packet(&map,&pdu(&[0xf9,1,0,0]),0,&mut p),Dispatch::Deferred);
    let ProviderRequest::StoreCalibration { correlation }=p.take() else {panic!()};
    s.transport_lost(&mut p);
    assert_eq!(s.complete(ProviderCompletion::CalibrationStored{correlation,verified:true},&mut p),Dispatch::Fenced);
    assert!(!s.has_pending_work());
}
