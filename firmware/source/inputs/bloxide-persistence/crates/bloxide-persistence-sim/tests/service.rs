use bloxide_persistence::{
    Classification, CommandKind, CompletionError, CompletionStatus, FailureKind, FailurePhase,
    Geometry, OperationKey, Outcome, PersistenceService, ReadIssue, Recovery, Resolve,
    SaveResponse, Schema, SchemaError, ServiceMode, ServiceState, Slot, SlotRead, Snapshot,
    classify_slot, encode_record, recover,
};
use bloxide_persistence_sim::Simulator;

#[derive(Clone, Copy)]
struct LedSchema;

impl Schema for LedSchema {
    fn id(&self) -> u16 {
        1
    }

    fn payload_len(&self) -> u16 {
        4
    }

    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        out[..2].copy_from_slice(&1000_u16.to_le_bytes());
        out[2..4].copy_from_slice(&500_u16.to_le_bytes());
        Ok(())
    }

    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError> {
        if bytes.len() != 4 {
            return Err(SchemaError::InvalidValue);
        }
        let period = u16::from_le_bytes(bytes[..2].try_into().unwrap());
        let duty = u16::from_le_bytes(bytes[2..].try_into().unwrap());
        if (100..=10_000).contains(&period) && duty <= 1000 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}

fn key(sequence: u64) -> OperationKey {
    OperationKey {
        service_epoch: 1,
        session_generation: 1,
        sequence,
    }
}

fn snapshot(period: u16, duty: u16, revision: u32, sequence: u64) -> Snapshot {
    let mut payload = [0_u8; 4];
    payload[..2].copy_from_slice(&period.to_le_bytes());
    payload[2..].copy_from_slice(&duty.to_le_bytes());
    Snapshot::new(&LedSchema, &payload, 7, revision, key(sequence)).unwrap()
}

fn geometry() -> Geometry {
    Geometry::new(1024, 8).unwrap()
}

fn blank_recovery() -> Recovery {
    recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap()
}

fn new_service(recovery: Recovery, recovery_authorized: bool) -> PersistenceService<LedSchema> {
    PersistenceService::new(
        geometry(),
        LedSchema,
        recovery,
        recovery_authorized,
        1,
        1,
        1,
        0,
        test_backend(),
    )
}

fn drive(service: &mut PersistenceService<LedSchema>, simulator: &mut Simulator) -> usize {
    let mut commands = 0;
    while service.retained().is_none() {
        let command = service
            .take_command(0)
            .unwrap()
            .expect("active service must make bounded progress");
        assert!(service.take_command(0).unwrap().is_none());
        let completion = simulator.execute(command);
        service.complete(completion).unwrap();
        commands += 1;
        assert!(commands < 2000);
    }
    commands
}

fn classify_media(simulator: &Simulator, slot: Slot) -> Classification {
    classify_slot(
        SlotRead {
            bytes: simulator.slot(slot),
            issue: None,
        },
        geometry(),
        &LedSchema,
    )
}

#[test]
fn first_save_is_seq1_slot_a_and_exactly_99_commands() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 44);
    let captured = snapshot(2000, 250, 10, 1);
    assert_eq!(
        service.save(key(1), captured, 100, || 10),
        SaveResponse::Accepted
    );
    assert_eq!(drive(&mut service, &mut simulator), 99);
    assert!(matches!(
        service.retained(),
        Some(Outcome::Durable { record, existing: false, .. })
            if record.sequence == 1 && record.snapshot == captured
    ));
    assert!(matches!(
        classify_media(&simulator, Slot::A),
        Classification::Valid(record)
            if record.sequence == 1 && record.is_same_durable_snapshot(&captured)
    ));
    assert_eq!(classify_media(&simulator, Slot::B), Classification::Empty);
    assert_eq!(simulator.erase_commands(), 1);
    assert_eq!(simulator.program_commands(), 81);
}

#[test]
fn captured_bytes_do_not_follow_later_ram_edits() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 45);
    let mut active = [0_u8; 4];
    active[..2].copy_from_slice(&2000_u16.to_le_bytes());
    active[2..].copy_from_slice(&250_u16.to_le_bytes());
    let captured = Snapshot::new(&LedSchema, &active, 7, 10, key(1)).unwrap();
    assert_eq!(
        service.save(key(1), captured, 100, || 10),
        SaveResponse::Accepted
    );
    active[..2].copy_from_slice(&4000_u16.to_le_bytes());
    active[2..].copy_from_slice(&500_u16.to_le_bytes());
    drive(&mut service, &mut simulator);
    let Classification::Valid(record) = classify_media(&simulator, Slot::A) else {
        panic!("durable record missing")
    };
    assert_eq!(record.snapshot.bytes(), &[0xD0, 0x07, 0xFA, 0x00]);
    assert_eq!(active, [0xA0, 0x0F, 0xF4, 0x01]);
}

#[test]
fn qualification_denies_pair00_before_wear_or_erase() {
    let current = bloxide_persistence::Record::new(7, snapshot(2000, 250, 10, 7));
    let target = bloxide_persistence::Record::new(14, snapshot(1000, 500, 9, 14));
    let current_image = encode_record(&current, geometry());
    let target_image = encode_record(&target, geometry());
    let mut simulator = new_simulator(geometry(), 100, 46);
    simulator.slot_mut(Slot::A)[..current_image.bytes().len()]
        .copy_from_slice(current_image.bytes());
    simulator.slot_mut(Slot::B)[..target_image.bytes().len()].copy_from_slice(target_image.bytes());
    let one_index = if simulator.slot(Slot::B)[0] == 0 {
        1
    } else {
        0
    };
    simulator.slot_mut(Slot::B)[one_index] = 0;
    let recovery = recover(
        classify_media(&simulator, Slot::A),
        classify_media(&simulator, Slot::B),
        &LedSchema,
    )
    .unwrap();
    let mut service = new_service(recovery, true);
    assert_eq!(
        service.save(key(1), snapshot(3000, 750, 11, 1), 100, || 10),
        SaveResponse::Accepted
    );
    assert_eq!(drive(&mut service, &mut simulator), 4);
    assert!(matches!(
        service.retained(),
        Some(Outcome::Failed {
            phase: FailurePhase::Qualification,
            reason: FailureKind::UnsafeErasePrestate,
            no_new_commit: true,
            ..
        })
    ));
    assert_eq!(simulator.erase_commands(), 0);
    assert_eq!(simulator.wear_remaining(Slot::B), 100);
}

#[test]
fn permit_is_consumed_before_the_only_erase() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 47);
    service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
    let mut observed_wear = false;
    let mut observed_erase = false;
    while service.retained().is_none() {
        let command = service.take_command(0).unwrap().unwrap();
        match command.header.kind {
            CommandKind::ConsumeErasePermit { .. } => {
                assert!(!observed_erase);
                observed_wear = true;
            }
            CommandKind::Erase { .. } => {
                assert!(observed_wear);
                assert!(!observed_erase);
                observed_erase = true;
            }
            CommandKind::Read | CommandKind::Program => {}
        }
        let completion = simulator.execute(command);
        service.complete(completion).unwrap();
    }
    assert!(observed_wear && observed_erase);
}

#[test]
fn duplicate_wear_command_receipt_consumes_authority_once() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 4700);
    service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);

    loop {
        let command = service.take_command(0).unwrap().unwrap();
        if matches!(command.header.kind, CommandKind::ConsumeErasePermit { .. }) {
            let first = simulator.execute(command.clone());
            let duplicate = simulator.execute(command);
            assert_eq!(first.status, CompletionStatus::WearGranted { permit: 1 });
            assert_eq!(
                duplicate.status,
                CompletionStatus::WearGranted { permit: 1 }
            );
            assert_eq!(simulator.wear_remaining(Slot::A), 99);
            service.complete(first).unwrap();
            break;
        }
        let completion = simulator.execute(command);
        service.complete(completion).unwrap();
    }

    drive(&mut service, &mut simulator);
    assert!(matches!(service.retained(), Some(Outcome::Durable { .. })));
    assert_eq!(simulator.wear_remaining(Slot::A), 99);
}

#[test]
fn stale_completion_never_advances_or_discards_the_real_command() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 48);
    service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
    let command = service.take_command(0).unwrap().unwrap();
    let mut stale = simulator.execute(command.clone());
    stale.header.command_sequence += 1;
    assert_eq!(
        service.complete(stale),
        Err(CompletionError::CorrelationMismatch)
    );
    assert!(service.take_command(0).unwrap().is_none());
    assert!(service.health().correlation_fault);
    let exact = simulator.execute(command);
    service.complete(exact).unwrap();
    assert_eq!(service.mode(), ServiceMode::ReadOnlyFault);
}

#[test]
fn malformed_completion_is_fenced_without_discarding_the_exact_completion() {
    for malformed_status in [
        CompletionStatus::Ok,
        CompletionStatus::Read {
            len: geometry().granule_bytes(),
            issue: None,
        },
    ] {
        let mut service = new_service(blank_recovery(), false);
        let mut simulator = new_simulator(geometry(), 100, 4800);
        service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
        let command = service.take_command(0).unwrap().unwrap();
        assert_eq!(command.header.kind, CommandKind::Read);
        let exact = simulator.execute(command);
        let mut malformed = exact.clone();
        malformed.status = malformed_status;
        let state = service.state();
        let expected_error = match malformed_status {
            CompletionStatus::Ok => CompletionError::WrongCompletionKind,
            CompletionStatus::Read { .. } => CompletionError::InvalidReadLength,
            _ => unreachable!(),
        };

        assert_eq!(service.complete(malformed), Err(expected_error));
        assert_eq!(service.state(), state);
        assert_eq!(service.mode(), ServiceMode::ReadOnlyFault);
        assert!(service.take_command(0).unwrap().is_none());

        service.complete(exact).unwrap();
        assert_eq!(service.state(), ServiceState::Retained);
        assert!(service.take_command(0).unwrap().is_none());
        assert_eq!(simulator.erase_commands(), 0);
    }
}

#[test]
fn post_marker_lost_completion_reconciles_exact_new_record() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 49);
    let captured = snapshot(2000, 250, 10, 1);
    service.save(key(1), captured, 100, || 10);
    loop {
        let command = service.take_command(0).unwrap().unwrap();
        if command.header.kind == CommandKind::Program
            && command.header.offset == u32::from(geometry().marker_offset())
        {
            let mut completion = simulator.execute(command);
            completion.status = CompletionStatus::Failed { quiescent: true };
            service.complete(completion).unwrap();
            break;
        }
        let completion = simulator.execute(command);
        service.complete(completion).unwrap();
    }
    drive(&mut service, &mut simulator);
    assert!(matches!(
        service.retained(),
        Some(Outcome::Durable { record, .. })
            if record.sequence == 1 && record.snapshot == captured
    ));
}

#[test]
fn reset_waits_for_terminal_release_and_does_not_reacquire() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 50);
    service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
    service.reset();
    assert_eq!(service.mode(), ServiceMode::Quiescing);
    drive(&mut service, &mut simulator);
    assert_eq!(service.mode(), ServiceMode::Quiescing);
    service.release(key(1)).unwrap();
    assert_eq!(service.mode(), ServiceMode::Stopped);
    service.resume(2, 2, 2).unwrap();
    assert_eq!(service.mode(), ServiceMode::Running);
    assert!(matches!(
        service.resolve(key(1)),
        Resolve::Stale { durable: Some(_) }
    ));
}

#[test]
fn cancellation_is_retained_and_pending_save_cannot_be_cancelled() {
    let mut service = new_service(blank_recovery(), false);
    assert!(matches!(
        service.resolve_or_cancel(key(1)),
        Resolve::Terminal(Outcome::Cancelled { key: cancelled }) if cancelled == key(1)
    ));
    assert!(matches!(
        service.resolve_or_cancel(key(1)),
        Resolve::Terminal(Outcome::Cancelled { key: cancelled }) if cancelled == key(1)
    ));
    service.release(key(1)).unwrap();
    assert!(matches!(service.resolve(key(1)), Resolve::Retired { .. }));

    let mut service = new_service(blank_recovery(), false);
    service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
    assert_eq!(service.resolve_or_cancel(key(1)), Resolve::Pending);
}

#[test]
fn same_durable_snapshot_uses_no_output_or_wear_slot() {
    let current = bloxide_persistence::Record::new(7, snapshot(2000, 250, 10, 7));
    let recovery = recover(
        Classification::Valid(current),
        Classification::Empty,
        &LedSchema,
    )
    .unwrap();
    let mut service = new_service(recovery, false);
    let same = Snapshot::new(&LedSchema, current.snapshot.bytes(), 7, 10, key(1)).unwrap();
    assert!(matches!(
        service.save(key(1), same, 100, || 10),
        SaveResponse::Terminal(Outcome::Durable { existing: true, record, .. })
            if record.sequence == 7
    ));
    assert!(service.take_command(0).unwrap().is_none());
}

#[test]
fn finite_authority_survives_service_reconstruction_and_denies_201st_erase() {
    let mut simulator = new_simulator(geometry(), 100, 51);
    let mut recovery = blank_recovery();
    for index in 1..=200_u64 {
        let mut service = PersistenceService::new(
            geometry(),
            LedSchema,
            recovery,
            true,
            1,
            1,
            1,
            0,
            simulator.open_session().unwrap(),
        );
        let value = if index % 2 == 0 {
            (2000, 250)
        } else {
            (3000, 750)
        };
        let captured = snapshot(value.0, value.1, index as u32, index);
        assert_eq!(
            service.save(key(index), captured, index + 10, || index),
            SaveResponse::Accepted
        );
        drive(&mut service, &mut simulator);
        assert!(matches!(service.retained(), Some(Outcome::Durable { .. })));
        recovery = recover(
            classify_media(&simulator, Slot::A),
            classify_media(&simulator, Slot::B),
            &LedSchema,
        )
        .unwrap();
    }
    assert_eq!(simulator.wear_remaining(Slot::A), 0);
    assert_eq!(simulator.wear_remaining(Slot::B), 0);
    assert_eq!(simulator.erase_commands(), 200);

    let mut service = PersistenceService::new(
        geometry(),
        LedSchema,
        recovery,
        true,
        1,
        1,
        1,
        0,
        simulator.open_session().unwrap(),
    );
    service.save(key(201), snapshot(4000, 500, 201, 201), 300, || 201);
    drive(&mut service, &mut simulator);
    assert!(matches!(
        service.retained(),
        Some(Outcome::Failed {
            phase: FailurePhase::Wear,
            reason: FailureKind::WearUnavailable,
            no_new_commit: true,
            ..
        })
    ));
    assert_eq!(simulator.erase_commands(), 200);
}

#[test]
fn corrected_ecc_is_explicit_and_locks_writes() {
    let mut service = new_service(blank_recovery(), false);
    let mut simulator = new_simulator(geometry(), 100, 52);
    service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
    let command = service.take_command(0).unwrap().unwrap();
    let mut completion = simulator.execute(command);
    completion.status = CompletionStatus::Read {
        len: completion.header.len,
        issue: Some(ReadIssue::CorrectedEcc),
    };
    service.complete(completion).unwrap();
    assert!(service.health().write_locked);
    assert!(matches!(
        service.retained(),
        Some(Outcome::Failed {
            reason: FailureKind::Read(ReadIssue::CorrectedEcc),
            ..
        })
    ));
}

#[test]
fn deadline_rate_and_same_key_conflict_boundaries_are_exact() {
    let original = snapshot(2000, 250, 10, 1);
    let different = snapshot(3000, 750, 11, 1);

    let mut before = new_service(blank_recovery(), false);
    assert_eq!(
        before.save(key(1), original, 10, || 9),
        SaveResponse::Accepted
    );
    assert_eq!(
        before.save(key(1), different, 10, || 9),
        SaveResponse::KeyConflict
    );

    for now in [10, 11] {
        let mut service = new_service(blank_recovery(), false);
        assert!(matches!(
            service.save(key(1), original, 10, || now),
            SaveResponse::Terminal(Outcome::Rejected {
                reason: bloxide_persistence::RejectReason::Expired,
                ..
            })
        ));
        assert_eq!(
            service.save(key(1), different, 20, || now),
            SaveResponse::KeyConflict
        );
    }

    let mut service = PersistenceService::new(
        geometry(),
        LedSchema,
        blank_recovery(),
        true,
        1,
        1,
        1,
        1000,
        test_backend(),
    );
    let mut simulator = new_simulator(geometry(), 100, 53);
    service.save(key(1), original, 100, || 10);
    drive(&mut service, &mut simulator);
    service.release(key(1)).unwrap();
    let second = snapshot(3000, 750, 11, 2);
    assert!(matches!(
        service.save(key(2), second, 2000, || 1009),
        SaveResponse::Terminal(Outcome::Rejected {
            reason: bloxide_persistence::RejectReason::RateLimited,
            ..
        })
    ));
    service.release(key(2)).unwrap();
    let third = snapshot(3000, 750, 11, 3);
    assert_eq!(
        service.save(key(3), third, 2000, || 1010),
        SaveResponse::Accepted
    );
}

#[test]
fn every_command_position_rejects_stale_completion_before_exact_correlation() {
    for injection in 0..99 {
        let mut service = new_service(blank_recovery(), false);
        let mut simulator = new_simulator(geometry(), 100, 1000 + injection as u64);
        service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
        let mut index = 0;
        while service.retained().is_none() {
            let command = service.take_command(0).unwrap().unwrap();
            let exact = simulator.execute(command);
            if index == injection {
                let state = service.state();
                let mut stale = exact.clone();
                if injection % 2 == 0 {
                    stale.header.io_epoch += 1;
                } else {
                    stale.header.slot = stale.header.slot.other();
                }
                assert_eq!(
                    service.complete(stale),
                    Err(CompletionError::CorrelationMismatch)
                );
                assert_eq!(service.state(), state);
                assert!(service.take_command(0).unwrap().is_none());
            }
            service.complete(exact.clone()).unwrap();
            if index == injection {
                assert_eq!(
                    service.complete(exact),
                    Err(CompletionError::NothingOutstanding)
                );
            }
            index += 1;
        }
        assert!(index <= 99);
        if injection < 94 {
            assert!(matches!(
                service.retained(),
                Some(Outcome::Failed {
                    no_new_commit: true,
                    ..
                })
            ));
        } else {
            assert!(matches!(service.retained(), Some(Outcome::Durable { .. })));
        }
        assert!(service.health().write_locked);
        assert!(service.take_command(0).unwrap().is_none());
    }
}

#[test]
fn fixed_service_and_four_client_broker_fit_the_portable_budget() {
    let service = core::mem::size_of::<PersistenceService<LedSchema>>();
    let broker = core::mem::size_of::<bloxide_persistence_calibration::Broker<4>>();
    assert!(
        service + broker <= 8192,
        "service={service} broker={broker}"
    );
}

#[test]
fn reset_at_ten_workflow_boundaries_retains_then_stops() {
    for boundary in [0, 3, 4, 5, 9, 20, 84, 85, 90, 98] {
        for _repeat in 0..3 {
            let mut service = new_service(blank_recovery(), false);
            let mut simulator = new_simulator(geometry(), 100, 3000 + boundary as u64);
            service.save(key(1), snapshot(2000, 250, 10, 1), 100, || 10);
            let mut index = 0;
            while service.retained().is_none() {
                if index == boundary {
                    service.reset();
                    assert_eq!(service.mode(), ServiceMode::Quiescing);
                }
                let command = service.take_command(0).unwrap().unwrap();
                let completion = simulator.execute(command);
                service.complete(completion).unwrap();
                index += 1;
            }
            assert!(matches!(service.retained(), Some(Outcome::Durable { .. })));
            service.release(key(1)).unwrap();
            assert_eq!(service.mode(), ServiceMode::Stopped);
        }
    }
}

fn test_backend() -> bloxide_persistence::BackendConfig {
    bloxide_persistence::BackendConfig {
        lease: bloxide_persistence::WearLease(1),
        timing: bloxide_persistence::BackendTiming {
            read: 10,
            permit: 10,
            erase: 100,
            program: 10,
            quiesce: 100,
        },
    }
}

fn new_simulator(g: Geometry, w: u16, p: u64) -> Simulator {
    let mut sim = Simulator::new(g, w, p);
    assert_eq!(sim.open_session(), Some(test_backend()));
    sim
}
