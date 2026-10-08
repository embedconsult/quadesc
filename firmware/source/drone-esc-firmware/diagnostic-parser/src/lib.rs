#![no_std]
//! Pure bounded target diagnostic parser; also compiled into host contract tests.
use led_messages::{DUTY_ID, PERIOD_ID};
pub struct Line {
    bytes: [u8; 64],
    len: usize,
    overflow: bool,
}
impl Line {
    pub const fn new() -> Self {
        Self {
            bytes: [0; 64],
            len: 0,
            overflow: false,
        }
    }
    pub fn clear(&mut self) {
        self.len = 0;
        self.overflow = false;
    }
    pub fn push(&mut self, byte: u8) {
        if self.len == self.bytes.len() {
            self.overflow = true;
        } else {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }
    fn text(&self) -> Option<&[u8]> {
        if self.overflow || self.len == 0 || self.bytes[self.len - 1] != b'\n' {
            return None;
        }
        let mut end = self.len - 1;
        if end != 0 && self.bytes[end - 1] == b'\r' {
            end -= 1;
        }
        Some(&self.bytes[..end])
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Read,
    Apply(u32, u16),
    Save,
    Status,
    Flash,
    Invalid,
}
fn decimal(bytes: &[u8]) -> Option<u16> {
    if bytes.is_empty() {
        return None;
    }
    bytes.iter().try_fold(0u16, |n, b| {
        if !b.is_ascii_digit() {
            None
        } else {
            n.checked_mul(10)?.checked_add((b - b'0') as u16)
        }
    })
}
pub fn parse(line: &Line) -> Command {
    let Some(bytes) = line.text() else {
        return Command::Invalid;
    };
    match bytes {
        b"GET" => return Command::Read,
        b"SAVE" => return Command::Save,
        b"STATUS" => return Command::Status,
        b"FLASH" => return Command::Flash,
        _ => {}
    }
    if let Some(digits) = bytes.strip_prefix(b"SET PERIOD ") {
        return match decimal(digits) {
            Some(v @ 100..=10_000) => Command::Apply(PERIOD_ID, v),
            _ => Command::Invalid,
        };
    }
    if let Some(digits) = bytes.strip_prefix(b"SET DUTY ") {
        return match decimal(digits) {
            Some(v @ 0..=1_000) => Command::Apply(DUTY_ID, v),
            _ => Command::Invalid,
        };
    }
    Command::Invalid
}
