use crate::{ENCODED_BODY_BYTES, MAX_IMAGE_BYTES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryError {
    GranuleOutOfRange,
    GranuleNotPowerOfTwo,
    EraseOutOfRange,
    EraseNotGranuleMultiple,
    RecordDoesNotFit,
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    erase_bytes: u32,
    granule_bytes: u16,
    body_span: u16,
}

impl Geometry {
    #[allow(clippy::cast_possible_truncation)]
    pub const fn new(erase_bytes: u32, granule_bytes: u16) -> Result<Self, GeometryError> {
        if granule_bytes == 0 || granule_bytes > 256 {
            return Err(GeometryError::GranuleOutOfRange);
        }
        if !granule_bytes.is_power_of_two() {
            return Err(GeometryError::GranuleNotPowerOfTwo);
        }
        if erase_bytes < 512 || erase_bytes > 65_536 {
            return Err(GeometryError::EraseOutOfRange);
        }
        if !erase_bytes.is_multiple_of(granule_bytes as u32) {
            return Err(GeometryError::EraseNotGranuleMultiple);
        }
        let g = granule_bytes as usize;
        let Some(sum) = ENCODED_BODY_BYTES.checked_add(g - 1) else {
            return Err(GeometryError::ArithmeticOverflow);
        };
        let body_span = (sum / g) * g;
        let Some(programmed_span) = body_span.checked_add(g) else {
            return Err(GeometryError::ArithmeticOverflow);
        };
        if programmed_span > erase_bytes as usize || programmed_span > MAX_IMAGE_BYTES {
            return Err(GeometryError::RecordDoesNotFit);
        }
        Ok(Self {
            erase_bytes,
            granule_bytes,
            body_span: body_span as u16,
        })
    }

    #[must_use]
    pub const fn erase_bytes(self) -> u32 {
        self.erase_bytes
    }

    #[must_use]
    pub const fn granule_bytes(self) -> u16 {
        self.granule_bytes
    }

    #[must_use]
    pub const fn body_span(self) -> u16 {
        self.body_span
    }

    #[must_use]
    pub const fn marker_offset(self) -> u16 {
        self.body_span
    }

    #[must_use]
    pub const fn programmed_span(self) -> u16 {
        self.body_span + self.granule_bytes
    }

    #[must_use]
    pub const fn body_program_count(self) -> u16 {
        self.body_span / self.granule_bytes
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn read_count(self) -> u16 {
        self.erase_bytes.div_ceil(256) as u16
    }

    #[must_use]
    pub const fn normal_save_commands(self) -> u32 {
        3 + self.body_program_count() as u32 + 4 * self.read_count() as u32
    }
}
