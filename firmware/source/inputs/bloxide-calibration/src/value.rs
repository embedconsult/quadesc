use core::marker::PhantomData;

use crate::{EncodedError, EncodedValue, ScalarValue, WireType};

/// Compile-time unit marker for validated quantities.
pub trait Unit {
    const SYMBOL: &'static str;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unitless;
impl Unit for Unitless {
    const SYMBOL: &'static str = "1";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds;
impl Unit for Milliseconds {
    const SYMBOL: &'static str = "ms";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Permille;
impl Unit for Permille {
    const SYMBOL: &'static str = "permille";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Celsius;
impl Unit for Celsius {
    const SYMBOL: &'static str = "degC";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueError {
    InvalidTypeBounds,
    OutOfBounds,
    NonFinite,
    Encoding(EncodedError),
}

impl From<EncodedError> for ValueError {
    fn from(value: EncodedError) -> Self {
        Self::Encoding(value)
    }
}

/// Common behavior of private, validated scalar representations.
pub trait ValidatedScalar: Copy + Eq {
    const WIRE_TYPE: WireType;
    const UNIT: &'static str;

    fn scalar(self) -> ScalarValue;
    fn encode(self) -> EncodedValue;
}

/// A unit-aware `u16` whose representation cannot be constructed unchecked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedU16<U: Unit, const MIN: u16, const MAX: u16> {
    value: u16,
    unit: PhantomData<U>,
}

impl<U: Unit, const MIN: u16, const MAX: u16> BoundedU16<U, MIN, MAX> {
    pub fn try_new(value: u16) -> Result<Self, ValueError> {
        if MIN > MAX {
            return Err(ValueError::InvalidTypeBounds);
        }
        if !(MIN..=MAX).contains(&value) {
            return Err(ValueError::OutOfBounds);
        }
        Ok(Self {
            value,
            unit: PhantomData,
        })
    }

    pub fn try_decode(value: EncodedValue) -> Result<Self, ValueError> {
        Self::try_new(u16::from_le_bytes(value.exact()?))
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.value
    }
}

impl<U: Unit + Eq + Copy, const MIN: u16, const MAX: u16> ValidatedScalar
    for BoundedU16<U, MIN, MAX>
{
    const WIRE_TYPE: WireType = WireType::U16;
    const UNIT: &'static str = U::SYMBOL;

    fn scalar(self) -> ScalarValue {
        ScalarValue::U16(self.value)
    }

    fn encode(self) -> EncodedValue {
        EncodedValue::try_from_slice(&self.value.to_le_bytes()).expect("u16 always fits")
    }
}

/// A unit-aware `u32` whose representation cannot be constructed unchecked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedU32<U: Unit, const MIN: u32, const MAX: u32> {
    value: u32,
    unit: PhantomData<U>,
}

impl<U: Unit, const MIN: u32, const MAX: u32> BoundedU32<U, MIN, MAX> {
    pub fn try_new(value: u32) -> Result<Self, ValueError> {
        if MIN > MAX {
            return Err(ValueError::InvalidTypeBounds);
        }
        if !(MIN..=MAX).contains(&value) {
            return Err(ValueError::OutOfBounds);
        }
        Ok(Self {
            value,
            unit: PhantomData,
        })
    }

    pub fn try_decode(value: EncodedValue) -> Result<Self, ValueError> {
        Self::try_new(u32::from_le_bytes(value.exact()?))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.value
    }
}

impl<U: Unit + Eq + Copy, const MIN: u32, const MAX: u32> ValidatedScalar
    for BoundedU32<U, MIN, MAX>
{
    const WIRE_TYPE: WireType = WireType::U32;
    const UNIT: &'static str = U::SYMBOL;

    fn scalar(self) -> ScalarValue {
        ScalarValue::U32(self.value)
    }

    fn encode(self) -> EncodedValue {
        EncodedValue::try_from_slice(&self.value.to_le_bytes()).expect("u32 always fits")
    }
}

/// A unit-aware `i32` whose representation cannot be constructed unchecked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedI32<U: Unit, const MIN: i32, const MAX: i32> {
    value: i32,
    unit: PhantomData<U>,
}

impl<U: Unit, const MIN: i32, const MAX: i32> BoundedI32<U, MIN, MAX> {
    pub fn try_new(value: i32) -> Result<Self, ValueError> {
        if MIN > MAX {
            return Err(ValueError::InvalidTypeBounds);
        }
        if !(MIN..=MAX).contains(&value) {
            return Err(ValueError::OutOfBounds);
        }
        Ok(Self {
            value,
            unit: PhantomData,
        })
    }

    pub fn try_decode(value: EncodedValue) -> Result<Self, ValueError> {
        Self::try_new(i32::from_le_bytes(value.exact()?))
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.value
    }
}

impl<U: Unit + Eq + Copy, const MIN: i32, const MAX: i32> ValidatedScalar
    for BoundedI32<U, MIN, MAX>
{
    const WIRE_TYPE: WireType = WireType::I32;
    const UNIT: &'static str = U::SYMBOL;

    fn scalar(self) -> ScalarValue {
        ScalarValue::I32(self.value)
    }

    fn encode(self) -> EncodedValue {
        EncodedValue::try_from_slice(&self.value.to_le_bytes()).expect("i32 always fits")
    }
}

/// Defines a named bounded finite `f32` type with a private representation.
///
/// The generated type implements `Eq` because construction rejects every NaN.
#[macro_export]
macro_rules! define_bounded_f32 {
    ($visibility:vis $name:ident, unit = $unit:ty, min = $min:expr, max = $max:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        $visibility struct $name {
            value: f32,
            unit: ::core::marker::PhantomData<$unit>,
        }

        impl ::core::cmp::Eq for $name {}

        impl $name {
            pub fn try_new(value: f32) -> Result<Self, $crate::ValueError> {
                const MIN: f32 = $min;
                const MAX: f32 = $max;
                if !MIN.is_finite() || !MAX.is_finite() || MIN > MAX {
                    return Err($crate::ValueError::InvalidTypeBounds);
                }
                if !value.is_finite() {
                    return Err($crate::ValueError::NonFinite);
                }
                if !(MIN..=MAX).contains(&value) {
                    return Err($crate::ValueError::OutOfBounds);
                }
                Ok(Self {
                    value,
                    unit: ::core::marker::PhantomData,
                })
            }

            pub fn try_decode(value: $crate::EncodedValue) -> Result<Self, $crate::ValueError> {
                Self::try_new(f32::from_le_bytes(value.exact()?))
            }

            #[must_use]
            pub const fn get(self) -> f32 {
                self.value
            }
        }

        impl $crate::ValidatedScalar for $name {
            const WIRE_TYPE: $crate::WireType = $crate::WireType::F32;
            const UNIT: &'static str = <$unit as $crate::Unit>::SYMBOL;

            fn scalar(self) -> $crate::ScalarValue {
                $crate::ScalarValue::F32(self.value)
            }

            fn encode(self) -> $crate::EncodedValue {
                $crate::EncodedValue::try_from_slice(&self.value.to_le_bytes())
                    .expect("f32 always fits")
            }
        }
    };
}
