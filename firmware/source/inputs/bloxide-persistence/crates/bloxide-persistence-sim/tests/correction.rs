mod common;
use bloxide_persistence::*;
use bloxide_persistence_sim::Simulator;
use common::*;

#[test]
fn f1_live_alternation_same_value_resume_and_full_persistent_identity() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 7);
    let mut s = boot(&mut m, g);
    for n in 1..=8 {
        save(&mut s, &mut m, n);
        assert_eq!(
            identity(s.read_durable().unwrap()),
            selected(&m, g).unwrap()
        );
    }
    let r = s.read_durable().unwrap();
    for n in 9..=1008 {
        let snap = Snapshot::new(
            &Thermostat,
            r.snapshot.bytes(),
            r.snapshot.source_epoch(),
            r.snapshot.source_revision(),
            key(n),
        )
        .unwrap();
        assert!(matches!(
            s.save(key(n), snap, 100, || 0),
            SaveResponse::Terminal(Outcome::Durable { existing: true, .. })
        ));
        assert!(s.take_command(0).unwrap().is_none());
        s.release(key(n)).unwrap();
    }
    assert_eq!(m.erase_commands(), 8);
    s.stop();
    s.resume(18, 4, 24).unwrap();
    let k = OperationKey {
        service_epoch: 18,
        session_generation: 4,
        sequence: 1,
    };
    let snap = Snapshot::new(&Thermostat, capture(10).bytes(), 12, 1, k).unwrap();
    assert_eq!(s.save(k, snap, 100, || 0), SaveResponse::Accepted);
    drive(&mut s, &mut m);
    assert!(matches!(s.retained(),Some(Outcome::Durable{key,..}) if key==k));
    let mut other = r;
    other.snapshot = Snapshot::new(
        &Thermostat,
        r.snapshot.bytes(),
        r.snapshot.source_epoch(),
        r.snapshot.source_revision(),
        OperationKey {
            service_epoch: 99,
            ..r.snapshot.operation()
        },
    )
    .unwrap();
    assert!(r.persistent_identity_eq(&other));
    for field in 0..7 {
        let mut other = r;
        let mut k = r.snapshot.operation();
        let mut epoch = r.snapshot.source_epoch();
        let mut rev = r.snapshot.source_revision();
        let mut b = r.snapshot.bytes().to_vec();
        match field {
            0 => other.sequence += 1,
            1 => other.format += 1,
            2 => epoch += 1,
            3 => rev += 1,
            4 => k.sequence += 1,
            5 => k.session_generation += 1,
            _ => b[0] ^= 1,
        }
        other.snapshot = Snapshot::new(&Thermostat, &b, epoch, rev, k).unwrap();
        assert!(!r.persistent_identity_eq(&other));
    }
    println!("C17: 1000 same-snapshot requests; F1: 8 live alternating saves + resumed save");
}

#[test]
fn f6_release_fences_do_not_clear_newer_result_or_pending() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 7);
    let mut s = boot(&mut m, g);
    save(&mut s, &mut m, 1);
    assert_eq!(s.release(key(1)), Ok(()));
    s.save(key(2), capture(2), 100, || 0);
    assert!(s.release(key(2)).is_err());
    assert_eq!(s.release(key(1)), Ok(()));
    drive(&mut s, &mut m);
    let result = s.retained();
    assert_eq!(s.release(key(1)), Ok(()));
    assert_eq!(s.retained(), result);
    assert!(s.release(key(3)).is_err());
    assert!(
        s.release(OperationKey {
            service_epoch: 18,
            ..key(1)
        })
        .is_err()
    );
    assert_eq!(s.retained(), result);
}

#[derive(Clone, Copy)]
struct Registration {
    id: u16,
    len: u16,
    strict: bool,
}
impl Schema for Registration {
    fn id(&self) -> u16 {
        self.id
    }
    fn payload_len(&self) -> u16 {
        self.len
    }
    fn defaults(&self, b: &mut [u8; 256]) -> Result<(), SchemaError> {
        b.fill(0);
        Ok(())
    }
    fn validate(&self, b: &[u8]) -> Result<(), SchemaError> {
        if self.strict && b.first() != Some(&0) {
            Err(SchemaError::InvalidValue)
        } else {
            Ok(())
        }
    }
}
#[test]
fn f5_f9_registration_bounds_semantics_and_fresh_admission_clock() {
    let g = Geometry::new(1024, 8).unwrap();
    for (id, len) in [(0, 1), (1, 0), (1, 257), (1, 65535)] {
        let r = Registration {
            id,
            len,
            strict: false,
        };
        assert_eq!(
            recover(Classification::Empty, Classification::Empty, &r),
            Err(RecoveryError::InvalidDefaults)
        );
    }
    for foreign in [
        Registration {
            id: 99,
            len: 8,
            strict: false,
        },
        Registration {
            id: 2,
            len: 4,
            strict: false,
        },
        Registration {
            id: 2,
            len: 8,
            strict: false,
        },
    ] {
        let mut m = Simulator::new(g, 100, 7);
        let mut s = boot(&mut m, g);
        let b = [0u8; 8];
        let snap = Snapshot::new(&foreign, &b[..foreign.len as usize], 0, 0, key(1)).unwrap();
        assert!(matches!(
            s.save(key(1), snap, 100, || 0),
            SaveResponse::Terminal(Outcome::Rejected {
                reason: RejectReason::InvalidSnapshot,
                ..
            })
        ));
        assert_eq!(m.erase_commands(), 0);
        assert_eq!(m.wear_remaining(Slot::A), 100);
    }
    let mut m = Simulator::new(g, 100, 7);
    let mut s = boot(&mut m, g);
    let mut now = 8;
    assert!(matches!(
        s.save(key(1), capture(1), 10, || {
            now += 1;
            now
        }),
        SaveResponse::Terminal(Outcome::Rejected {
            reason: RejectReason::Expired,
            ..
        })
    ));
    assert!(s.take_command(0).unwrap().is_none());
}

#[test]
fn c11_all_command_geometry_failures_timeouts_quiesce_and_late_completion() {
    let mut injected = 0;
    let mut reads = 0;
    for (e, g) in GEOMETRIES {
        let geo = Geometry::new(e, g).unwrap();
        for index in 0..geo.normal_save_commands() {
            // settled error, nonquiescent+late, timeout+idle, timeout+failed-quiesce+late,
            // timeout+silent-quiesce+late+late-idle. No operation is retried.
            for variant in 0..6 {
                let mut m = Simulator::new(geo, 100, 7);
                let mut s = boot(&mut m, geo);
                s.save(key(1), capture(1), 100, || 0);
                for _ in 0..index {
                    let c = s.take_command(0).unwrap().unwrap();
                    s.complete(m.execute(c)).unwrap();
                }
                let c = s.take_command(0).unwrap().unwrap();
                let exact = if variant == 5 {
                    Completion {
                        header: c.header,
                        status: CompletionStatus::Failed { quiescent: true },
                        data: [0; 256],
                    }
                } else {
                    m.execute(c.clone())
                };
                if variant == 0 {
                    let mut r = exact.clone();
                    r.status = CompletionStatus::Failed { quiescent: true };
                    s.complete(r).unwrap();
                } else {
                    if variant == 1 {
                        let mut r = exact.clone();
                        r.status = CompletionStatus::Failed { quiescent: false };
                        s.complete(r).unwrap();
                    } else {
                        s.advance_time(1000);
                    }
                    s.reset();
                    s.stop();
                    assert!(!s.ownership_settled());
                    assert!(s.release(key(1)).is_err());
                    assert!(s.take_command(1000).unwrap().is_none());
                    if variant == 1 {
                        s.complete(exact).unwrap();
                    } else {
                        let q = s.take_quiesce_request(1000).unwrap();
                        assert!(s.take_quiesce_request(1001).is_none());
                        if variant == 2 || variant == 5 {
                            s.complete_quiesce(q, true).unwrap();
                        } else if variant == 3 {
                            s.complete_quiesce(q, false).unwrap();
                            assert!(s.release(key(1)).is_err());
                            s.complete(exact).unwrap();
                        } else {
                            s.advance_time(2000);
                            assert!(s.release(key(1)).is_err());
                            assert!(s.take_quiesce_request(2000).is_none());
                            s.complete(exact).unwrap();
                            assert!(s.release(key(1)).is_err());
                            s.complete_quiesce(q, true).unwrap();
                        }
                    }
                }
                let destructive = (m.erase_commands(), m.program_commands());
                for _ in 0..=e.div_ceil(256) {
                    if s.ownership_settled() {
                        break;
                    }
                    let next = s
                        .take_command(3000)
                        .unwrap()
                        .expect("finite read reconciliation");
                    assert_eq!(next.header.kind, CommandKind::Read);
                    s.complete(m.execute(next)).unwrap();
                }
                assert!(s.ownership_settled());
                assert_eq!(destructive, (m.erase_commands(), m.program_commands()));
                let out = s.retained().unwrap();
                if matches!(
                    out,
                    Outcome::Failed {
                        no_new_commit: true,
                        ..
                    }
                ) {
                    assert!(selected(&m, geo).is_none());
                }
                if let Outcome::Durable { record, .. } = out {
                    assert_eq!(selected(&m, geo), Some(identity(record)));
                }
                for _ in 0..3 {
                    assert_eq!(
                        s.save(key(1), capture(1), 100, || 0),
                        SaveResponse::Terminal(out)
                    );
                    assert_eq!(s.resolve(key(1)), Resolve::Terminal(out));
                }
                s.release(key(1)).unwrap();
                assert!(s.health().write_locked);
                assert!(s.resume(18, 4, 24).is_err());
                injected += 1;
            }
            // Every physical read position: all three explicit read issues.
            for issue in [
                ReadIssue::Io,
                ReadIssue::CorrectedEcc,
                ReadIssue::UncorrectableEcc,
            ] {
                let mut m = Simulator::new(geo, 100, 7);
                let mut s = boot(&mut m, geo);
                s.save(key(1), capture(1), 100, || 0);
                for _ in 0..index {
                    let c = s.take_command(0).unwrap().unwrap();
                    s.complete(m.execute(c)).unwrap();
                }
                let c = s.take_command(0).unwrap().unwrap();
                if c.header.kind != CommandKind::Read {
                    continue;
                }
                let mut r = m.execute(c);
                r.status = CompletionStatus::Read {
                    len: r.header.len,
                    issue: Some(issue),
                };
                s.complete(r).unwrap();
                if selected(&m, geo).is_some() {
                    assert!(matches!(s.retained(), Some(Outcome::Indeterminate { .. })));
                } else {
                    assert!(matches!(
                        s.retained(),
                        Some(Outcome::Failed {
                            no_new_commit: true,
                            ..
                        })
                    ));
                }
                assert!(s.take_command(0).unwrap().is_none());
                reads += 1;
            }
        }
    }
    assert_eq!(reads, 528);
    println!(
        "C11: {injected} every-command/geometry error+timeout traces; {reads} read/ECC injections"
    );
}

#[test]
fn f3_no_reconciliation_restart_and_f7_no_destructive_progress() {
    let g = Geometry::new(1024, 8).unwrap();
    for position in 0..99 {
        for bad in 0..4 {
            let mut m = Simulator::new(g, 100, 7);
            let mut s = boot(&mut m, g);
            s.save(key(1), capture(1), 100, || 0);
            for _ in 0..position {
                let c = s.take_command(0).unwrap().unwrap();
                s.complete(m.execute(c)).unwrap();
            }
            let c = s.take_command(0).unwrap().unwrap();
            let r = m.execute(c);
            let mut wrong = r.clone();
            match bad {
                0 => wrong.header.io_epoch += 1,
                1 => wrong.header.slot = wrong.header.slot.other(),
                2 => wrong.header.command_sequence += 1,
                _ => wrong.header.len += 1,
            }
            assert!(s.complete(wrong).is_err());
            assert!(!s.ownership_settled());
            s.complete(r).unwrap();
            let before = (m.erase_commands(), m.program_commands());
            drive(&mut s, &mut m);
            assert_eq!(before, (m.erase_commands(), m.program_commands()));
            assert!(s.health().write_locked);
        }
    }
    for fail_offset in 0..4 {
        let mut m = Simulator::new(g, 100, 7);
        let mut s = boot(&mut m, g);
        s.save(key(1), capture(1), 100, || 0);
        until(&mut s, &mut m, ServiceState::VerifyCommitted);
        for _ in 0..fail_offset {
            let c = s.take_command(0).unwrap().unwrap();
            s.complete(m.execute(c)).unwrap();
        }
        let c = s.take_command(0).unwrap().unwrap();
        let mut r = m.execute(c);
        r.status = CompletionStatus::Failed { quiescent: true };
        s.complete(r).unwrap();
        for _ in 0..20 {
            assert!(s.take_command(0).unwrap().is_none());
        }
        assert!(matches!(s.retained(), Some(Outcome::Indeterminate { .. })));
    }
}

#[test]
fn f4_silent_engine_never_releases_or_acknowledges_drain() {
    for (e, g) in GEOMETRIES {
        let geo = Geometry::new(e, g).unwrap();
        for phase in [
            ServiceState::QualifyErase,
            ServiceState::AcquireWear,
            ServiceState::Erasing,
            ServiceState::VerifyErased,
            ServiceState::ProgramBody,
            ServiceState::VerifyBody,
            ServiceState::ProgramCommit,
            ServiceState::VerifyCommitted,
        ] {
            let mut m = Simulator::new(geo, 100, 1);
            let mut s = boot(&mut m, geo);
            s.save(key(1), capture(1), 100, || 0);
            until(&mut s, &mut m, phase);
            let c = s.take_command(0).unwrap().unwrap();
            s.advance_time(1000);
            let q = s.take_quiesce_request(1000).unwrap();
            for now in 2000..2020 {
                s.advance_time(now);
                s.reset();
                s.stop();
                assert!(s.take_quiesce_request(now).is_none());
                assert!(s.take_command(now).unwrap().is_none());
                assert!(!s.ownership_settled());
                assert!(s.release(key(1)).is_err());
                assert!(s.resume(18, 4, 24).is_err());
            }
            assert!(matches!(
                s.resolve(key(1)),
                Resolve::Terminal(Outcome::Indeterminate { .. })
            ));
            s.complete_quiesce(q, false).unwrap();
            assert!(s.release(key(1)).is_err());
            s.complete(m.execute(c)).unwrap();
            for _ in 0..=e.div_ceil(256) {
                if s.ownership_settled() {
                    break;
                }
                let c = s.take_command(3000).unwrap().unwrap();
                assert_eq!(c.header.kind, CommandKind::Read);
                s.complete(m.execute(c)).unwrap();
            }
            assert!(s.ownership_settled());
            s.release(key(1)).unwrap();
            assert_eq!(s.mode(), ServiceMode::ReadOnlyFault);
        }
    }
}

#[test]
fn c05_c17_sequence_exhaustion_and_capture_identity_are_service_obligations() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 1);
    let old = Record::new(u64::MAX, capture(1));
    let r = recover(
        Classification::Valid(old),
        Classification::Empty,
        &Thermostat,
    )
    .unwrap();
    let mut s = PersistenceService::new(
        g,
        Thermostat,
        r,
        true,
        17,
        3,
        23,
        0,
        m.open_session().unwrap(),
    );
    for n in 2..=5 {
        let a = old.snapshot;
        let bytes = if n == 2 {
            capture(2).bytes().to_vec()
        } else {
            a.bytes().to_vec()
        };
        let cap = Snapshot::new(
            &Thermostat,
            &bytes,
            a.source_epoch() + u64::from(n == 3),
            a.source_revision() + u32::from(n >= 4),
            key(n),
        )
        .unwrap();
        assert!(matches!(
            s.save(key(n), cap, 100, || 0),
            SaveResponse::Terminal(Outcome::Rejected {
                reason: RejectReason::SequenceExhausted,
                ..
            })
        ));
        assert!(s.take_command(0).unwrap().is_none());
        s.release(key(n)).unwrap();
    }
    let a = old.snapshot;
    let cap = Snapshot::new(
        &Thermostat,
        a.bytes(),
        a.source_epoch(),
        a.source_revision(),
        key(6),
    )
    .unwrap();
    assert!(
        matches!(s.save(key(6),cap,100,||0),SaveResponse::Terminal(Outcome::Durable{existing:true,record,..}) if record.sequence==u64::MAX)
    );
    assert_eq!(m.erase_commands(), 0);
    assert_eq!(m.wear_remaining(Slot::A), 100);
}
