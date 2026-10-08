//! Owned host-side exchange schema. Firmware uses compile-time descriptors and
//! never parses this JSON representation.

use serde::{Deserialize, Serialize};
use std::{string::String, vec::Vec};

use crate::{
    Access, ByteOrder, MappedVariable, NumericLimits, Registry, ScalarValue, WireType, WritePolicy,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescriptorDocument {
    pub format: u16,
    pub variables: Vec<ExchangeVariable>,
}

impl DescriptorDocument {
    #[must_use]
    pub fn from_registry<const N: usize>(registry: &Registry<N>) -> Self {
        Self {
            format: 1,
            variables: registry
                .variables()
                .iter()
                .map(ExchangeVariable::from)
                .collect(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(value: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeVariable {
    pub variable_id: u32,
    pub symbol: String,
    pub owner: String,
    pub description: String,
    pub wire_type: ExchangeWireType,
    pub byte_order: ExchangeByteOrder,
    pub unit: String,
    pub scaling: ExchangeScaling,
    pub limits: Option<ExchangeLimits>,
    pub default: Option<ExchangeScalar>,
    pub access: ExchangeAccess,
    pub write_policy: ExchangeWritePolicy,
    pub schema_version: u16,
    pub address_extension: u8,
    pub address: u32,
}

impl From<&MappedVariable> for ExchangeVariable {
    fn from(mapped: &MappedVariable) -> Self {
        let descriptor = mapped.descriptor();
        let scaling = descriptor.scaling();
        Self {
            variable_id: descriptor.variable_id(),
            symbol: descriptor.symbol().into(),
            owner: descriptor.owner().into(),
            description: descriptor.description().into(),
            wire_type: descriptor.wire_type().into(),
            byte_order: descriptor.byte_order().into(),
            unit: descriptor.unit().into(),
            scaling: ExchangeScaling {
                numerator: scaling.numerator(),
                denominator: scaling.denominator(),
                offset: scaling.offset(),
            },
            limits: descriptor.limits().map(Into::into),
            default: descriptor.default().map(Into::into),
            access: descriptor.access().into(),
            write_policy: descriptor.write_policy().into(),
            schema_version: descriptor.schema_version(),
            address_extension: mapped.address_extension(),
            address: mapped.address(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeWireType {
    U8,
    U16,
    U32,
    I16,
    I32,
    F32,
    Blob { width: u8 },
}

impl From<WireType> for ExchangeWireType {
    fn from(value: WireType) -> Self {
        match value {
            WireType::U8 => Self::U8,
            WireType::U16 => Self::U16,
            WireType::U32 => Self::U32,
            WireType::I16 => Self::I16,
            WireType::I32 => Self::I32,
            WireType::F32 => Self::F32,
            WireType::Blob { width } => Self::Blob { width },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeByteOrder {
    Little,
}

impl From<ByteOrder> for ExchangeByteOrder {
    fn from(value: ByteOrder) -> Self {
        match value {
            ByteOrder::Little => Self::Little,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeAccess {
    CalibrationReadWrite,
    MeasurementReadOnly,
    MetadataReadOnly,
}

impl From<Access> for ExchangeAccess {
    fn from(value: Access) -> Self {
        match value {
            Access::CalibrationReadWrite => Self::CalibrationReadWrite,
            Access::MeasurementReadOnly => Self::MeasurementReadOnly,
            Access::MetadataReadOnly => Self::MetadataReadOnly,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeWritePolicy {
    CompleteScalar,
    ReadOnly,
}

impl From<WritePolicy> for ExchangeWritePolicy {
    fn from(value: WritePolicy) -> Self {
        match value {
            WritePolicy::CompleteScalar => Self::CompleteScalar,
            WritePolicy::ReadOnly => Self::ReadOnly,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeScaling {
    pub numerator: i64,
    pub denominator: u64,
    pub offset: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeLimits {
    pub minimum: ExchangeScalar,
    pub maximum: ExchangeScalar,
}

impl From<NumericLimits> for ExchangeLimits {
    fn from(value: NumericLimits) -> Self {
        Self {
            minimum: value.minimum().into(),
            maximum: value.maximum().into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ExchangeScalar {
    U8(u8),
    U16(u16),
    U32(u32),
    I16(i16),
    I32(i32),
    F32(f32),
}

impl From<ScalarValue> for ExchangeScalar {
    fn from(value: ScalarValue) -> Self {
        match value {
            ScalarValue::U8(value) => Self::U8(value),
            ScalarValue::U16(value) => Self::U16(value),
            ScalarValue::U32(value) => Self::U32(value),
            ScalarValue::I16(value) => Self::I16(value),
            ScalarValue::I32(value) => Self::I32(value),
            ScalarValue::F32(value) => Self::F32(value),
        }
    }
}
