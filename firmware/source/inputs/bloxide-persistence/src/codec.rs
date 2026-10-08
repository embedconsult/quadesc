use crate::{
    BODY_BYTES, ENCODED_BODY_BYTES, FORMAT, Geometry, HEADER_BYTES, MAGIC, MAX_PAYLOAD_BYTES,
    OperationKey, Record, Schema, SlotImage, Snapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadIssue {
    Io,
    CorrectedEcc,
    UncorrectableEcc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotRead<'a> {
    pub bytes: &'a [u8],
    pub issue: Option<ReadIssue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorruptReason {
    Size,
    BodyPair,
    Magic,
    ZeroSchema,
    Length,
    ZeroSequence,
    Reserved,
    LogicalPadding,
    PhysicalPadding,
    PhysicalTail,
    Crc,
    SchemaLength,
    Semantic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedKind {
    Format(u16),
    Schema(u16),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Classification {
    Empty,
    Uncommitted,
    Corrupt(CorruptReason),
    Unsupported(UnsupportedKind),
    Unreadable(ReadIssue),
    Valid(Record),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EraseQualification {
    MarkerOne,
    PairEleven,
    CanonicalCorrupt,
    SubordinateValid,
    ExactDuplicate,
    Unsafe,
}

impl EraseQualification {
    #[must_use]
    pub const fn is_eligible(self) -> bool {
        !matches!(self, Self::Unsafe)
    }
}

#[must_use]
pub const fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    let mut index = 0;
    while index < bytes.len() {
        crc ^= bytes[index] as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x82F6_3B78
            } else {
                crc >> 1
            };
            bit += 1;
        }
        index += 1;
    }
    !crc
}

fn record_crc(body: &[u8; BODY_BYTES]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in body[..60].iter().chain(body[64..].iter()) {
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

#[must_use]
pub fn encode_record(record: &Record, geometry: Geometry) -> SlotImage {
    let mut logical = [0xFF; BODY_BYTES];
    logical[..8].copy_from_slice(&MAGIC);
    logical[8..10].copy_from_slice(&record.format.to_le_bytes());
    logical[10..12].copy_from_slice(&record.snapshot.schema().to_le_bytes());
    logical[12..16].copy_from_slice(&u32::from(record.snapshot.payload_len()).to_le_bytes());
    logical[16..24].copy_from_slice(&record.sequence.to_le_bytes());
    logical[24..32].copy_from_slice(&record.snapshot.source_epoch().to_le_bytes());
    logical[32..36].copy_from_slice(&record.snapshot.source_revision().to_le_bytes());
    logical[36..40].copy_from_slice(&record.snapshot.operation().session_generation.to_le_bytes());
    logical[40..48].copy_from_slice(&record.snapshot.operation().sequence.to_le_bytes());
    logical[48..60].fill(0);
    let payload_end = HEADER_BYTES + usize::from(record.snapshot.payload_len());
    logical[HEADER_BYTES..payload_end].copy_from_slice(record.snapshot.bytes());
    let crc = record_crc(&logical);
    logical[60..64].copy_from_slice(&crc.to_le_bytes());

    let mut image = SlotImage::erased(geometry);
    for (index, byte) in logical.into_iter().enumerate() {
        image.bytes_mut()[index * 2] = byte;
        image.bytes_mut()[index * 2 + 1] = !byte;
    }
    let marker = usize::from(geometry.marker_offset());
    let marker_end = marker + usize::from(geometry.granule_bytes());
    image.bytes_mut()[marker..marker_end].fill(0);
    image
}

pub fn decode_body(bytes: &[u8]) -> Result<[u8; BODY_BYTES], CorruptReason> {
    if bytes.len() < ENCODED_BODY_BYTES {
        return Err(CorruptReason::Size);
    }
    let mut logical = [0_u8; BODY_BYTES];
    for (index, pair) in bytes[..ENCODED_BODY_BYTES].chunks_exact(2).enumerate() {
        if pair[0] ^ pair[1] != 0xFF {
            return Err(CorruptReason::BodyPair);
        }
        logical[index] = pair[0];
    }
    Ok(logical)
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[must_use]
pub fn classify_slot<S: Schema>(
    read: SlotRead<'_>,
    geometry: Geometry,
    schema: &S,
) -> Classification {
    if read.bytes.len() != geometry.erase_bytes() as usize {
        return Classification::Corrupt(CorruptReason::Size);
    }
    let span = usize::from(geometry.programmed_span());
    classify_scanned(
        &read.bytes[..span],
        read.bytes.iter().all(|&byte| byte == 0xFF),
        read.bytes[span..].iter().all(|&byte| byte == 0xFF),
        read.issue,
        geometry,
        schema,
    )
}

pub(crate) fn classify_scanned<S: Schema>(
    prefix: &[u8],
    all_ff: bool,
    tail_ff: bool,
    issue: Option<ReadIssue>,
    geometry: Geometry,
    schema: &S,
) -> Classification {
    if let Some(issue) = issue {
        return Classification::Unreadable(issue);
    }
    if prefix.len() != usize::from(geometry.programmed_span()) {
        return Classification::Corrupt(CorruptReason::Size);
    }
    if all_ff {
        return Classification::Empty;
    }
    let marker = usize::from(geometry.marker_offset());
    let marker_end = marker + usize::from(geometry.granule_bytes());
    if prefix[marker..marker_end].iter().any(|&byte| byte != 0) {
        return Classification::Uncommitted;
    }
    classify_body_scanned(prefix, tail_ff, geometry, schema)
}

pub(crate) fn classify_body_scanned<S: Schema>(
    prefix: &[u8],
    tail_ff: bool,
    geometry: Geometry,
    schema: &S,
) -> Classification {
    let logical = match decode_body(prefix) {
        Ok(logical) => logical,
        Err(reason) => return Classification::Corrupt(reason),
    };
    if logical[..8] != MAGIC {
        return Classification::Corrupt(CorruptReason::Magic);
    }
    let format = le_u16(&logical[8..10]);
    let schema_id = le_u16(&logical[10..12]);
    if schema_id == 0 {
        return Classification::Corrupt(CorruptReason::ZeroSchema);
    }
    let length_u32 = le_u32(&logical[12..16]);
    if length_u32 == 0 || length_u32 > 256 {
        return Classification::Corrupt(CorruptReason::Length);
    }
    let sequence = le_u64(&logical[16..24]);
    if sequence == 0 {
        return Classification::Corrupt(CorruptReason::ZeroSequence);
    }
    if logical[48..60].iter().any(|&byte| byte != 0) {
        return Classification::Corrupt(CorruptReason::Reserved);
    }
    let length = length_u32 as usize;
    if logical[HEADER_BYTES + length..]
        .iter()
        .any(|&byte| byte != 0xFF)
    {
        return Classification::Corrupt(CorruptReason::LogicalPadding);
    }
    if prefix[ENCODED_BODY_BYTES..usize::from(geometry.body_span())]
        .iter()
        .any(|&byte| byte != 0xFF)
    {
        return Classification::Corrupt(CorruptReason::PhysicalPadding);
    }
    if !tail_ff {
        return Classification::Corrupt(CorruptReason::PhysicalTail);
    }
    if le_u32(&logical[60..64]) != record_crc(&logical) {
        return Classification::Corrupt(CorruptReason::Crc);
    }
    if format != FORMAT {
        return Classification::Unsupported(UnsupportedKind::Format(format));
    }
    if schema_id != schema.id() {
        return Classification::Unsupported(UnsupportedKind::Schema(schema_id));
    }
    if length_u32 != u32::from(schema.payload_len()) {
        return Classification::Corrupt(CorruptReason::SchemaLength);
    }
    if schema
        .validate(&logical[HEADER_BYTES..HEADER_BYTES + length])
        .is_err()
    {
        return Classification::Corrupt(CorruptReason::Semantic);
    }
    let mut payload = [0xFF; MAX_PAYLOAD_BYTES];
    payload[..length].copy_from_slice(&logical[HEADER_BYTES..HEADER_BYTES + length]);
    let snapshot = Snapshot::new(
        schema,
        &payload[..length],
        le_u64(&logical[24..32]),
        le_u32(&logical[32..36]),
        OperationKey {
            service_epoch: 0,
            session_generation: le_u32(&logical[36..40]),
            sequence: le_u64(&logical[40..48]),
        },
    );
    let Ok(snapshot) = snapshot else {
        return Classification::Corrupt(CorruptReason::Semantic);
    };
    // The persistence service epoch is intentionally absent from the durable
    // envelope. A boot decoder uses zero; only session generation and sequence
    // are historical request labels in the record.
    Classification::Valid(Record {
        format,
        sequence,
        snapshot,
    })
}

#[must_use]
pub fn qualify_erase(
    bytes: &[u8],
    geometry: Geometry,
    classification: &Classification,
    selected: Option<&Record>,
) -> EraseQualification {
    if bytes.len() != geometry.erase_bytes() as usize {
        return EraseQualification::Unsafe;
    }
    let span = usize::from(geometry.programmed_span());
    qualify_scanned(
        &bytes[..span],
        bytes[span..].iter().all(|&byte| byte == 0xFF),
        geometry,
        classification,
        selected,
    )
}

pub(crate) fn qualify_scanned(
    prefix: &[u8],
    tail_ff: bool,
    geometry: Geometry,
    classification: &Classification,
    selected: Option<&Record>,
) -> EraseQualification {
    if prefix.len() != usize::from(geometry.programmed_span()) {
        return EraseQualification::Unsafe;
    }
    let marker = usize::from(geometry.marker_offset());
    let marker_end = marker + usize::from(geometry.granule_bytes());
    if prefix[marker..marker_end].iter().any(|&byte| byte != 0) {
        return EraseQualification::MarkerOne;
    }
    if prefix[..ENCODED_BODY_BYTES]
        .chunks_exact(2)
        .any(|pair| pair[0] & pair[1] != 0)
    {
        return EraseQualification::PairEleven;
    }
    if decode_body(prefix).is_err()
        || prefix[ENCODED_BODY_BYTES..usize::from(geometry.body_span())]
            .iter()
            .any(|&byte| byte != 0xFF)
        || !tail_ff
    {
        return EraseQualification::Unsafe;
    }
    match (classification, selected) {
        (Classification::Corrupt(_), _) => EraseQualification::CanonicalCorrupt,
        (Classification::Valid(target), Some(current)) if target.sequence < current.sequence => {
            EraseQualification::SubordinateValid
        }
        (Classification::Valid(target), Some(current))
            if target.persistent_identity_eq(current) =>
        {
            EraseQualification::ExactDuplicate
        }
        _ => EraseQualification::Unsafe,
    }
}
