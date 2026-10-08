use bloxide_calibration::{
    ApplyField, ApplyRequest, CommitGate, CommitRecord, EncodedValue, GateError,
    OperationKey as CK, Owner, OwnerOutcome,
};
use bloxide_persistence::*;
use bloxide_persistence_calibration::{Broker, CanonicalCalibration, CaptureError, capture_owner};
use bloxide_persistence_sim::Simulator;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Led {
    period: u16,
    duty: u16,
}
impl CanonicalCalibration for Led {
    fn encode(self, b: &mut [u8; 256]) -> u16 {
        b[..2].copy_from_slice(&self.period.to_le_bytes());
        b[2..4].copy_from_slice(&self.duty.to_le_bytes());
        4
    }
}
#[derive(Clone, Copy)]
struct LedSchema;
impl Schema for LedSchema {
    fn id(&self) -> u16 {
        1
    }
    fn payload_len(&self) -> u16 {
        4
    }
    fn defaults(&self, b: &mut [u8; 256]) -> Result<(), SchemaError> {
        Led {
            period: 1000,
            duty: 500,
        }
        .encode(b);
        Ok(())
    }
    fn validate(&self, b: &[u8]) -> Result<(), SchemaError> {
        if b.len() == 4
            && (100..=10000).contains(&u16::from_le_bytes(b[..2].try_into().unwrap()))
            && u16::from_le_bytes(b[2..].try_into().unwrap()) <= 1000
        {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}
struct Gate {
    published: u64,
}
impl CommitGate<Led> for Gate {
    type Permit = ();
    fn try_reserve(&mut self, _: &Led) -> Result<(), GateError> {
        Ok(())
    }
    fn publish(&mut self, _: (), _: CommitRecord<Led>) {
        self.published += 1;
    }
}
fn pk(k: CK) -> OperationKey {
    OperationKey {
        service_epoch: k.service_epoch,
        session_generation: k.session_generation,
        sequence: k.sequence,
    }
}
fn apply(owner: &mut Owner<Led>, gate: &mut Gate, n: u64, period: u16) {
    let key = CK {
        service_epoch: 7,
        session_generation: 3,
        sequence: n,
    };
    let req = ApplyRequest {
        field: ApplyField {
            key,
            field_id: 1,
            encoded_value: EncodedValue::try_from_slice(&period.to_le_bytes()).unwrap(),
            expires_at_us: 100,
        },
        expected_revision: None,
    };
    let r = owner.apply(
        req,
        || 0,
        |v, _, b| {
            Ok(Led {
                period: u16::from_le_bytes(b.as_slice().try_into().unwrap()),
                ..v
            })
        },
        gate,
    );
    assert!(matches!(r, OwnerOutcome::Applied { changed: true, .. }));
    owner.release(key).unwrap();
}
fn boot(m: &mut Simulator, g: Geometry) -> PersistenceService<LedSchema> {
    let c = |slot| {
        classify_slot(
            SlotRead {
                bytes: m.slot(slot),
                issue: None,
            },
            g,
            &LedSchema,
        )
    };
    let r = recover(c(Slot::A), c(Slot::B), &LedSchema).unwrap();
    let config = m.open_session().unwrap();
    PersistenceService::new(g, LedSchema, r, true, 90, 11, 1, 0, config)
}
fn drive(s: &mut PersistenceService<LedSchema>, m: &mut Simulator) {
    for _ in 0..2000 {
        if s.ownership_settled() {
            return;
        }
        let c = s.take_command(0).unwrap().unwrap();
        s.complete(m.execute(c)).unwrap();
    }
    panic!("unbounded")
}

#[test]
fn c07_thousand_real_owner_edits_across_six_storage_phases() {
    let phases = [
        ServiceState::QualifyErase,
        ServiceState::Erasing,
        ServiceState::VerifyErased,
        ServiceState::ProgramBody,
        ServiceState::ProgramCommit,
        ServiceState::VerifyCommitted,
    ];
    let g = Geometry::new(1024, 8).unwrap();
    let mut owner = Owner::new(
        Led {
            period: 1000,
            duty: 500,
        },
        7,
        3,
    );
    let mut gate = Gate { published: 0 };
    let mut broker = Broker::<1>::new(90, 11);
    let mut m = Simulator::new(g, 1000, 1);
    let mut s = boot(&mut m, g);
    for n in 0..1000 {
        let key = broker.reserve(0).unwrap();
        let capture = capture_owner(&owner, &LedSchema, key, Some((7, n))).unwrap();
        let saved = owner.active();
        assert_eq!(s.save(pk(key), capture, 100, || 0), SaveResponse::Accepted);
        while s.state() != phases[n as usize % 6] {
            let c = s.take_command(0).unwrap().unwrap();
            s.complete(m.execute(c)).unwrap();
        }
        apply(
            &mut owner,
            &mut gate,
            u64::from(n) + 1,
            if n % 2 == 0 { 2000 } else { 1000 },
        );
        drive(&mut s, &mut m);
        let out = s.retained().unwrap();
        assert!(
            matches!(out,Outcome::Durable{key:k,record,..} if k==pk(key)&&record.snapshot==capture)
        );
        assert_ne!(owner.active(), saved);
        assert_eq!(owner.active_revision().get(), n + 1);
        assert_eq!(capture.source_revision(), n);
        broker.record_terminal(0, out).unwrap();
        s.release(pk(key)).unwrap();
        assert_eq!(broker.release(0).unwrap(), key);
    }
    assert_eq!(gate.published, 1000);
    assert_eq!(m.erase_commands(), 1000);
    println!("C07: 1000 actual Owner apply/capture/service traces over six storage phases");
}

fn clients<const N: usize>(seed: u64) -> (usize, usize, usize) {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 10000, seed);
    let mut s = boot(&mut m, g);
    let mut owner = Owner::new(
        Led {
            period: 1000,
            duty: 500,
        },
        7,
        3,
    );
    let mut gate = Gate { published: 0 };
    let mut broker = Broker::<N>::new(90, 11);
    let mut keys = [None; N];
    let mut captures = [None; N];
    let mut disconnected = [false; N];
    let mut rng = seed;
    let (mut completed, mut busy, mut disconnects) = (0, 0, 0);
    let mut edit = 0;
    let mut issued = 0;
    let mut command = None;
    let mut completion = None;
    let mut ticks = 0;
    while completed < 5000 {
        ticks += 1;
        assert!(ticks < 5_000_000, "bounded scheduler stalled");
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let choice = (rng >> 32) as usize;
        // Reserve/capture all available clients before progressing backend; other
        // requests observe Busy while the single save or retained result is held.
        for client in 0..N {
            if keys[client].is_none() && issued < 5000 {
                edit += 1;
                apply(
                    &mut owner,
                    &mut gate,
                    edit,
                    if edit % 2 == 1 { 2000 } else { 1000 },
                );
                let k = broker.reserve(client).unwrap();
                keys[client] = Some(k);
                captures[client] = Some(capture_owner(&owner, &LedSchema, k, None).unwrap());
                issued += 1;
            }
        }
        // Submit in globally increasing sequence order so a fenced watermark never
        // consumes an unsubmitted earlier request. Repeat/Busy are observations only.
        let candidate = (0..N)
            .filter(|&i| keys[i].is_some() && broker.result(i).is_none())
            .min_by_key(|&i| keys[i].unwrap().sequence);
        if let Some(client) = candidate {
            let k = keys[client].unwrap();
            let capture = captures[client].unwrap();
            match s.save(pk(k), capture, 100, || 0) {
                SaveResponse::Accepted | SaveResponse::Pending => {}
                SaveResponse::Busy => busy += 1,
                SaveResponse::Terminal(out) => {
                    assert!(
                        matches!(out, Outcome::Durable { record, .. } if record.snapshot == capture)
                    );
                    assert_eq!(out.key(), pk(k));
                    broker.record_terminal(client, out).unwrap();
                    s.release(pk(k)).unwrap();
                }
                x => panic!("unexpected request response {x:?}"),
            }
            for other in 0..N {
                if (!s.ownership_settled() || s.retained().is_some())
                    && other != client
                    && broker.result(other).is_none()
                    && let Some(k) = keys[other]
                {
                    assert_eq!(
                        s.save(pk(k), captures[other].unwrap(), 100, || 0),
                        SaveResponse::Busy
                    );
                    busy += 1;
                }
            }
        }
        let client = choice % N;
        if keys[client].is_some() && choice.is_multiple_of(11) {
            disconnected[client] = true;
            disconnects += 1;
            assert!(broker.reserve(client).is_err());
        }
        if command.is_none() && completion.is_none() {
            command = s.take_command(0).unwrap();
        }
        if !choice.is_multiple_of(3)
            && let Some(c) = command.take()
        {
            assert!(completion.is_none());
            completion = Some(m.execute(c));
        }
        if !choice.is_multiple_of(5)
            && let Some(r) = completion.take()
        {
            s.complete(r).unwrap();
        }
        // Held results survive disconnect and arbitrary delays. Reconciliation is
        // mandatory before releasing a disconnected client's slot for reuse.
        for i in 0..N {
            if let Some(out) = broker.result(i) {
                assert_eq!(out.key(), pk(keys[i].unwrap()));
                assert!(broker.reserve(i).is_err());
                if choice.is_multiple_of(7) || completed + N >= 5000 {
                    let k = broker.release(i).unwrap();
                    assert_eq!(k, keys[i].unwrap());
                    keys[i] = None;
                    captures[i] = None;
                    disconnected[i] = false;
                    completed += 1;
                }
            }
        }
    }
    assert!(command.is_none() && completion.is_none());
    assert!(keys.iter().all(Option::is_none));
    assert!(s.ownership_settled());
    assert!(m.erase_commands() > 0);
    assert!(disconnects > 0);
    (completed, busy, disconnects)
}
#[test]
fn c08_ten_thousand_meaningful_interleaved_outcomes() {
    let a = clients::<1>(7);
    let b = clients::<4>(17);
    assert_eq!(a.0 + b.0, 10000);
    assert!(b.1 > 0);
    println!("C08: 10000 real service/Owner/broker outcomes; one-client={a:?}, four-client={b:?}");
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct BadLength(u16);
impl CanonicalCalibration for BadLength {
    fn encode(self, _: &mut [u8; 256]) -> u16 {
        self.0
    }
}
#[test]
fn f9_capture_rejects_untrusted_returned_lengths_without_indexing() {
    for len in [0, 257, 65535] {
        let owner = Owner::new(BadLength(len), 1, 1);
        assert_eq!(
            capture_owner(
                &owner,
                &LedSchema,
                CK {
                    service_epoch: 90,
                    session_generation: 11,
                    sequence: 1
                },
                None
            ),
            Err(CaptureError::InvalidSnapshot(
                SnapshotError::LengthOutOfRange
            ))
        );
    }
    let owner = Owner::new(BadLength(1), 1, 1);
    assert_eq!(
        capture_owner(
            &owner,
            &LedSchema,
            CK {
                service_epoch: 90,
                session_generation: 11,
                sequence: 1
            },
            None
        ),
        Err(CaptureError::InvalidSnapshot(SnapshotError::LengthMismatch))
    );
}

/// This is intentionally a release-mode qualification, using 2^32 real public
/// mutations instead of modifying Owner's private revision or substituting a fake.
#[test]
#[ignore = "run explicitly with --release --ignored; 4294967296 public Owner mutations"]
fn c07_actual_owner_revision_wrap_public_api() {
    let mut owner = Owner::new(
        Led {
            period: 1000,
            duty: 500,
        },
        7,
        3,
    );
    let mut gate = Gate { published: 0 };
    let mut at_max = None;
    let mut at_zero = None;
    for n in 1..=u64::from(u32::MAX) + 1 {
        apply(
            std::hint::black_box(&mut owner),
            &mut gate,
            n,
            if n % 2 == 1 { 2000 } else { 1000 },
        );
        if n == u64::from(u32::MAX) {
            at_max = Some(
                capture_owner(
                    &owner,
                    &LedSchema,
                    CK {
                        service_epoch: 90,
                        session_generation: 11,
                        sequence: 1,
                    },
                    Some((7, u32::MAX)),
                )
                .unwrap(),
            );
        }
        if n == u64::from(u32::MAX) + 1 {
            at_zero = Some(
                capture_owner(
                    &owner,
                    &LedSchema,
                    CK {
                        service_epoch: 90,
                        session_generation: 11,
                        sequence: 2,
                    },
                    Some((7, 0)),
                )
                .unwrap(),
            );
        }
    }
    assert_eq!(gate.published, 4294967296);
    let a = at_max.unwrap();
    let b = at_zero.unwrap();
    assert_eq!(a.source_revision(), u32::MAX);
    assert_eq!(b.source_revision(), 0);
    assert_ne!(a.bytes(), b.bytes());
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 1);
    let mut s = boot(&mut m, g);
    for capture in [a, b] {
        assert_eq!(
            s.save(capture.operation(), capture, 100, || 0),
            SaveResponse::Accepted
        );
        drive(&mut s, &mut m);
        assert!(
            matches!(s.retained(),Some(Outcome::Durable{record,..}) if record.snapshot==capture)
        );
        s.release(capture.operation()).unwrap();
    }
    assert_eq!(s.read_durable().unwrap().sequence, 2);
    assert_eq!(owner.active_revision().get(), 0);
    println!(
        "C07: 4294967296 real public Owner apply/release calls; MAX and zero captures both persisted"
    );
}

#[test]
fn c15_boot_installs_validated_values_once_at_revision_zero() {
    let g = Geometry::new(1024, 8).unwrap();
    let mut m = Simulator::new(g, 100, 1);
    let mut s = boot(&mut m, g);
    let mut owner = Owner::new(
        Led {
            period: 1000,
            duty: 500,
        },
        7,
        3,
    );
    let mut gate = Gate { published: 0 };
    apply(&mut owner, &mut gate, 1, 2000);
    let k = CK {
        service_epoch: 90,
        session_generation: 11,
        sequence: 1,
    };
    let capture = capture_owner(&owner, &LedSchema, k, Some((7, 1))).unwrap();
    s.save(pk(k), capture, 100, || 0);
    drive(&mut s, &mut m);
    let saved = s.read_durable().unwrap();
    let b = saved.snapshot.bytes();
    LedSchema.validate(b).unwrap();
    let installed = Owner::new(
        Led {
            period: u16::from_le_bytes(b[..2].try_into().unwrap()),
            duty: u16::from_le_bytes(b[2..].try_into().unwrap()),
        },
        1,
        1,
    );
    assert_eq!(installed.active(), owner.active());
    assert_eq!(installed.active_revision().get(), 0);
    assert_eq!(saved.snapshot.source_revision(), 1);
    s.release(pk(k)).unwrap();
    let k = CK { sequence: 2, ..k };
    let capture = capture_owner(&owner, &LedSchema, k, None).unwrap();
    // A new epoch label forces a write without mutating RAM values.
    let capture = Snapshot::new(&LedSchema, capture.bytes(), 8, 1, pk(k)).unwrap();
    s.save(pk(k), capture, 100, || 0);
    let c = s.take_command(0).unwrap().unwrap();
    let mut r = m.execute(c);
    r.status = CompletionStatus::Read {
        len: r.header.len,
        issue: Some(ReadIssue::Io),
    };
    s.complete(r).unwrap();
    assert_eq!(owner.active(), installed.active());
    assert_eq!(owner.active_revision().get(), 1);
}
