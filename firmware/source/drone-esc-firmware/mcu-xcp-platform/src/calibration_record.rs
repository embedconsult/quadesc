//! Portable whole-calibration record. All integers and IEEE754 values are LE.
//! The final 16-byte flash word is a commit footer, programmed last.
pub const CHANNELS: usize = 30;
pub const RECORD_BYTES: usize = 288;
pub type AdcCalibration = [[f32; 2]; CHANNELS];
const MAGIC: &[u8; 8] = b"ESCCAL\x02\0";
const COMMIT: &[u8; 8] = b"CALDONE\0";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    pub sequence: u64,
    pub revision: u32,
    pub period_ms: u16,
    pub duty_permille: u16,
    pub adc: AdcCalibration,
}
pub fn valid_adc(adc: &AdcCalibration) -> bool {
    adc.iter().all(|v| v[0].is_finite() && v[1].is_finite()
        && v[0].abs() <= 10_000.0 && v[1].abs() <= 1_000_000.0)
}
fn valid_led(period: u16, duty: u16) -> bool {
    (100..=10_000).contains(&period) && duty <= 1000
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1)); }
    }
    !crc
}
impl Record {
    pub fn encode(self) -> Option<[u8; RECORD_BYTES]> {
        if self.sequence == 0 || !valid_led(self.period_ms, self.duty_permille) || !valid_adc(&self.adc) { return None; }
        let mut b = [0u8; RECORD_BYTES];
        b[..8].copy_from_slice(MAGIC);
        b[8..16].copy_from_slice(&self.sequence.to_le_bytes());
        b[16..20].copy_from_slice(&self.revision.to_le_bytes());
        b[20..22].copy_from_slice(&self.period_ms.to_le_bytes());
        b[22..24].copy_from_slice(&self.duty_permille.to_le_bytes());
        b[24..26].copy_from_slice(&(CHANNELS as u16).to_le_bytes());
        for (i, values) in self.adc.iter().enumerate() {
            for (j, value) in values.iter().enumerate() {
                let start = 32 + i * 8 + j * 4;
                b[start..start+4].copy_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        b[272..280].copy_from_slice(COMMIT);
        let crc = crc32(&b[..280]);
        b[280..284].copy_from_slice(&crc.to_le_bytes());
        Some(b)
    }
    pub fn decode(b: &[u8; RECORD_BYTES]) -> Option<Self> {
        if &b[..8] != MAGIC || &b[272..280] != COMMIT || b[26..32] != [0; 6]
            || b[284..288] != [0; 4] || u16::from_le_bytes(b[24..26].try_into().ok()?) != CHANNELS as u16
            || crc32(&b[..280]) != u32::from_le_bytes(b[280..284].try_into().ok()?) { return None; }
        let mut adc = [[0.0; 2]; CHANNELS];
        for (i, values) in adc.iter_mut().enumerate() {
            for (j, value) in values.iter_mut().enumerate() {
                let start = 32 + i * 8 + j * 4;
                *value = f32::from_bits(u32::from_le_bytes(b[start..start+4].try_into().ok()?));
            }
        }
        let r = Self { sequence: u64::from_le_bytes(b[8..16].try_into().ok()?),
            revision: u32::from_le_bytes(b[16..20].try_into().ok()?),
            period_ms: u16::from_le_bytes(b[20..22].try_into().ok()?),
            duty_permille: u16::from_le_bytes(b[22..24].try_into().ok()?), adc };
        if r.sequence == 0 || !valid_led(r.period_ms, r.duty_permille) || !valid_adc(&r.adc) { return None; }
        Some(r)
    }
    /// Decode the previous sector-A LED-only format; never auto-writes migration.
    pub fn decode_legacy(b: &[u8; RECORD_BYTES], defaults: AdcCalibration) -> Option<Self> {
        if &b[..8] != b"LEDCAL\x01\0" || b[24..28] != [0; 4]
            || crc32(&b[..28]) != u32::from_le_bytes(b[28..32].try_into().ok()?) { return None; }
        let r = Self { sequence: u64::from_le_bytes(b[16..24].try_into().ok()?),
            revision: u32::from_le_bytes(b[12..16].try_into().ok()?),
            period_ms: u16::from_le_bytes(b[8..10].try_into().ok()?),
            duty_permille: u16::from_le_bytes(b[10..12].try_into().ok()?), adc: defaults };
        if r.sequence == 0 || !valid_led(r.period_ms, r.duty_permille) || !valid_adc(&r.adc) { return None; }
        Some(r)
    }
}
/// Slot index and the newest whole valid record; equal conflicting generations
/// are rejected. Never merge fields from two records or a torn new record.
pub fn select(a: &[u8; RECORD_BYTES], b: &[u8; RECORD_BYTES], defaults: AdcCalibration) -> Option<(usize, Record)> {
    let a = Record::decode(a).or_else(|| Record::decode_legacy(a, defaults));
    let b = Record::decode(b);
    match (a, b) {
        (Some(a), Some(b)) if a.sequence == b.sequence && a != b => None,
        (Some(a), Some(b)) if b.sequence > a.sequence => Some((1, b)),
        (Some(a), _) => Some((0, a)),
        (_, Some(b)) => Some((1, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(sequence: u64) -> Record { Record { sequence, revision: sequence as u32,
        period_ms: 100, duty_permille: 500, adc: [[0.25, -2.0]; CHANNELS] } }
    #[test] fn roundtrip_and_every_corrupt_byte_rejected() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let r = record(12); let bytes = r.encode().unwrap();
        assert_eq!(Record::decode(&bytes), Some(r));
        for i in 0..RECORD_BYTES { let mut bad = bytes; bad[i] ^= 1; assert_eq!(Record::decode(&bad), None); }
        assert_eq!(select(&[255; RECORD_BYTES], &[255; RECORD_BYTES], [[1.0,0.0]; CHANNELS]), None);
    }
    #[test] fn interrupted_updates_keep_old_whole_record_until_commit() {
        let old = record(1).encode().unwrap();
        let mut new = record(2); new.adc[0] = [1.5, 10.0]; new.period_ms = 700;
        let bytes = new.encode().unwrap();
        for n in 0..RECORD_BYTES {
            let mut torn = [255; RECORD_BYTES]; torn[..n].copy_from_slice(&bytes[..n]);
            assert_eq!(select(&old, &torn, [[1.0,0.0]; CHANNELS]), Some((0, record(1))));
        }
        assert_eq!(select(&old, &bytes, [[1.0,0.0]; CHANNELS]), Some((1, new)));
        assert_eq!(select(&[255; RECORD_BYTES], &bytes, [[1.0,0.0]; CHANNELS]), Some((1, new)));
    }
    #[test] fn legacy_migration_retains_led_until_explicit_new_commit() {
        let mut legacy = [255; RECORD_BYTES]; legacy[..32].fill(0);
        legacy[..8].copy_from_slice(b"LEDCAL\x01\0"); legacy[8..10].copy_from_slice(&100u16.to_le_bytes());
        legacy[10..12].copy_from_slice(&500u16.to_le_bytes()); legacy[12..16].copy_from_slice(&99u32.to_le_bytes());
        legacy[16..24].copy_from_slice(&9u64.to_le_bytes());
        let crc = crc32(&legacy[..28]); legacy[28..32].copy_from_slice(&crc.to_le_bytes());
        let defaults = [[0.0008, 0.0]; CHANNELS];
        let (_, migrated) = select(&legacy, &[255; RECORD_BYTES], defaults).unwrap();
        assert_eq!((migrated.period_ms,migrated.duty_permille,migrated.sequence,migrated.revision), (100,500,9,99));
        assert_eq!(migrated.adc, defaults);
        let committed = Record { sequence: 10, ..migrated }.encode().unwrap();
        assert_eq!(select(&legacy, &committed, defaults).unwrap().0, 1);
    }
    #[test] fn invalid_calibration_and_conflicting_generations_rejected() {
        for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 1_000_001.0] {
            let mut r = record(1); r.adc[0][0] = value; assert!(r.encode().is_none());
            r = record(1); r.adc[29][1] = value; assert!(r.encode().is_none());
        }
        let a = record(1); let mut b = a; b.adc[1][0] = 2.0;
        assert_eq!(select(&a.encode().unwrap(), &b.encode().unwrap(), [[1.0,0.0]; CHANNELS]), None);
        let mut bad = a.encode().unwrap(); bad[32..36].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
        let crc = crc32(&bad[..280]); bad[280..284].copy_from_slice(&crc.to_le_bytes());
        assert!(Record::decode(&bad).is_none());
    }
}
