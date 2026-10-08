mod common;
use bloxide_persistence::*;
use bloxide_persistence_sim::Simulator;
use common::*;

#[test]
fn c01_c19_real_service_all_prefixes_three_starts_five_geometries() {
    let mut count = 0;
    for (e, g) in GEOMETRIES {
        let geo = Geometry::new(e, g).unwrap();
        for start in 0..3 {
            let mut m = Simulator::new(geo, 100, 88);
            let mut s = boot(&mut m, geo);
            // Produce prestates through real saves: seq7 A / seq6 B and the reverse.
            if start == 1 {
                for n in 1..=7 {
                    save(&mut s, &mut m, n);
                }
            }
            if start == 2 {
                for n in 1..=7 {
                    save(&mut s, &mut m, n);
                }
                // Initial-condition fixture only: name the two already service-written
                // physical units in reverse order, then perform a real boot scan.
                // No record is manufactured; every tested erase/program still comes
                // from a newly admitted live service command and charged receipt.
                let a = m.slot(Slot::A).to_vec();
                let b = m.slot(Slot::B).to_vec();
                m.slot_mut(Slot::A).copy_from_slice(&b);
                m.slot_mut(Slot::B).copy_from_slice(&a);
                s = boot(&mut m, geo);
                assert_eq!(s.read_durable().unwrap().sequence, 7);
                assert_eq!(recovery(&m, geo).selected().unwrap().slot, Slot::B);
            }
            let old = selected(&m, geo);
            let previous_seq = old.as_ref().map_or(0, |r| r.sequence);
            let n = 20;
            let expected = identity(Record::new(previous_seq + 1, capture(n)));
            let current_slot = s
                .read_durable()
                .map(|_| recovery(&m, geo).selected().unwrap().slot);
            assert_eq!(
                s.save(key(n), capture(n), 100, || 0),
                SaveResponse::Accepted
            );
            while !s.ownership_settled() {
                let c = s.take_command(0).unwrap().unwrap();
                let max = match c.header.kind {
                    CommandKind::Erase { .. } => Some(e as usize),
                    CommandKind::Program => Some(g as usize),
                    _ => None,
                };
                if let Some(max) = max {
                    for prefix in 0..=max {
                        let mut cut = m.clone();
                        let accepted = if matches!(c.header.kind, CommandKind::Erase { .. }) {
                            cut.execute_erase_prefix(&c, prefix)
                        } else {
                            cut.execute_program_prefix(&c, prefix)
                        };
                        assert!(accepted);
                        if let Some(slot) = current_slot {
                            assert_eq!(cut.slot(slot), m.slot(slot));
                        }
                        let observed = selected(&cut, geo);
                        assert!(observed == old || observed == Some(expected.clone()));
                        let reboot = boot(&mut cut, geo);
                        assert_eq!(reboot.read_durable().map(identity), observed);
                        count += 1;
                    }
                }
                s.complete(m.execute(c)).unwrap();
            }
            assert_eq!(selected(&m, geo), Some(expected));
        }
    }
    println!(
        "C01/C19: {count} real-service destructive prefix branches with full-field independent oracle"
    );
    assert_eq!(count, 47160);
}

#[test]
fn c19_hundred_real_thermostat_service_save_reboot_cycles() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 99);
    for n in 1..=100 {
        let mut s = boot(&mut m, g);
        save(&mut s, &mut m, n);
        assert_eq!(selected(&m, g), Some(identity(Record::new(n, capture(n)))));
        let reboot = boot(&mut m, g);
        assert_eq!(reboot.read_durable().map(identity), selected(&m, g));
        // All MCU counters restart; lease uniqueness comes only from m.
    }
    assert_eq!(m.erase_commands(), 100);
    assert_eq!(m.wear_remaining(Slot::A), 50);
    assert_eq!(m.wear_remaining(Slot::B), 50);
    println!("C19: 100 actual service save/reboot cycles; full persistent labels and bytes");
}

#[test]
fn c06_120_lifecycle_traces_preserve_owned_commands_and_epoch_fences() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut traces = 0;
    for boundary in [0, 3, 4, 5, 9, 20, 84, 85, 94, 98] {
        for lifecycle in 0..4 {
            for _repeat in 0..3 {
                let mut m = Simulator::new(g, 100, 77);
                let mut s = boot(&mut m, g);
                s.save(key(1), capture(1), 100, || 0);
                for _ in 0..boundary {
                    let c = s.take_command(0).unwrap().unwrap();
                    s.complete(m.execute(c)).unwrap();
                }
                let c = s.take_command(0).unwrap().unwrap();
                let r = m.execute(c);
                if lifecycle == 3 {
                    let old_selected = selected(&m, g);
                    let mut reboot = boot(&mut m, g);
                    assert_eq!(reboot.read_durable().map(identity), old_selected);
                    assert_eq!(reboot.complete(r), Err(CompletionError::NothingOutstanding));
                    // Resume after power cut actually uses service, authority and Q.
                    assert_eq!(
                        reboot.save(key(2), capture(2), 100, || 0),
                        SaveResponse::Accepted
                    );
                    drive(&mut reboot, &mut m);
                    assert!(matches!(reboot.retained(), Some(Outcome::Durable { .. })));
                } else {
                    match lifecycle {
                        0 => s.reset(),
                        1 => s.stop(),
                        _ => s.request_quiesce(),
                    }
                    assert!(s.resume(18, 4, 24).is_err());
                    assert!(s.release(key(1)).is_err());
                    s.complete(r.clone()).unwrap();
                    drive(&mut s, &mut m);
                    assert_eq!(s.mode(), ServiceMode::Quiescing);
                    s.release(key(1)).unwrap();
                    assert_eq!(s.mode(), ServiceMode::Stopped);
                    s.resume(18, 4, 24).unwrap();
                    assert_eq!(s.complete(r), Err(CompletionError::NothingOutstanding));
                    assert!(matches!(s.resolve(key(1)), Resolve::Stale { .. }));
                }
                traces += 1;
            }
        }
    }
    assert_eq!(traces, 120);
    println!("C06: {traces} real Reset/Stop/disconnect/reboot traces");
}

#[test]
fn c10_lifetime_allowance_interruptions_duplicates_and_lost_ack() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 42);
    let mut erases = 0;
    let mut cuts = 0;
    for attempt in 1..=400 {
        let mut s = boot(&mut m, g);
        let k = key(1); // identically restarted MCU counters
        let b = capture(attempt);
        let snap = Snapshot::new(&Thermostat, b.bytes(), 11, attempt as u32, k).unwrap();
        assert_eq!(s.save(k, snap, 100, || 0), SaveResponse::Accepted);
        let mut power_cut = false;
        for _ in 0..100 {
            if s.ownership_settled() {
                break;
            }
            let c = s.take_command(0).unwrap().unwrap();
            if matches!(c.header.kind, CommandKind::ConsumeErasePermit { .. }) {
                let r = m.execute(c.clone());
                for _ in 0..100 {
                    assert_eq!(m.execute(c.clone()), r);
                }
                s.complete(r).unwrap();
            } else if matches!(c.header.kind, CommandKind::Erase { .. }) {
                erases += 1;
                if erases % 3 == 0 {
                    assert!(m.execute_erase_prefix(&c, 512));
                    assert_eq!(
                        m.execute(c).status,
                        CompletionStatus::Failed { quiescent: true }
                    );
                    cuts += 1;
                    power_cut = true;
                    break;
                } else {
                    let r = m.execute(c.clone());
                    assert_eq!(
                        m.execute(c).status,
                        CompletionStatus::Failed { quiescent: true }
                    );
                    s.complete(r).unwrap();
                }
            } else {
                s.complete(m.execute(c)).unwrap();
            }
        }
        assert!(m.erase_commands() <= 200);
        if !power_cut
            && matches!(
                s.retained(),
                Some(Outcome::Failed {
                    reason: FailureKind::WearUnavailable,
                    ..
                })
            )
        {
            break;
        }
    }
    assert!(cuts > 0);
    assert!(erases <= 200);
    assert_eq!(erases, m.erase_commands());
    assert_eq!(
        m.wear_remaining(Slot::A) + m.wear_remaining(Slot::B),
        200 - erases as u16
    );
    // Losing an acquisition response also spends budget and invalidates the lease.
    let mut lost = Simulator::new(g, 2, 43);
    for _ in 0..2 {
        let mut s = boot(&mut lost, g);
        s.save(key(1), capture(1), 100, || 0);
        until(&mut s, &mut lost, ServiceState::AcquireWear);
        let c = s.take_command(0).unwrap().unwrap();
        assert!(matches!(
            lost.execute(c).status,
            CompletionStatus::WearGranted { .. }
        ));
    }
    let mut s = boot(&mut lost, g);
    s.save(key(1), capture(1), 100, || 0);
    drive(&mut s, &mut lost);
    assert_eq!(lost.erase_commands(), 0);
    assert_eq!(lost.wear_remaining(Slot::A), 0);
    lost.lose_authority();
    assert!(lost.open_session().is_none());
    println!(
        "C10: {erases} charged erases including {cuts} interrupted erases; 100 acquisition replays per attempt; lost ack consumption"
    );
}

#[test]
fn c13_thousand_deterministic_capacity_one_pump_permutations() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut total_commands = 0;
    for seed in 0..1000u64 {
        let mut m = Simulator::new(g, 100, seed);
        let mut s = boot(&mut m, g);
        s.save(key(1), capture(1), 100, || 0);
        let (mut command, mut completion) = (None, None);
        let mut trace = TraceRing::default();
        let mut rng = seed + 17;
        let mut executed = 0;
        for tick in 0..10000 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let action = if tick % 16 == 15 {
                3
            } else {
                ((rng >> 32) % 4) as u8
            };
            if (action == 0 || action == 3) && command.is_none() && completion.is_none() {
                command = s.take_command(0).unwrap();
                if let Some(c) = &command {
                    trace.record(c.header, 1);
                }
            }
            if (action == 1 || action == 3)
                && completion.is_none()
                && let Some(c) = command.take()
            {
                trace.record(c.header, 2);
                completion = Some(m.execute(c));
                executed += 1;
            }
            if (action == 2 || action == 3)
                && let Some(r) = completion.take()
            {
                trace.record(r.header, 3);
                s.complete(r).unwrap();
            }
            if command.is_some() || completion.is_some() {
                assert!(s.take_command(0).unwrap().is_none());
            }
            assert_eq!(
                s.resolve_or_cancel(key(1)),
                if let Some(out) = s.retained() {
                    Resolve::Terminal(out)
                } else {
                    Resolve::Pending
                }
            );
            if s.ownership_settled() {
                break;
            }
        }
        assert_eq!(executed, 99);
        assert!(command.is_none() && completion.is_none());
        assert!(trace.entries().iter().any(|t| t.event == 3));
        assert_eq!(selected(&m, g), Some(identity(Record::new(1, capture(1)))));
        total_commands += executed;
    }
    assert_eq!(total_commands, 99000);
    println!("C13: 1000 capacity-one scheduler permutations / {total_commands} real commands");
}

#[test]
fn c09_thirty_two_lifecycle_key_combinations() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut count = 0;
    for mode in 0..4 {
        for kind in 0..8 {
            let mut m = Simulator::new(g, 100, 1);
            let mut s = boot(&mut m, g);
            match mode {
                0 => {}
                1 => s.stop(),
                2 => {
                    s.save(key(1), capture(1), 100, || 0);
                }
                _ => {
                    s.resolve_or_cancel(key(1));
                }
            }
            let k = match kind {
                0 => key(1),
                1 => key(2),
                2 => key(0),
                3 => OperationKey {
                    service_epoch: 16,
                    ..key(1)
                },
                4 => OperationKey {
                    session_generation: 2,
                    ..key(1)
                },
                5 => OperationKey {
                    service_epoch: 18,
                    ..key(1)
                },
                6 => OperationKey {
                    session_generation: 4,
                    ..key(1)
                },
                _ => key(u64::MAX),
            };
            let cap = Snapshot::new(&Thermostat, capture(1).bytes(), 11, 1, k).unwrap();
            let before = m.erase_commands();
            let result = s.save(k, cap, 10, || 10);
            if (3..=6).contains(&kind) {
                assert!(matches!(result, SaveResponse::Stale { .. }));
            } else if k == key(1) && mode == 2 {
                assert_eq!(result, SaveResponse::Pending);
            } else if k == key(1) && mode == 3 {
                assert_eq!(result, SaveResponse::KeyConflict);
            } else if mode >= 2 {
                assert_eq!(result, SaveResponse::Busy);
            } else {
                assert!(matches!(
                    result,
                    SaveResponse::Terminal(Outcome::Rejected { .. })
                ));
            }
            assert_eq!(m.erase_commands(), before);
            count += 1;
        }
    }
    assert_eq!(count, 32);
    println!("C09: {count} lifecycle/key combinations");
}

#[test]
fn c18_maximum_geometry_real_slots_trace_and_drop_budget() {
    use std::mem::{needs_drop, size_of};
    let total = size_of::<PersistenceService<Thermostat>>()
        + size_of::<bloxide_persistence_calibration::Broker<4>>()
        + size_of::<Option<BackendCommand>>()
        + size_of::<Option<Completion>>()
        + size_of::<Option<QuiesceRequest>>()
        + size_of::<TraceRing>()
        + BODY_BYTES;
    println!(
        "C18 host x86_64 core+broker4+owned command/completion/control slots+trace32+decoded scratch320={total} bytes"
    );
    assert!(total <= 8192);
    assert!(!needs_drop::<PersistenceService<Thermostat>>());
    assert!(!needs_drop::<TraceRing>());
    let g = Geometry::new(65536, 256).unwrap();
    let mut m = Simulator::new(g, 100, 1);
    let mut s = boot(&mut m, g);
    save(&mut s, &mut m, 1);
    assert_eq!(selected(&m, g), Some(identity(Record::new(1, capture(1)))));
}

#[derive(Clone, Copy)]
struct Maximum;
impl Schema for Maximum {
    fn id(&self) -> u16 {
        65535
    }
    fn payload_len(&self) -> u16 {
        256
    }
    fn defaults(&self, b: &mut [u8; 256]) -> Result<(), SchemaError> {
        b.fill(0);
        Ok(())
    }
    fn validate(&self, b: &[u8]) -> Result<(), SchemaError> {
        if b.len() == 256 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}
#[test]
fn c18_maximum_payload_save_streams_65536_byte_units() {
    let g = Geometry::new(65536, 256).unwrap();
    let mut m = Simulator::new(g, 100, 8);
    let r = recover(Classification::Empty, Classification::Empty, &Maximum).unwrap();
    let mut s =
        PersistenceService::new(g, Maximum, r, true, 17, 3, 23, 0, m.open_session().unwrap());
    let payload: [u8; 256] = core::array::from_fn(|i| i as u8);
    let snapshot = Snapshot::new(&Maximum, &payload, 22, u32::MAX, key(1)).unwrap();
    assert_eq!(s.save(key(1), snapshot, 100, || 0), SaveResponse::Accepted);
    let mut count = 0;
    while !s.ownership_settled() {
        let c = s.take_command(0).unwrap().unwrap();
        s.complete(m.execute(c)).unwrap();
        count += 1;
        assert!(count < 2000);
    }
    assert_eq!(count, g.normal_save_commands());
    assert!(matches!(s.retained(),Some(Outcome::Durable{record,..}) if record.snapshot==snapshot));
    let raw = m.slot(Slot::A);
    for (i, &byte) in payload.iter().enumerate() {
        assert_eq!(&raw[128 + 2 * i..130 + 2 * i], &[byte, !byte]);
    }
    assert!(raw[1024..].iter().all(|&x| x == 255));
}
