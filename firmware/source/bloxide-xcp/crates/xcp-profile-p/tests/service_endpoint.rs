use bloxide_calibration::{
    ApplyField, ApplyRequest, CommitGate, CommitRecord, EncodedValue, GateError,
    OperationKey as OwnerKey, Owner, OwnerOutcome, RejectReason as OwnerReject,
};
use bloxide_persistence::{
    Classification, CompletionStatus, Geometry, Outcome, PersistenceService, Schema, SchemaError,
    Slot, SlotRead, classify_slot, recover,
};
use bloxide_persistence_calibration::{Broker, CanonicalCalibration};
use bloxide_persistence_sim::Simulator;
use xcp_messages::Packet;
use xcp_profile_p::{Admission, Domain, DrainProof, ProfileP, Reply, ScalarError, VIEW_BASE};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Led {
    period: u16,
    duty: u16,
}
impl CanonicalCalibration for Led {
    fn encode(self, out: &mut [u8; 256]) -> u16 {
        out[..2].copy_from_slice(&self.period.to_le_bytes());
        out[2..4].copy_from_slice(&self.duty.to_le_bytes());
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
    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        Led {
            period: 1000,
            duty: 500,
        }
        .encode(out);
        Ok(())
    }
    fn validate(&self, b: &[u8]) -> Result<(), SchemaError> {
        if b.len() != 4 {
            return Err(SchemaError::InvalidValue);
        }
        let p = u16::from_le_bytes([b[0], b[1]]);
        let d = u16::from_le_bytes([b[2], b[3]]);
        if (100..=10_000).contains(&p) && d <= 1000 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}
struct Gate;
impl CommitGate<Led> for Gate {
    type Permit = ();
    fn try_reserve(&mut self, _: &Led) -> Result<(), GateError> {
        Ok(())
    }
    fn publish(&mut self, _: (), _: CommitRecord<Led>) {}
}
struct LedDomain {
    owner: Owner<Led>,
    seq: u64,
}
impl LedDomain {
    fn new() -> Self {
        Self {
            owner: Owner::new(
                Led {
                    period: 1000,
                    duty: 500,
                },
                7,
                3,
            ),
            seq: 1,
        }
    }
}
impl Domain<Led> for LedDomain {
    fn owner(&self) -> &Owner<Led> {
        &self.owner
    }
    fn read_scalar(&self, address: u32) -> Option<[u8; 2]> {
        match address {
            0x1000 => Some(self.owner.active().period.to_le_bytes()),
            0x1002 => Some(self.owner.active().duty.to_le_bytes()),
            _ => None,
        }
    }
    fn write_scalar(&mut self, address: u32, value: [u8; 2], now: u64) -> Result<(), ScalarError> {
        let field = match address {
            0x1000 => 1,
            0x1002 => 2,
            _ => return Err(ScalarError::Bounds),
        };
        let candidate = u16::from_le_bytes(value);
        if (field == 1 && !(100..=10_000).contains(&candidate)) || (field == 2 && candidate > 1000)
        {
            return Err(ScalarError::Bounds);
        }
        let key = OwnerKey {
            service_epoch: 7,
            session_generation: 3,
            sequence: self.seq,
        };
        self.seq += 1;
        let req = ApplyRequest {
            field: ApplyField {
                key,
                field_id: field,
                encoded_value: EncodedValue::try_from_slice(&value).unwrap(),
                expires_at_us: now + 100,
            },
            expected_revision: None,
        };
        let result = self.owner.apply(
            req,
            || now,
            |mut led, id, v| {
                let n = u16::from_le_bytes(v.as_slice().try_into().unwrap());
                match id {
                    1 => led.period = n,
                    2 => led.duty = n,
                    _ => return Err(OwnerReject::UnknownVariable),
                }
                Ok(led)
            },
            &mut Gate,
        );
        self.owner.release(key).unwrap();
        match result {
            OwnerOutcome::Applied { .. } => Ok(()),
            _ => Err(ScalarError::Policy),
        }
    }
}
fn packet(b: &[u8]) -> Packet {
    Packet::try_from_slice(b).unwrap()
}
fn bytes(r: Reply) -> Vec<u8> {
    match r {
        Reply::Packet(p) => p.as_slice().to_vec(),
        r => panic!("unexpected {r:?}"),
    }
}
fn send(
    p: &mut ProfileP,
    d: &mut LedDomain,
    s: &mut PersistenceService<LedSchema>,
    b: &mut Broker<4>,
    cmd: &[u8],
) -> Vec<u8> {
    bytes(p.handle(
        &packet(cmd),
        d,
        &LedSchema,
        s,
        b,
        Admission {
            disarmed: true,
            maintenance: true,
            schema_known: true,
        },
        0,
        || 0,
    ))
}
fn fixture() -> (
    ProfileP,
    LedDomain,
    PersistenceService<LedSchema>,
    Broker<4>,
    Simulator,
) {
    let g = Geometry::new(1024, 8).unwrap();
    let mut sim = Simulator::new(g, 100, 44);
    let recovery = recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap();
    let config = sim.open_session().unwrap();
    let service = PersistenceService::new(g, LedSchema, recovery, true, 1, 1, 1, 0, config);
    let mut identity = [0u8; 128];
    identity[..8].copy_from_slice(b"BLXPP001");
    identity[8..10].copy_from_slice(&1u16.to_le_bytes());
    identity[10..12].copy_from_slice(&640u16.to_le_bytes());
    identity[12..16].copy_from_slice(&0x5000_0001u32.to_le_bytes());
    for (i, byte) in identity[16..112].iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(17).wrapping_add(3);
    }
    identity[112..116].copy_from_slice(&0x10000u32.to_le_bytes());
    identity[116..120].copy_from_slice(&0x1000u32.to_le_bytes());
    identity[120..122].copy_from_slice(&4u16.to_le_bytes());
    identity[122..124].copy_from_slice(&1u16.to_le_bytes());
    identity[124..128].copy_from_slice(&[4, 1, 1, 0]);
    (
        ProfileP::new(identity, 1).unwrap(),
        LedDomain::new(),
        service,
        Broker::new(1, 1),
        sim,
    )
}
fn drive(s: &mut PersistenceService<LedSchema>, m: &mut Simulator) {
    for _ in 0..1000 {
        if s.ownership_settled() {
            return;
        }
        let command = s.take_command(0).unwrap().unwrap();
        s.complete(m.execute(command)).unwrap();
    }
    panic!("unbounded")
}
fn mta(addr: u32) -> [u8; 8] {
    let a = addr.to_le_bytes();
    [0xf6, 0, 0, 0, a[0], a[1], a[2], a[3]]
}
fn read_view(
    p: &mut ProfileP,
    d: &mut LedDomain,
    s: &mut PersistenceService<LedSchema>,
    b: &mut Broker<4>,
) -> [u8; 640] {
    assert_eq!(send(p, d, s, b, &mta(VIEW_BASE)), [0xff]);
    let mut out = [0; 640];
    let mut at = 0;
    while at < 640 {
        let n = (640 - at).min(7);
        let r = send(p, d, s, b, &[0xf5, n as u8]);
        assert_eq!(r.len(), n + 1);
        out[at..at + n].copy_from_slice(&r[1..]);
        at += n;
    }
    out
}
#[test]
fn literal_pag_vectors_precedence_and_s0_isolation() {
    let (mut p, mut d, mut s, mut b, _) = fixture();
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]),
        [0xff, 1, 0, 8, 8, 0, 1, 1]
    );
    for (cmd, reply) in [
        (&[0xe9][..], &[0xff, 1, 1][..]),
        (&[0xe8, 0, 0, 0, 0], &[0xff, 0, 0, 0, 0, 0x10, 0, 0]),
        (&[0xe8, 0, 0, 1, 0], &[0xff, 0, 0, 0, 4, 0, 0, 0]),
        (&[0xe8, 1, 0, 0, 0], &[0xff, 1, 0, 0, 0, 0]),
        (&[0xe7, 0, 0, 0], &[0xff, 0x3f, 0]),
        (&[0xea, 1, 0], &[0xff, 0, 0, 0]),
        (&[0xea, 2, 0], &[0xff, 0, 0, 0]),
        (&[0xeb, 1, 0, 0], &[0xff]),
        (&[0xeb, 2, 0, 0], &[0xff]),
        (&[0xeb, 3, 0, 0], &[0xff]),
        (&[0xe6, 1, 0], &[0xff]),
        (&[0xe5, 0, 0], &[0xff, 0, 1]),
        (&[0xe6, 0, 0], &[0xff]),
        (&[0xe5, 0, 0], &[0xff, 0, 0]),
    ] {
        assert_eq!(
            send(&mut p, &mut d, &mut s, &mut b, cmd),
            reply,
            "{cmd:02x?}"
        )
    }
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xfe, 0x27]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1]),
        [0xfe, 0x21]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 2, 0, 0]),
        [0xfe, 0x22]
    );
    assert_eq!(send(&mut p, &mut d, &mut s, &mut b, &[0xe4]), [0xfe, 0x20]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe8, 0, 1, 9, 0]),
        [0xfe, 0x28]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe8, 2, 0, 0, 0]),
        [0xfe, 0x22]
    );
}
#[test]
fn accepted_capture_is_immutable_and_durable_is_separate_from_later_ram() {
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    assert!(p.request_bit());
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xfd]),
        [0xff, 1, 0, 0, 0, 0]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xfe, 0x10]
    );
    send(&mut p, &mut d, &mut s, &mut b, &mta(0x1000));
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf0, 2, 0xd0, 0x07]),
        [0xff]
    );
    assert_eq!(d.owner.active().period, 2000);
    let pending = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(&pending[128..132], &[0xe8, 3, 0xf4, 1]);
    assert_eq!(&pending[384..388], &[0xff; 4]);
    assert_eq!(u32::from_le_bytes(pending[36..40].try_into().unwrap()), 1);
    drive(&mut s, &mut m);
    assert!(p.observe(0, &mut s, &mut b));
    assert!(!p.request_bit());
    let result = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(&result[128..132], &[0xe8, 3, 0xf4, 1]);
    assert_eq!(&result[384..388], &[0xe8, 3, 0xf4, 1]);
    assert_eq!(d.owner.active().period, 2000);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xfd]),
        [0xff, 0, 0, 0, 0, 0]
    );
    let g = Geometry::new(1024, 8).unwrap();
    assert!(
        matches!(classify_slot(SlotRead{bytes:m.slot(Slot::A),issue:None},g,&LedSchema),Classification::Valid(r) if r.snapshot.bytes()==[0xe8,3,0xf4,1])
    );
}
#[test]
fn definite_post_ff_failure_sticky_and_diagnostic_reconnect_requires_real_drain() {
    let (mut p, mut d, mut s, mut b, _) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    let command = s.take_command(0).unwrap().unwrap();
    let mut completion = bloxide_persistence::Completion {
        header: command.header,
        status: CompletionStatus::Failed { quiescent: true },
        data: [0; 256],
    };
    s.complete(completion.clone()).unwrap();
    for _ in 0..1000 {
        if s.ownership_settled() {
            break;
        }
        if let Some(c) = s.take_command(0).unwrap() {
            completion.header = c.header;
            completion.status = CompletionStatus::Failed { quiescent: true };
            s.complete(completion.clone()).unwrap();
        } else {
            break;
        }
    }
    assert!(matches!(
        s.retained(),
        Some(Outcome::Failed { .. } | Outcome::Indeterminate { .. })
    ));
    assert!(p.observe(0, &mut s, &mut b));
    assert!(p.request_bit());
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xfd]),
        [0xff, 1, 0, 0, 0, 0]
    );
    let before = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xfe, 0x10]
    );
    let after = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(&before[40..80], &after[40..80]);
    p.fence_wire();
    assert!(!p.diagnostic_reconnect(
        DrainProof {
            backend_idle: true,
            completions_drained: true,
            transport_idle: false,
            replies_drained: true,
            all_clients_recorded: true
        },
        &s
    ));
    assert!(p.diagnostic_reconnect(
        DrainProof {
            backend_idle: true,
            completions_drained: true,
            transport_idle: true,
            replies_drained: true,
            all_clients_recorded: true
        },
        &s
    ));
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]),
        [0xff, 1, 0, 8, 8, 0, 1, 1]
    );
    assert!(!p.request_bit());
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe5, 0, 0]),
        [0xff, 0, 0]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1]),
        [0xfe, 0x21]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 2, 0]),
        [0xfe, 0x22]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 1]),
        [0xfe, 0x28]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]),
        [0xfe, 0x27]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 0, 0]),
        [0xfe, 0x27]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xeb, 1, 0, 0]),
        [0xfe, 0x27]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xeb, 1, 0]),
        [0xfe, 0x21]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe5, 0, 0]),
        [0xff, 0, 0]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xfe, 0x27]
    );
    send(&mut p, &mut d, &mut s, &mut b, &mta(0x1000));
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf0, 2, 0xd0, 7]),
        [0xfe, 0x27]
    );
    assert_eq!(d.owner.active().period, 1000);
}

fn read_internal_view(
    p: &mut ProfileP,
    client: usize,
    s: &PersistenceService<LedSchema>,
) -> [u8; 640] {
    assert!(p.make_view(client, s));
    let mut out = [0; 640];
    let mut at = 0;
    while at < out.len() {
        let count = (out.len() - at).min(7) as u8;
        let reply = p.upload_client(client, count).unwrap();
        out[at..at + count as usize].copy_from_slice(&reply.as_slice()[1..]);
        at += count as usize;
    }
    out
}

#[test]
fn internal_denial_preserves_each_accepted_capture_then_exact_admissions_replace_it() {
    use bloxide_persistence::{OperationKey, RejectReason, Resolve, SaveResponse};
    use bloxide_persistence_calibration::capture_owner;

    for client in 1..4 {
        let (mut p, mut d, mut s, mut b, mut media) = fixture();
        let first = b.reserve(client).unwrap();
        let c1 = capture_owner(d.owner(), &LedSchema, first, None).unwrap();
        let k1 = OperationKey {
            service_epoch: first.service_epoch,
            session_generation: first.session_generation,
            sequence: first.sequence,
        };
        assert!(p.retain_client_capture(client, c1));
        assert_eq!(p.retained_key(client), None);
        let response = s.save(k1, c1, 100, || 0);
        assert_eq!(response, SaveResponse::Accepted);
        assert!(p.finish_client_save(client, response, &mut s, &mut b));
        drive(&mut s, &mut media);
        assert!(p.observe(client, &mut s, &mut b));
        let accepted = read_internal_view(&mut p, client, &s);
        assert_eq!(u32::from_le_bytes(accepted[36..40].try_into().unwrap()), 2);
        assert_eq!(&accepted[128..132], c1.bytes());
        assert_eq!(b.result(client), None);

        let second = b.reserve(client).unwrap();
        let c2 = capture_owner(d.owner(), &LedSchema, second, None).unwrap();
        let k2 = OperationKey {
            service_epoch: second.service_epoch,
            session_generation: second.session_generation,
            sequence: second.sequence,
        };
        assert!(p.retain_client_capture(client, c2));
        assert!(!p.retain_client_capture(client, c2));
        assert_eq!(p.retained_key(client), Some(k1));
        let response = s.save(k2, c2, 0, || 1);
        assert_eq!(
            response,
            SaveResponse::Terminal(Outcome::Rejected {
                key: k2,
                reason: RejectReason::Expired
            })
        );
        assert!(p.finish_client_save(client, response, &mut s, &mut b));
        assert_eq!(p.retained_key(client), Some(k1));
        assert!(matches!(s.resolve(k2), Resolve::Retired { .. }));
        assert_eq!(b.result(client), None);
        let after_denial = read_internal_view(&mut p, client, &s);
        assert_eq!(&accepted[12..16], &after_denial[12..16]);
        assert_eq!(&accepted[36..80], &after_denial[36..80]);
        assert_eq!(&accepted[128..384], &after_denial[128..384]);

        let third = b.reserve(client).unwrap();
        let c3 = capture_owner(d.owner(), &LedSchema, third, None).unwrap();
        let k3 = OperationKey {
            service_epoch: third.service_epoch,
            session_generation: third.session_generation,
            sequence: third.sequence,
        };
        assert!(p.retain_client_capture(client, c3));
        let response = s.save(k3, c3, 100, || 0);
        assert!(
            matches!(response, SaveResponse::Terminal(Outcome::Durable { key, existing: true, .. }) if key == k3)
        );
        assert_eq!(p.retained_key(client), Some(k1));
        assert!(p.finish_client_save(client, response, &mut s, &mut b));
        assert_eq!(p.retained_key(client), Some(k3));
        assert!(matches!(s.resolve(k3), Resolve::Retired { .. }));
        let fast = read_internal_view(&mut p, client, &s);
        assert_eq!(u32::from_le_bytes(fast[36..40].try_into().unwrap()), 2);
        assert_eq!(&fast[128..132], c3.bytes());

        d.write_scalar(0x1000, 2000u16.to_le_bytes(), 0).unwrap();
        let fourth = b.reserve(client).unwrap();
        let c4 = capture_owner(d.owner(), &LedSchema, fourth, None).unwrap();
        let k4 = OperationKey {
            service_epoch: fourth.service_epoch,
            session_generation: fourth.session_generation,
            sequence: fourth.sequence,
        };
        assert!(p.retain_client_capture(client, c4));
        let response = s.save(k4, c4, 100, || 0);
        assert_eq!(response, SaveResponse::Accepted);
        assert_eq!(p.retained_key(client), Some(k3));
        assert!(p.finish_client_save(client, response, &mut s, &mut b));
        let pending = read_internal_view(&mut p, client, &s);
        assert_eq!(u32::from_le_bytes(pending[36..40].try_into().unwrap()), 1);
        assert_eq!(&pending[128..132], c4.bytes());
        assert_eq!(&pending[384..388], c1.bytes());
        drive(&mut s, &mut media);
        assert!(p.observe(client, &mut s, &mut b));
        assert_eq!(p.retained_key(client), Some(k4));
        assert_eq!(b.result(client), None);
        let durable = read_internal_view(&mut p, client, &s);
        assert_eq!(u32::from_le_bytes(durable[36..40].try_into().unwrap()), 2);
        assert_eq!(&durable[128..132], c4.bytes());
        assert_eq!(&durable[384..388], c4.bytes());
    }
}

#[test]
fn internal_busy_candidate_cancels_after_other_client_settles_without_losing_history() {
    use bloxide_persistence::{OperationKey, Resolve, SaveResponse};
    use bloxide_persistence_calibration::capture_owner;
    let (mut p, mut d, mut s, mut b, mut media) = fixture();
    let first = b.reserve(1).unwrap();
    let c1 = capture_owner(d.owner(), &LedSchema, first, None).unwrap();
    let k1 = OperationKey {
        service_epoch: first.service_epoch,
        session_generation: first.session_generation,
        sequence: first.sequence,
    };
    assert!(p.retain_client_capture(1, c1));
    let response = s.save(k1, c1, 100, || 0);
    assert_eq!(response, SaveResponse::Accepted);
    assert!(p.finish_client_save(1, response, &mut s, &mut b));
    drive(&mut s, &mut media);
    assert!(p.observe(1, &mut s, &mut b));
    let before = read_internal_view(&mut p, 1, &s);

    d.write_scalar(0x1000, 2000u16.to_le_bytes(), 0).unwrap();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    let second = b.reserve(1).unwrap();
    let c2 = capture_owner(d.owner(), &LedSchema, second, None).unwrap();
    let k2 = OperationKey {
        service_epoch: second.service_epoch,
        session_generation: second.session_generation,
        sequence: second.sequence,
    };
    assert!(p.retain_client_capture(1, c2));
    let response = s.save(k2, c2, 100, || 0);
    assert_eq!(response, SaveResponse::Busy);
    assert!(!p.finish_client_save(1, response, &mut s, &mut b));
    assert!(p.is_fenced());
    assert_eq!(p.retained_key(1), Some(k1));
    drive(&mut s, &mut media);
    assert!(p.observe(0, &mut s, &mut b));
    assert!(p.reconcile_staged(&mut s, &mut b));
    assert!(matches!(s.resolve(k2), Resolve::Retired { .. }));
    assert_eq!(b.result(1), None);
    let after = read_internal_view(&mut p, 1, &s);
    assert_eq!(&before[36..80], &after[36..80]);
    assert_eq!(&before[128..384], &after[128..384]);
}
#[test]
fn retained_latch_chunks_and_errors_do_not_advance_cursor() {
    let (mut p, mut d, mut s, mut b, _) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &mta(VIEW_BASE + 1)),
        [0xfe, 0x24]
    );
    send(&mut p, &mut d, &mut s, &mut b, &mta(VIEW_BASE));
    let one = send(&mut p, &mut d, &mut s, &mut b, &[0xf5, 1]);
    assert_eq!(one, &[0xff, b'B']);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf5, 8]),
        [0xfe, 0x22]
    );
    let seven = send(&mut p, &mut d, &mut s, &mut b, &[0xf5, 7]);
    assert_eq!(&seven[1..], b"LXPVW01");
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf0, 2, 1, 0]),
        [0xfe, 0x23]
    );
    send(&mut p, &mut d, &mut s, &mut b, &mta(VIEW_BASE + 639));
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf5, 2]),
        [0xfe, 0x24]
    );
    assert_eq!(send(&mut p, &mut d, &mut s, &mut b, &[0xf5, 1]).len(), 2);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &mta(VIEW_BASE + 1)),
        [0xfe, 0x24]
    );
    assert!(core::mem::size_of::<ProfileP>() <= 6144);
}

#[test]
fn all_four_clients_record_before_namespace_advance_and_fast_existing() {
    use bloxide_persistence::{OperationKey, SaveResponse};
    use bloxide_persistence_calibration::capture_owner;
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    drive(&mut s, &mut m);
    assert!(p.observe(0, &mut s, &mut b));
    let before = read_view(&mut p, &mut d, &mut s, &mut b);
    let denied = bytes(p.handle(
        &packet(&[0xf9, 1, 0, 0]),
        &mut d,
        &LedSchema,
        &mut s,
        &mut b,
        Admission {
            disarmed: false,
            maintenance: true,
            schema_known: true,
        },
        0,
        || 0,
    ));
    assert_eq!(denied, [0xfe, 0x27]);
    let after = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(&before[40..80], &after[40..80]);
    for client in 1..4 {
        let key = b.reserve(client).unwrap();
        let captured = capture_owner(d.owner(), &LedSchema, key, None).unwrap();
        assert!(p.retain_client_capture(client, captured));
        let operation = OperationKey {
            service_epoch: key.service_epoch,
            session_generation: key.session_generation,
            sequence: key.sequence,
        };
        let response = s.save(operation, captured, 100, || 0);
        assert!(matches!(
            response,
            SaveResponse::Terminal(Outcome::Durable { existing: true, .. })
        ));
        assert!(p.finish_client_save(client, response, &mut s, &mut b));
        assert_eq!(b.result(client), None);
    }
    s.stop();
    let proof = DrainProof {
        backend_idle: true,
        completions_drained: true,
        transport_idle: true,
        replies_drained: true,
        all_clients_recorded: true,
    };
    assert!(!p.advance_namespace(
        DrainProof {
            transport_idle: false,
            ..proof
        },
        &mut s,
        &mut b,
        2
    ));
    assert!(p.advance_namespace(proof, &mut s, &mut b, 2));
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]),
        [0xff, 1, 0, 8, 8, 0, 1, 1]
    );
    assert_eq!((p.service_epoch(), p.generation()), (2, 2));
}

#[test]
fn uncertain_completion_and_mismatch_keep_original_custody() {
    use bloxide_persistence::CompletionError;
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    let key = p.retained_key(0).unwrap();
    let command = s.take_command(0).unwrap().unwrap();
    let mut wrong = m.execute(command.clone());
    wrong.header.command_sequence += 1;
    assert_eq!(
        p.complete_backend(&mut s, wrong),
        Err(CompletionError::CorrelationMismatch)
    );
    assert!(p.is_fenced());
    assert_eq!(p.retained_key(0), Some(key));
    assert!(!s.ownership_settled());
    assert!(!p.diagnostic_reconnect(
        DrainProof {
            backend_idle: true,
            completions_drained: true,
            transport_idle: true,
            replies_drained: true,
            all_clients_recorded: true
        },
        &s
    ));
    // The actual completion still belongs to the original command. It may
    // refine uncertainty, but never licenses a second F9 response or retry.
    let exact = m.execute(command);
    let _ = p.complete_backend(&mut s, exact);
    assert_eq!(p.retained_key(0), Some(key));
    assert!(matches!(
        s.retained(),
        Some(Outcome::Indeterminate { .. } | Outcome::Failed { .. }) | None
    ));
}

#[test]
fn internal_client_latches_remain_coherent_across_other_client_save() {
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    assert!(p.make_view(1, &s));
    let first = p.upload_client(1, 7).unwrap();
    assert_eq!(&first.as_slice()[1..], b"BLXPVW0");
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]);
    drive(&mut s, &mut m);
    assert!(p.observe(0, &mut s, &mut b));
    assert!(p.make_view(2, &s));
    let mut old = [0u8; 640];
    old[..7].copy_from_slice(&first.as_slice()[1..]);
    let mut at = 7;
    while at < 640 {
        let n = ((at % 7) + 1).min(640 - at);
        let r = p.upload_client(1, n as u8).unwrap();
        old[at..at + n].copy_from_slice(&r.as_slice()[1..]);
        at += n;
    }
    let mut fresh = [0u8; 640];
    let mut at = 0;
    while at < 640 {
        let n = ((at % 7) + 1).min(640 - at);
        let r = p.upload_client(2, n as u8).unwrap();
        fresh[at..at + n].copy_from_slice(&r.as_slice()[1..]);
        at += n;
    }
    assert_eq!(&old[384..388], &[0xff; 4]);
    assert_eq!(&fresh[384..388], &[0xe8, 3, 0xf4, 1]);
    assert!(p.upload_client(1, 1).is_none());
    assert!(p.upload_client(2, 1).is_none());
    assert_eq!(d.owner.active().period, 1000);
}

#[test]
fn endpoint_refuses_missing_and_uniform_synthetic_identity_at_construction() {
    use xcp_profile_p::IdentityError;
    assert!(matches!(
        ProfileP::new([0; 128], 1),
        Err(IdentityError::Layout)
    ));
    let mut identity = [0u8; 128];
    identity[..8].copy_from_slice(b"BLXPP001");
    identity[8..10].copy_from_slice(&1u16.to_le_bytes());
    identity[10..12].copy_from_slice(&640u16.to_le_bytes());
    identity[12..16].copy_from_slice(&0x5000_0001u32.to_le_bytes());
    identity[16..112].fill(0xcd);
    identity[112..116].copy_from_slice(&0x10000u32.to_le_bytes());
    identity[116..120].copy_from_slice(&0x1000u32.to_le_bytes());
    identity[120..122].copy_from_slice(&4u16.to_le_bytes());
    identity[122..124].copy_from_slice(&1u16.to_le_bytes());
    identity[124..128].copy_from_slice(&[4, 1, 1, 0]);
    assert!(matches!(
        ProfileP::new(identity, 1),
        Err(IdentityError::Unbound)
    ));
}

#[test]
fn post_ff_uncertainty_refines_only_after_exact_late_completion() {
    use bloxide_persistence::CompletionStatus;
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    let key = p.retained_key(0).unwrap();
    let command = s.take_command(0).unwrap().unwrap();
    let mut uncertain = m.execute(command.clone());
    uncertain.status = CompletionStatus::Failed { quiescent: false };
    p.complete_backend(&mut s, uncertain).unwrap();
    assert!(matches!(s.retained(),Some(Outcome::Indeterminate{key:k,..}) if k==key));
    assert!(!s.ownership_settled());
    assert!(!p.observe(0, &mut s, &mut b));
    assert!(p.request_bit());
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 0, 0]),
        [0xfe, 0x10]
    );
    let view = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(u32::from_le_bytes(view[12..16].try_into().unwrap()) & 8, 0);
    assert_eq!(p.retained_key(0), Some(key));
    p.complete_backend(&mut s, m.execute(command)).unwrap();
    assert!(s.ownership_settled());
    assert!(p.observe(0, &mut s, &mut b));
    assert!(p.request_bit());
    assert!(s.read_durable().is_none());
    assert_eq!(p.retained_key(0), Some(key));
}

#[test]
fn lost_ff_fences_stream_without_retry_and_requires_true_drain_before_new_connect() {
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    let key = p.retained_key(0).unwrap();
    p.fence_wire();
    assert!(matches!(
        p.handle(
            &packet(&[0xfd]),
            &mut d,
            &LedSchema,
            &mut s,
            &mut b,
            Admission {
                disarmed: true,
                maintenance: true,
                schema_known: true
            },
            0,
            || 0
        ),
        Reply::Fenced
    ));
    assert!(matches!(
        p.handle(
            &packet(&[0xff, 0]),
            &mut d,
            &LedSchema,
            &mut s,
            &mut b,
            Admission {
                disarmed: true,
                maintenance: true,
                schema_known: true
            },
            0,
            || 0
        ),
        Reply::Fenced
    ));
    drive(&mut s, &mut m);
    assert!(p.observe(0, &mut s, &mut b));
    assert_eq!(p.retained_key(0), Some(key));
    s.stop();
    let proof = DrainProof {
        backend_idle: true,
        completions_drained: true,
        transport_idle: true,
        replies_drained: true,
        all_clients_recorded: true,
    };
    assert!(!p.advance_namespace(
        DrainProof {
            replies_drained: false,
            ..proof
        },
        &mut s,
        &mut b,
        2
    ));
    assert!(p.advance_namespace(proof, &mut s, &mut b, 2));
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]),
        [0xff, 1, 0, 8, 8, 0, 1, 1]
    );
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xfd]),
        [0xff, 0, 0, 0, 0, 0]
    );
    assert_eq!(p.retained_key(0), Some(key));
}

#[test]
fn partial_epoch_handshake_and_counter_exhaustion_stay_fenced() {
    let (mut p, mut d, mut s, mut b, _) = fixture();
    let proof = DrainProof {
        backend_idle: true,
        completions_drained: true,
        transport_idle: true,
        replies_drained: true,
        all_clients_recorded: true,
    };
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    s.stop();
    assert!(!p.advance_namespace(proof, &mut s, &mut b, 1)); // broker advanced, service I/O epoch did not
    assert!(p.is_fenced());
    assert!(!p.advance_namespace(proof, &mut s, &mut b, 2)); // no reset/reconstruction of broker
    let identity = *p.identity();
    let g = Geometry::new(1024, 8).unwrap();
    let mut sim = Simulator::new(g, 100, 99);
    let config = sim.open_session().unwrap();
    let recovery = recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap();
    let mut exhausted_service =
        PersistenceService::new(g, LedSchema, recovery, false, u64::MAX, 1, 1, 0, config);
    let mut exhausted_broker = Broker::<4>::new(u64::MAX, 1);
    let mut exhausted = ProfileP::new(identity, u64::MAX).unwrap();
    send(
        &mut exhausted,
        &mut d,
        &mut exhausted_service,
        &mut exhausted_broker,
        &[0xff, 0],
    );
    exhausted_service.stop();
    assert!(!exhausted.advance_namespace(proof, &mut exhausted_service, &mut exhausted_broker, 2));
    assert!(exhausted.is_fenced());
}

#[test]
fn wire_fast_durable_existing_clears_bit_without_observed_pending_poll() {
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    let identity = *p.identity();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    assert_eq!(
        send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]),
        [0xff]
    );
    drive(&mut s, &mut m);
    assert!(p.observe(0, &mut s, &mut b));
    let g = Geometry::new(1024, 8).unwrap();
    let a = classify_slot(
        SlotRead {
            bytes: m.slot(Slot::A),
            issue: None,
        },
        g,
        &LedSchema,
    );
    let bb = classify_slot(
        SlotRead {
            bytes: m.slot(Slot::B),
            issue: None,
        },
        g,
        &LedSchema,
    );
    let recovered = recover(a, bb, &LedSchema).unwrap();
    let backend = m.open_session().unwrap();
    let mut service = PersistenceService::new(g, LedSchema, recovered, false, 2, 1, 2, 0, backend);
    let mut broker = Broker::<4>::new(2, 1);
    let mut profile = ProfileP::new(identity, 2).unwrap();
    let mut domain = LedDomain::new();
    send(
        &mut profile,
        &mut domain,
        &mut service,
        &mut broker,
        &[0xff, 0],
    );
    send(
        &mut profile,
        &mut domain,
        &mut service,
        &mut broker,
        &[0xe6, 1, 0],
    );
    assert_eq!(
        send(
            &mut profile,
            &mut domain,
            &mut service,
            &mut broker,
            &[0xf9, 1, 0, 0]
        ),
        [0xff]
    );
    assert!(!profile.request_bit());
    assert_eq!(
        send(
            &mut profile,
            &mut domain,
            &mut service,
            &mut broker,
            &[0xfd]
        ),
        [0xff, 0, 0, 0, 0, 0]
    );
    let view = read_view(&mut profile, &mut domain, &mut service, &mut broker);
    assert_eq!(&view[128..132], &view[384..388]);
    assert_eq!(u32::from_le_bytes(view[36..40].try_into().unwrap()), 2);
}

#[test]
fn another_clients_later_durable_record_does_not_replace_wire_capture_or_result() {
    use bloxide_persistence::{OperationKey, SaveResponse};
    use bloxide_persistence_calibration::capture_owner;
    let (mut p, mut d, mut s, mut b, mut m) = fixture();
    send(&mut p, &mut d, &mut s, &mut b, &[0xff, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xe6, 1, 0]);
    send(&mut p, &mut d, &mut s, &mut b, &[0xf9, 1, 0, 0]);
    drive(&mut s, &mut m);
    assert!(p.observe(0, &mut s, &mut b));
    let old_key = p.retained_key(0).unwrap();
    d.write_scalar(0x1000, 2000u16.to_le_bytes(), 0).unwrap();
    let key = b.reserve(1).unwrap();
    let capture = capture_owner(d.owner(), &LedSchema, key, None).unwrap();
    assert!(p.retain_client_capture(1, capture));
    let operation = OperationKey {
        service_epoch: key.service_epoch,
        session_generation: key.session_generation,
        sequence: key.sequence,
    };
    let response = s.save(operation, capture, 100, || 0);
    assert_eq!(response, SaveResponse::Accepted);
    assert!(p.finish_client_save(1, response, &mut s, &mut b));
    drive(&mut s, &mut m);
    assert!(p.observe(1, &mut s, &mut b));
    let view = read_view(&mut p, &mut d, &mut s, &mut b);
    assert_eq!(&view[128..132], &[0xe8, 3, 0xf4, 1]);
    assert_eq!(&view[384..388], &[0xd0, 7, 0xf4, 1]);
    assert_eq!(p.retained_key(0), Some(old_key));
    assert!(!p.request_bit());
    assert_eq!(u32::from_le_bytes(view[36..40].try_into().unwrap()), 2);
}
