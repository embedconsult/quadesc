#![allow(dead_code)]
use bloxide_persistence::*;
use bloxide_persistence_sim::Simulator;

#[derive(Clone, Copy)]
pub struct Thermostat;
impl Schema for Thermostat {
    fn id(&self) -> u16 {
        2
    }
    fn payload_len(&self) -> u16 {
        8
    }
    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        out[..4].copy_from_slice(&20000i32.to_le_bytes());
        out[4..8].copy_from_slice(&1000u32.to_le_bytes());
        Ok(())
    }
    fn validate(&self, b: &[u8]) -> Result<(), SchemaError> {
        if b.len() != 8 {
            return Err(SchemaError::InvalidValue);
        }
        let t = i32::from_le_bytes(b[..4].try_into().unwrap());
        let h = u32::from_le_bytes(b[4..].try_into().unwrap());
        if (5000..=35000).contains(&t)
            && (100..=5000).contains(&h)
            && i64::from(t) - i64::from(h) >= 0
        {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}
pub fn key(n: u64) -> OperationKey {
    OperationKey {
        service_epoch: 17,
        session_generation: 3,
        sequence: n,
    }
}
pub fn capture(n: u64) -> Snapshot {
    let mut b = [0; 8];
    b[..4].copy_from_slice(&(20000 + (n % 1000) as i32).to_le_bytes());
    b[4..].copy_from_slice(&1000u32.to_le_bytes());
    Snapshot::new(&Thermostat, &b, 11, n as u32, key(n)).unwrap()
}
pub fn recovery(m: &Simulator, g: Geometry) -> Recovery {
    let c = |slot| {
        classify_slot(
            SlotRead {
                bytes: m.slot(slot),
                issue: None,
            },
            g,
            &Thermostat,
        )
    };
    recover(c(Slot::A), c(Slot::B), &Thermostat).unwrap()
}
pub fn boot(m: &mut Simulator, g: Geometry) -> PersistenceService<Thermostat> {
    let r = recovery(m, g);
    let config = m.open_session().unwrap();
    PersistenceService::new(g, Thermostat, r, true, 17, 3, 23, 0, config)
}
pub fn drive(s: &mut PersistenceService<Thermostat>, m: &mut Simulator) -> usize {
    for count in 0..2000 {
        if s.ownership_settled() {
            return count;
        }
        let c = s
            .take_command(0)
            .unwrap()
            .expect("bounded healthy progress");
        assert!(s.take_command(0).unwrap().is_none());
        s.complete(m.execute(c)).unwrap();
    }
    panic!("unbounded service")
}
pub fn until(s: &mut PersistenceService<Thermostat>, m: &mut Simulator, state: ServiceState) {
    for _ in 0..2000 {
        if s.state() == state {
            return;
        }
        let c = s.take_command(0).unwrap().unwrap();
        s.complete(m.execute(c)).unwrap();
    }
    panic!("missing state")
}
pub fn save(s: &mut PersistenceService<Thermostat>, m: &mut Simulator, n: u64) {
    assert_eq!(
        s.save(key(n), capture(n), 100, || 0),
        SaveResponse::Accepted
    );
    drive(s, m);
    assert!(
        matches!(s.retained(),Some(Outcome::Durable{key:k, record,..}) if k==key(n) && record.snapshot==capture(n))
    );
    s.release(key(n)).unwrap();
}

// Independent raw decoder: normal polynomial, reflected inputs/output; no product
// encoder/parser/checksum in this oracle. Compare all persistent fields, not CRC alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity {
    pub sequence: u64,
    pub source_epoch: u64,
    pub revision: u32,
    pub generation: u32,
    pub op_sequence: u64,
    pub payload: Vec<u8>,
}
pub fn crc(bytes: &[u8]) -> u32 {
    let mut c = 0xffffffffu32;
    for b in bytes {
        c ^= u32::from(b.reverse_bits()) << 24;
        for _ in 0..8 {
            c = if c & 0x80000000 != 0 {
                (c << 1) ^ 0x1edc6f41
            } else {
                c << 1
            };
        }
    }
    (!c).reverse_bits()
}
pub fn oracle(p: &[u8], g: Geometry) -> Option<Identity> {
    let marker = 640usize.div_ceil(g.granule_bytes() as usize) * g.granule_bytes() as usize;
    let end = marker + g.granule_bytes() as usize;
    if p.len() != g.erase_bytes() as usize
        || p[marker..end].iter().any(|&x| x != 0)
        || p[..640]
            .chunks_exact(2)
            .any(|x| x[0] & x[1] != 0 || x[0].count_ones() + x[1].count_ones() != 8)
        || p[640..marker].iter().chain(&p[end..]).any(|&x| x != 255)
    {
        return None;
    }
    let b: Vec<_> = p[..640].iter().step_by(2).copied().collect();
    let u16at = |i| u16::from_le_bytes(b[i..i + 2].try_into().unwrap());
    let u32at = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let u64at = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    let mut domain = b[..60].to_vec();
    domain.extend_from_slice(&b[64..]);
    if &b[..8] != b"BLXPERS2"
        || u16at(8) != 2
        || u16at(10) != 2
        || u32at(12) != 8
        || u64at(16) == 0
        || b[48..60].iter().any(|&x| x != 0)
        || b[72..].iter().any(|&x| x != 255)
        || u32at(60) != crc(&domain)
    {
        return None;
    }
    let t = i32::from_le_bytes(b[64..68].try_into().unwrap());
    let h = u32at(68);
    if !(5000..=35000).contains(&t) || !(100..=5000).contains(&h) || i64::from(t) - i64::from(h) < 0
    {
        return None;
    }
    Some(Identity {
        sequence: u64at(16),
        source_epoch: u64at(24),
        revision: u32at(32),
        generation: u32at(36),
        op_sequence: u64at(40),
        payload: b[64..72].to_vec(),
    })
}
pub fn identity(r: Record) -> Identity {
    Identity {
        sequence: r.sequence,
        source_epoch: r.snapshot.source_epoch(),
        revision: r.snapshot.source_revision(),
        generation: r.snapshot.operation().session_generation,
        op_sequence: r.snapshot.operation().sequence,
        payload: r.snapshot.bytes().to_vec(),
    }
}
pub fn selected(m: &Simulator, g: Geometry) -> Option<Identity> {
    match (oracle(m.slot(Slot::A), g), oracle(m.slot(Slot::B), g)) {
        (Some(a), Some(b)) => Some(if a.sequence >= b.sequence { a } else { b }),
        (a, b) => a.or(b),
    }
}
pub const GEOMETRIES: [(u32, u16); 5] = [(1024, 1), (1024, 8), (1024, 16), (4096, 64), (4096, 256)];
