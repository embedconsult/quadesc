use bloxide_persistence::{
    Classification, CorruptReason, EraseQualification, FORMAT, Geometry, GeometryError,
    OperationKey, ReadIssue, Record, RecoveryDisposition, RecoveryError, SavePlan, Schema,
    SchemaError, Slot, SlotRead, Snapshot, UnsupportedKind, classify_slot, crc32c, encode_record,
    qualify_erase, recover,
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
        if bytes.len() != 4 {
            return Err(SchemaError::InvalidValue);
        }
        let period = u16::from_le_bytes([bytes[0], bytes[1]]);
        let duty = u16::from_le_bytes([bytes[2], bytes[3]]);
        if (100..=10_000).contains(&period) && duty <= 1000 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}

#[derive(Clone, Copy)]
struct OtherSchema;

impl Schema for OtherSchema {
    fn id(&self) -> u16 {
        2
    }

    fn payload_len(&self) -> u16 {
        4
    }

    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        out[..4].copy_from_slice(&[1, 2, 3, 4]);
        Ok(())
    }

    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError> {
        (bytes.len() == 4)
            .then_some(())
            .ok_or(SchemaError::InvalidValue)
    }
}

fn key(sequence: u64) -> OperationKey {
    OperationKey {
        service_epoch: 9,
        session_generation: 1,
        sequence,
    }
}

fn led_snapshot(period: u16, duty: u16, epoch: u64, revision: u32, op: u64) -> Snapshot {
    let mut bytes = [0_u8; 4];
    bytes[..2].copy_from_slice(&period.to_le_bytes());
    bytes[2..].copy_from_slice(&duty.to_le_bytes());
    Snapshot::new(&LedSchema, &bytes, epoch, revision, key(op)).unwrap()
}

fn full_image(record: Record, geometry: Geometry) -> Vec<u8> {
    let image = encode_record(&record, geometry);
    let mut full = vec![0xFF; geometry.erase_bytes() as usize];
    full[..image.bytes().len()].copy_from_slice(image.bytes());
    full
}

fn classify(bytes: &[u8], geometry: Geometry) -> Classification {
    classify_slot(SlotRead { bytes, issue: None }, geometry, &LedSchema)
}

#[test]
fn geometry_is_checked_before_use() {
    for granule in [1, 2, 4, 8, 16, 32, 64, 128] {
        let geometry = Geometry::new(1024, granule).unwrap();
        assert_eq!(geometry.body_span(), 640);
        assert!(geometry.programmed_span() <= 1024);
    }
    let geometry = Geometry::new(4096, 256).unwrap();
    assert_eq!(geometry.body_span(), 768);
    assert_eq!(geometry.programmed_span(), 1024);
    assert_eq!(Geometry::new(512, 8), Err(GeometryError::RecordDoesNotFit));
    assert_eq!(
        Geometry::new(1024, 3),
        Err(GeometryError::GranuleNotPowerOfTwo)
    );
    assert_eq!(
        Geometry::new(513, 8),
        Err(GeometryError::EraseNotGranuleMultiple)
    );
    assert_eq!(
        Geometry::new(65_537, 1),
        Err(GeometryError::EraseOutOfRange)
    );
    assert_eq!(geometry.normal_save_commands(), 3 + 3 + 4 * 16);
}

#[test]
fn crc_and_reviewed_vector_are_exact() {
    assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    let geometry = Geometry::new(1024, 8).unwrap();
    let actual = full_image(Record::new(7, led_snapshot(2000, 250, 1, 10, 7)), geometry);
    assert_eq!(
        actual.as_slice(),
        include_bytes!("../evidence/r2-vectors/active-a.bin")
    );
    assert_eq!(
        classify(&actual, geometry),
        Classification::Valid(Record::new(
            7,
            Snapshot::new(
                &LedSchema,
                &[0xD0, 0x07, 0xFA, 0x00],
                1,
                10,
                OperationKey {
                    service_epoch: 0,
                    session_generation: 1,
                    sequence: 7,
                },
            )
            .unwrap(),
        ))
    );
}

#[test]
fn classifier_orders_pair_checks_before_unknown_authority() {
    let geometry = Geometry::new(1024, 8).unwrap();
    let snapshot = led_snapshot(2000, 250, 1, 10, 7);
    let unknown = full_image(
        Record {
            format: 3,
            sequence: 7,
            snapshot,
        },
        geometry,
    );
    assert_eq!(
        classify(&unknown, geometry),
        Classification::Unsupported(UnsupportedKind::Format(3))
    );

    let other = Snapshot::new(&OtherSchema, &[1, 2, 3, 4], 1, 10, key(7)).unwrap();
    let unknown_schema = full_image(Record::new(7, other), geometry);
    assert_eq!(
        classify(&unknown_schema, geometry),
        Classification::Unsupported(UnsupportedKind::Schema(2))
    );

    let mut broken_unknown = unknown;
    broken_unknown[0] ^= 1;
    assert_eq!(
        classify(&broken_unknown, geometry),
        Classification::Corrupt(CorruptReason::BodyPair)
    );
    assert_eq!(
        classify_slot(
            SlotRead {
                bytes: &broken_unknown,
                issue: Some(ReadIssue::CorrectedEcc),
            },
            geometry,
            &LedSchema,
        ),
        Classification::Unreadable(ReadIssue::CorrectedEcc)
    );
}

#[test]
fn every_single_bit_change_is_non_valid_in_both_slots() {
    let geometry = Geometry::new(1024, 8).unwrap();
    let original = full_image(Record::new(7, led_snapshot(2000, 250, 1, 10, 7)), geometry);
    let mut checked = 0_u32;
    for _slot in [Slot::A, Slot::B] {
        for bit in 0..original.len() * 8 {
            let mut damaged = original.clone();
            damaged[bit / 8] ^= 1 << (bit % 8);
            assert!(!matches!(
                classify(&damaged, geometry),
                Classification::Valid(_)
            ));
            checked += 1;
        }
    }
    assert_eq!(checked, 16_384);
}

#[test]
fn q_refuses_repairable_invalid_media_and_accepts_proven_witnesses() {
    let geometry = Geometry::new(1024, 8).unwrap();
    let current_record = Record::new(7, led_snapshot(2000, 250, 1, 10, 7));
    let current = full_image(current_record, geometry);

    let mut pair_zero = full_image(Record::new(14, led_snapshot(1000, 500, 1, 9, 14)), geometry);
    let one_index = if pair_zero[0] == 0 { 1 } else { 0 };
    pair_zero[one_index] = 0;
    let pair_zero_class = classify(&pair_zero, geometry);
    assert_eq!(
        pair_zero_class,
        Classification::Corrupt(CorruptReason::BodyPair)
    );
    assert_eq!(
        qualify_erase(
            &pair_zero,
            geometry,
            &pair_zero_class,
            Some(&current_record)
        ),
        EraseQualification::Unsafe
    );

    let mut damaged_tail = current.clone();
    *damaged_tail.last_mut().unwrap() = 0xFE;
    let tail_class = classify(&damaged_tail, geometry);
    assert_eq!(
        tail_class,
        Classification::Corrupt(CorruptReason::PhysicalTail)
    );
    assert_eq!(
        qualify_erase(&damaged_tail, geometry, &tail_class, Some(&current_record)),
        EraseQualification::Unsafe
    );

    let blank = vec![0xFF; 1024];
    assert_eq!(
        qualify_erase(
            &blank,
            geometry,
            &Classification::Empty,
            Some(&current_record)
        ),
        EraseQualification::MarkerOne
    );

    let witness = include_bytes!("../evidence/r2-vectors/witness-data-rails.bin");
    let witness_class = classify(witness, geometry);
    assert_eq!(
        witness_class,
        Classification::Corrupt(CorruptReason::BodyPair)
    );
    assert_eq!(
        qualify_erase(witness, geometry, &witness_class, Some(&current_record)),
        EraseQualification::PairEleven
    );

    let subordinate = full_image(Record::new(6, led_snapshot(1000, 500, 1, 9, 6)), geometry);
    let subordinate_class = classify(&subordinate, geometry);
    assert_eq!(
        qualify_erase(
            &subordinate,
            geometry,
            &subordinate_class,
            Some(&current_record)
        ),
        EraseQualification::SubordinateValid
    );
}

#[test]
fn recovery_precedence_and_first_allocation_are_exact() {
    let blank = recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap();
    let new = led_snapshot(1000, 500, 4, 0, 1);
    assert_eq!(
        blank.plan(new, false),
        Ok(SavePlan::Write {
            target: Slot::A,
            record: Record::new(1, new),
        })
    );

    let a = Record::new(7, led_snapshot(2000, 250, 1, 10, 7));
    let b = Record::new(6, led_snapshot(1000, 500, 1, 9, 6));
    let selected = recover(
        Classification::Valid(a),
        Classification::Valid(b),
        &LedSchema,
    )
    .unwrap();
    assert!(matches!(
        selected.disposition,
        RecoveryDisposition::Selected(selected) if selected.slot == Slot::A && selected.record == a
    ));
    assert_eq!(
        selected.plan(a.snapshot, true),
        Ok(SavePlan::DurableExisting(selected.selected().unwrap()))
    );
    assert_eq!(
        selected.plan(led_snapshot(3000, 750, 1, 11, 8), false),
        Err(RecoveryError::RecoveryAuthorizationRequired)
    );

    let different = Record::new(7, led_snapshot(3000, 750, 1, 11, 8));
    let ambiguous = recover(
        Classification::Valid(a),
        Classification::Valid(different),
        &LedSchema,
    )
    .unwrap();
    assert!(matches!(
        ambiguous.disposition,
        RecoveryDisposition::Defaults { reasons, .. }
            if reasons.contains(bloxide_persistence::DefaultsReason::AMBIGUOUS)
    ));
    assert_eq!(ambiguous.plan(new, true), Err(RecoveryError::WritesLocked));

    let max = Record::new(u64::MAX, led_snapshot(2000, 250, 1, 10, 7));
    let exhausted = recover(
        Classification::Valid(max),
        Classification::Empty,
        &LedSchema,
    )
    .unwrap();
    assert!(matches!(
        exhausted.plan(max.snapshot, true),
        Ok(SavePlan::DurableExisting(_))
    ));
    assert_eq!(
        exhausted.plan(led_snapshot(3000, 750, 1, 11, 8), true),
        Err(RecoveryError::SequenceExhausted)
    );
    assert_eq!(FORMAT, 2);
}

#[test]
fn torn_marker_never_confers_authority() {
    let g1 = Geometry::new(1024, 1).unwrap();
    let g1_record = Record::new(1, led_snapshot(1000, 500, 1, 0, 1));
    let g1_committed = full_image(g1_record, g1);
    let g1_marker = usize::from(g1.marker_offset());
    for marker_value in 0_u8..=u8::MAX {
        let mut torn = g1_committed.clone();
        torn[g1_marker] = marker_value;
        if marker_value == 0 {
            assert!(matches!(
                classify(&torn, g1),
                Classification::Valid(decoded)
                    if decoded.sequence == 1
                        && decoded.is_same_durable_snapshot(&g1_record.snapshot)
            ));
        } else {
            assert_eq!(classify(&torn, g1), Classification::Uncommitted);
        }
    }

    let g8 = Geometry::new(1024, 8).unwrap();
    let g8_record = Record::new(1, led_snapshot(1000, 500, 1, 0, 1));
    let g8_committed = full_image(g8_record, g8);
    let g8_marker = usize::from(g8.marker_offset());
    for progress in 0..=8 {
        let mut torn = g8_committed.clone();
        torn[g8_marker..g8_marker + 8].fill(0xFF);
        torn[g8_marker..g8_marker + progress].fill(0);
        if progress == 8 {
            assert!(matches!(
                classify(&torn, g8),
                Classification::Valid(decoded)
                    if decoded.sequence == 1
                        && decoded.is_same_durable_snapshot(&g8_record.snapshot)
            ));
        } else {
            assert_eq!(classify(&torn, g8), Classification::Uncommitted);
        }
    }
}

#[derive(Clone, Copy)]
struct BytesSchema<const N: u16>;

impl<const N: u16> Schema for BytesSchema<N> {
    fn id(&self) -> u16 {
        1
    }

    fn payload_len(&self) -> u16 {
        N
    }

    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        out[..usize::from(N)].fill(0x5A);
        Ok(())
    }

    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError> {
        (bytes.len() == usize::from(N))
            .then_some(())
            .ok_or(SchemaError::InvalidValue)
    }
}

#[test]
fn every_payload_edge_and_supported_granule_round_trips() {
    for granule in [1, 2, 4, 8, 16, 32, 64, 128, 256] {
        let geometry = Geometry::new(4096, granule).unwrap();
        macro_rules! check_len {
            ($length:literal) => {{
                let schema = BytesSchema::<$length>;
                let bytes = [0x5A; $length as usize];
                let snapshot = Snapshot::new(&schema, &bytes, 3, 4, key(5)).unwrap();
                let record = Record::new(6, snapshot);
                let image = encode_record(&record, geometry);
                let mut full = vec![0xFF; geometry.erase_bytes() as usize];
                full[..image.bytes().len()].copy_from_slice(image.bytes());
                assert!(matches!(
                    classify_slot(
                        SlotRead { bytes: &full, issue: None },
                        geometry,
                        &schema,
                    ),
                    Classification::Valid(decoded)
                        if decoded.sequence == 6
                            && decoded.is_same_durable_snapshot(&snapshot)
                ));
            }};
        }
        check_len!(1);
        check_len!(4);
        check_len!(8);
        check_len!(255);
        check_len!(256);
    }
}

#[test]
fn format_matrix_and_led_validation_boundaries_are_explicit() {
    let geometry = Geometry::new(1024, 8).unwrap();
    for format in [0, 1, 2, 3, u16::MAX] {
        let record = Record {
            format,
            sequence: 7,
            snapshot: led_snapshot(1000, 500, 1, 9, 7),
        };
        let image = full_image(record, geometry);
        if format == 2 {
            assert!(matches!(
                classify(&image, geometry),
                Classification::Valid(_)
            ));
        } else {
            assert_eq!(
                classify(&image, geometry),
                Classification::Unsupported(UnsupportedKind::Format(format))
            );
        }
    }

    for (period, duty, valid) in [
        (99, 500, false),
        (100, 0, true),
        (1000, 500, true),
        (10_000, 1000, true),
        (10_001, 500, false),
        (1000, 1001, false),
    ] {
        let period: u16 = period;
        let duty: u16 = duty;
        let mut payload = [0_u8; 4];
        payload[..2].copy_from_slice(&period.to_le_bytes());
        payload[2..].copy_from_slice(&duty.to_le_bytes());
        assert_eq!(
            Snapshot::new(&LedSchema, &payload, 1, 0, key(1)).is_ok(),
            valid
        );
    }
}
