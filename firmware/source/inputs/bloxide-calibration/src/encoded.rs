/// Errors produced while constructing or decoding a fixed scalar payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodedError {
    Empty,
    TooWide,
    WrongWidth { expected: u8, actual: u8 },
}

/// An owned, bounded encoded scalar value.
///
/// Four bytes cover every scalar in the S0 contract. The private length keeps
/// zero padding from being interpreted as part of the value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedValue {
    bytes: [u8; 4],
    len: u8,
}

impl EncodedValue {
    pub fn try_from_slice(value: &[u8]) -> Result<Self, EncodedError> {
        if value.is_empty() {
            return Err(EncodedError::Empty);
        }
        if value.len() > 4 {
            return Err(EncodedError::TooWide);
        }

        let mut bytes = [0; 4];
        bytes[..value.len()].copy_from_slice(value);
        Ok(Self {
            bytes,
            len: value.len() as u8,
        })
    }

    #[must_use]
    pub const fn len(self) -> u8 {
        self.len
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    pub fn exact<const N: usize>(&self) -> Result<[u8; N], EncodedError> {
        if usize::from(self.len) != N {
            return Err(EncodedError::WrongWidth {
                expected: N as u8,
                actual: self.len,
            });
        }
        let mut bytes = [0; N];
        bytes.copy_from_slice(self.as_slice());
        Ok(bytes)
    }
}
