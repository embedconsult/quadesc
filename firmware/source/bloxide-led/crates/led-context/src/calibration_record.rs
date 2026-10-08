//! Minimal sector-A LED calibration format. All fields little endian.
//! 0..8 magic/version, 8..12 period/duty, 12..16 owner revision,
//! 16..24 sequence, 24..28 reserved zero, 28..32 IEEE CRC32 over 0..28.
use led_messages::LedConfig;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationRecord { pub config: LedConfig, pub revision: u32, pub sequence: u64 }
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes { crc ^= byte as u32; for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1))); } }
    !crc
}
impl CalibrationRecord {
    pub fn encode(self) -> [u8;32] {
        let mut b = [0;32]; b[..8].copy_from_slice(b"LEDCAL\x01\0");
        b[8..10].copy_from_slice(&self.config.period_ms().to_le_bytes());
        b[10..12].copy_from_slice(&self.config.duty_permille().to_le_bytes());
        b[12..16].copy_from_slice(&self.revision.to_le_bytes());
        b[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        let crc = crc32(&b[..28]); b[28..].copy_from_slice(&crc.to_le_bytes()); b
    }
    pub fn decode(b: [u8;32]) -> Option<Self> {
        if &b[..8] != b"LEDCAL\x01\0" || b[24..28] != [0;4] || crc32(&b[..28]) != u32::from_le_bytes(b[28..].try_into().ok()?) { return None; }
        let config = LedConfig::new(u16::from_le_bytes(b[8..10].try_into().ok()?), u16::from_le_bytes(b[10..12].try_into().ok()?)).ok()?;
        let sequence = u64::from_le_bytes(b[16..24].try_into().ok()?); if sequence == 0 { return None; }
        Some(Self { config, revision: u32::from_le_bytes(b[12..16].try_into().ok()?), sequence })
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn restore_and_reject_corruption_or_partial_program() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let r = CalibrationRecord { config: LedConfig::new(1700,250).unwrap(), revision: 2, sequence: 1 };
        let b=r.encode(); assert_eq!(CalibrationRecord::decode(b),Some(r));
        for i in 0..32 { let mut bad=b;bad[i]^=1;assert_eq!(CalibrationRecord::decode(bad),None); }
        let mut partial=b;partial[16..].fill(255);assert_eq!(CalibrationRecord::decode(partial),None);
        assert_eq!(CalibrationRecord::decode([255;32]),None);
    }
}
