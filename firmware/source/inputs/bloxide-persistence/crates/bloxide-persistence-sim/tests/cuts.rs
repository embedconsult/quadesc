use bloxide_persistence::{
    Geometry, OperationKey, Record, Schema, SchemaError, Snapshot, encode_record,
};

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
        let period = u16::from_le_bytes([bytes[0], bytes[1]]);
        let duty = u16::from_le_bytes([bytes[2], bytes[3]]);
        if bytes.len() == 4 && (100..=10_000).contains(&period) && duty <= 1000 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OracleRecord {
    sequence: u64,
    source_epoch: u64,
    source_revision: u32,
    generation: u32,
    operation: u64,
    payload: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OracleClass {
    Empty,
    Uncommitted,
    Corrupt,
    Unsupported,
    Valid(OracleRecord),
}

fn crc32c_parts(first: &[u8], second: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in first.iter().chain(second) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x82F6_3B78
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

// This decoder is intentionally written from the byte contract and imports no
// production classification or recovery function.
fn oracle_classify(bytes: &[u8], geometry: Geometry) -> OracleClass {
    if bytes.iter().all(|&byte| byte == 0xFF) {
        return OracleClass::Empty;
    }
    let marker = usize::from(geometry.marker_offset());
    let marker_end = marker + usize::from(geometry.granule_bytes());
    if bytes[marker..marker_end].iter().any(|&byte| byte != 0) {
        return OracleClass::Uncommitted;
    }
    let mut logical = [0_u8; 320];
    for (index, pair) in bytes[..640].chunks_exact(2).enumerate() {
        if pair[0] ^ pair[1] != 0xFF {
            return OracleClass::Corrupt;
        }
        logical[index] = pair[0];
    }
    let length = u32_at(&logical, 12);
    let sequence = u64_at(&logical, 16);
    if &logical[..8] != b"BLXPERS2"
        || u16_at(&logical, 10) == 0
        || sequence == 0
        || !(1..=256).contains(&length)
        || logical[48..60].iter().any(|&byte| byte != 0)
        || logical[64 + length as usize..]
            .iter()
            .any(|&byte| byte != 0xFF)
        || bytes[640..marker].iter().any(|&byte| byte != 0xFF)
        || bytes[marker_end..].iter().any(|&byte| byte != 0xFF)
        || u32_at(&logical, 60) != crc32c_parts(&logical[..60], &logical[64..])
    {
        return OracleClass::Corrupt;
    }
    if u16_at(&logical, 8) != 2 || u16_at(&logical, 10) != 1 {
        return OracleClass::Unsupported;
    }
    if length != 4 {
        return OracleClass::Corrupt;
    }
    let payload: [u8; 4] = logical[64..68].try_into().unwrap();
    let period = u16::from_le_bytes(payload[..2].try_into().unwrap());
    let duty = u16::from_le_bytes(payload[2..].try_into().unwrap());
    if !(100..=10_000).contains(&period) || duty > 1000 {
        return OracleClass::Corrupt;
    }
    OracleClass::Valid(OracleRecord {
        sequence,
        source_epoch: u64_at(&logical, 24),
        source_revision: u32_at(&logical, 32),
        generation: u32_at(&logical, 36),
        operation: u64_at(&logical, 40),
        payload,
    })
}

fn oracle_recover(a: &[u8], b: &[u8], geometry: Geometry) -> Option<OracleRecord> {
    let a = oracle_classify(a, geometry);
    let b = oracle_classify(b, geometry);
    assert!(!matches!(a, OracleClass::Unsupported));
    assert!(!matches!(b, OracleClass::Unsupported));
    match (a, b) {
        (OracleClass::Valid(a), OracleClass::Valid(b)) if a.sequence == b.sequence => {
            assert_eq!(a, b, "equal sequence with different records is ambiguous");
            Some(a)
        }
        (OracleClass::Valid(a), OracleClass::Valid(b)) => {
            Some(if a.sequence > b.sequence { a } else { b })
        }
        (OracleClass::Valid(record), _) | (_, OracleClass::Valid(record)) => Some(record),
        _ => None,
    }
}

fn snapshot(period: u16, duty: u16, revision: u32, sequence: u64) -> Snapshot {
    let mut payload = [0_u8; 4];
    payload[..2].copy_from_slice(&period.to_le_bytes());
    payload[2..].copy_from_slice(&duty.to_le_bytes());
    Snapshot::new(
        &LedSchema,
        &payload,
        1,
        revision,
        OperationKey {
            service_epoch: 8,
            session_generation: 1,
            sequence,
        },
    )
    .unwrap()
}

fn image(record: Record, geometry: Geometry) -> Vec<u8> {
    let programmed = encode_record(&record, geometry);
    let mut bytes = vec![0xFF; geometry.erase_bytes() as usize];
    bytes[..programmed.bytes().len()].copy_from_slice(programmed.bytes());
    bytes
}

fn old_new(record: OracleRecord, old: OracleRecord, new: OracleRecord) {
    assert!(
        record == old || record == new,
        "recovered third configuration: {record:?}"
    );
}

#[test]
fn every_destructive_byte_prefix_recovers_old_or_new() {
    let geometries = [(1, 1024), (8, 1024), (16, 1024), (64, 4096), (256, 4096)];
    let mut checks = 0_u64;
    for (granule, erase) in geometries {
        let geometry = Geometry::new(erase, granule).unwrap();
        let old_a = image(Record::new(7, snapshot(2000, 250, 10, 7)), geometry);
        let predecessor_b = image(Record::new(6, snapshot(1000, 500, 9, 6)), geometry);
        let new_b = image(Record::new(8, snapshot(3000, 750, 11, 8)), geometry);
        let old = oracle_recover(&old_a, &predecessor_b, geometry).unwrap();
        let new = oracle_classify(&new_b, geometry);
        let OracleClass::Valid(new) = new else {
            panic!("new vector invalid")
        };

        for prefix in 0..=erase as usize {
            let mut target = predecessor_b.clone();
            target[..prefix].fill(0xFF);
            let recovered = oracle_recover(&old_a, &target, geometry).unwrap();
            old_new(recovered, old, new);
            checks += 1;
        }

        let marker = usize::from(geometry.marker_offset());
        for prefix in 0..=marker {
            let mut target = vec![0xFF; erase as usize];
            for index in 0..prefix {
                target[index] &= new_b[index];
            }
            let recovered = oracle_recover(&old_a, &target, geometry).unwrap();
            old_new(recovered, old, new);
            checks += 1;
        }
        for prefix in 0..=usize::from(granule) {
            let mut target = vec![0xFF; erase as usize];
            target[..marker].copy_from_slice(&new_b[..marker]);
            target[marker..marker + prefix].copy_from_slice(&new_b[marker..marker + prefix]);
            let recovered = oracle_recover(&old_a, &target, geometry).unwrap();
            old_new(recovered, old, new);
            checks += 1;
        }

        let old_b = image(Record::new(7, snapshot(2000, 250, 10, 7)), geometry);
        let predecessor_a = image(Record::new(6, snapshot(1000, 500, 9, 6)), geometry);
        let new_a = image(Record::new(8, snapshot(3000, 750, 11, 8)), geometry);
        let reverse_old = oracle_recover(&predecessor_a, &old_b, geometry).unwrap();
        for prefix in 0..=usize::from(geometry.programmed_span()) {
            let mut target = vec![0xFF; erase as usize];
            for index in 0..prefix {
                target[index] &= new_a[index];
            }
            let recovered = oracle_recover(&target, &old_b, geometry).unwrap();
            old_new(recovered, reverse_old, new);
            checks += 1;
        }

        let first = image(Record::new(1, snapshot(1000, 500, 0, 1)), geometry);
        let blank = vec![0xFF; erase as usize];
        for prefix in 0..=usize::from(geometry.programmed_span()) {
            let mut target = blank.clone();
            for index in 0..prefix {
                target[index] &= first[index];
            }
            let recovered = oracle_recover(&target, &blank, geometry);
            assert!(matches!(
                recovered,
                None | Some(OracleRecord { sequence: 1, .. })
            ));
            checks += 1;
        }
    }
    assert!(checks > 20_000);
}

#[test]
fn seeded_nonprefix_and_repeated_erase_subsets_preserve_old() {
    let geometries = [(1, 1024), (8, 1024), (16, 1024), (64, 4096), (256, 4096)];
    let mut state = 17_u64;
    let mut checks = 0_u32;
    for (granule, erase) in geometries {
        let geometry = Geometry::new(erase, granule).unwrap();
        let old_a = image(Record::new(7, snapshot(2000, 250, 10, 7)), geometry);
        let predecessor = image(Record::new(6, snapshot(1000, 500, 9, 6)), geometry);
        let old = oracle_recover(&old_a, &predecessor, geometry).unwrap();
        for _ in 0..4096 {
            let mut once = predecessor.clone();
            for byte in &mut once {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                *byte |= (state >> 56) as u8;
            }
            let mut twice = once.clone();
            for byte in &mut twice {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                *byte |= (state >> 56) as u8;
            }
            assert_eq!(oracle_recover(&old_a, &once, geometry), Some(old));
            assert_eq!(oracle_recover(&old_a, &twice, geometry), Some(old));
            checks += 2;
        }
    }
    assert_eq!(checks, 40_960);
}

#[test]
fn reviewed_witness_and_vectors_have_independent_expected_meaning() {
    let geometry = Geometry::new(1024, 8).unwrap();
    let active = include_bytes!("../../../evidence/r2-vectors/active-a.bin");
    let predecessor = include_bytes!("../../../evidence/r2-vectors/inactive-b.bin");
    let witness = include_bytes!("../../../evidence/r2-vectors/witness-data-rails.bin");
    let intended = include_bytes!("../../../evidence/r2-vectors/intended-new.bin");
    assert_eq!(
        oracle_classify(active, geometry),
        OracleClass::Valid(OracleRecord {
            sequence: 7,
            source_epoch: 1,
            source_revision: 10,
            generation: 1,
            operation: 7,
            payload: [0xD0, 0x07, 0xFA, 0x00],
        })
    );
    assert!(matches!(
        oracle_classify(predecessor, geometry),
        OracleClass::Valid(_)
    ));
    assert_eq!(oracle_classify(witness, geometry), OracleClass::Corrupt);
    let old = oracle_recover(active, predecessor, geometry).unwrap();
    assert_eq!(oracle_recover(active, witness, geometry), Some(old));
    assert!(matches!(
        oracle_classify(intended, geometry),
        OracleClass::Valid(OracleRecord { sequence: 8, .. })
    ));
}
