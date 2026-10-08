use core::cmp::Ordering;

use crate::EncodedValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireType {
    U8,
    U16,
    U32,
    I16,
    I32,
    F32,
    Blob { width: u8 },
}

impl WireType {
    #[must_use]
    pub const fn width(self) -> u8 {
        match self {
            Self::U8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::Blob { width } => width,
        }
    }

    #[must_use]
    pub const fn is_scalar(self) -> bool {
        !matches!(self, Self::Blob { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteOrder {
    Little,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    CalibrationReadWrite,
    MeasurementReadOnly,
    MetadataReadOnly,
}

impl Access {
    #[must_use]
    pub const fn is_writable(self) -> bool {
        matches!(self, Self::CalibrationReadWrite)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritePolicy {
    CompleteScalar,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Scaling {
    numerator: i64,
    denominator: u64,
    offset: i64,
}

impl Scaling {
    pub const IDENTITY: Self = Self {
        numerator: 1,
        denominator: 1,
        offset: 0,
    };

    pub const fn try_new(
        numerator: i64,
        denominator: u64,
        offset: i64,
    ) -> Result<Self, DescriptorError> {
        if denominator == 0 || numerator == 0 {
            return Err(DescriptorError::InvalidScaling);
        }
        Ok(Self {
            numerator,
            denominator,
            offset,
        })
    }

    #[must_use]
    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u64 {
        self.denominator
    }

    #[must_use]
    pub const fn offset(self) -> i64 {
        self.offset
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScalarValue {
    U8(u8),
    U16(u16),
    U32(u32),
    I16(i16),
    I32(i32),
    F32(f32),
}

impl ScalarValue {
    #[must_use]
    pub const fn wire_type(self) -> WireType {
        match self {
            Self::U8(_) => WireType::U8,
            Self::U16(_) => WireType::U16,
            Self::U32(_) => WireType::U32,
            Self::I16(_) => WireType::I16,
            Self::I32(_) => WireType::I32,
            Self::F32(_) => WireType::F32,
        }
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        match self {
            Self::F32(value) => value.is_finite(),
            _ => true,
        }
    }

    fn partial_cmp_same(self, other: Self) -> Option<Ordering> {
        match (self, other) {
            (Self::U8(left), Self::U8(right)) => left.partial_cmp(&right),
            (Self::U16(left), Self::U16(right)) => left.partial_cmp(&right),
            (Self::U32(left), Self::U32(right)) => left.partial_cmp(&right),
            (Self::I16(left), Self::I16(right)) => left.partial_cmp(&right),
            (Self::I32(left), Self::I32(right)) => left.partial_cmp(&right),
            (Self::F32(left), Self::F32(right)) => left.partial_cmp(&right),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumericLimits {
    minimum: ScalarValue,
    maximum: ScalarValue,
}

impl NumericLimits {
    pub fn try_new(minimum: ScalarValue, maximum: ScalarValue) -> Result<Self, DescriptorError> {
        if !minimum.is_finite() || !maximum.is_finite() {
            return Err(DescriptorError::NonFinite);
        }
        if minimum.partial_cmp_same(maximum) != Some(Ordering::Less)
            && minimum.partial_cmp_same(maximum) != Some(Ordering::Equal)
        {
            return Err(DescriptorError::InvalidLimits);
        }
        Ok(Self { minimum, maximum })
    }

    #[must_use]
    pub const fn minimum(self) -> ScalarValue {
        self.minimum
    }

    #[must_use]
    pub const fn maximum(self) -> ScalarValue {
        self.maximum
    }

    #[must_use]
    pub fn contains(self, value: ScalarValue) -> bool {
        value.is_finite()
            && matches!(
                (
                    self.minimum.partial_cmp_same(value),
                    value.partial_cmp_same(self.maximum)
                ),
                (
                    Some(Ordering::Less | Ordering::Equal),
                    Some(Ordering::Less | Ordering::Equal)
                )
            )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DescriptorSpec {
    pub variable_id: u32,
    pub symbol: &'static str,
    pub owner: &'static str,
    pub description: &'static str,
    pub wire_type: WireType,
    pub byte_order: ByteOrder,
    pub unit: &'static str,
    pub scaling: Scaling,
    pub limits: Option<NumericLimits>,
    pub default: Option<ScalarValue>,
    pub access: Access,
    pub write_policy: WritePolicy,
    pub schema_version: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    EmptySymbol,
    EmptyOwner,
    EmptyUnit,
    ZeroWidth,
    BlobTooWide,
    TypeMismatch,
    InvalidLimits,
    DefaultOutOfBounds,
    NonFinite,
    InvalidScaling,
    InvalidAccessPolicy,
}

#[derive(Clone, Copy, Debug)]
pub struct VariableDescriptor {
    spec: DescriptorSpec,
}

impl VariableDescriptor {
    pub fn try_new(spec: DescriptorSpec) -> Result<Self, DescriptorError> {
        if spec.symbol.is_empty() {
            return Err(DescriptorError::EmptySymbol);
        }
        if spec.owner.is_empty() {
            return Err(DescriptorError::EmptyOwner);
        }
        if spec.unit.is_empty() {
            return Err(DescriptorError::EmptyUnit);
        }
        if spec.wire_type.width() == 0 {
            return Err(DescriptorError::ZeroWidth);
        }
        if matches!(spec.wire_type, WireType::Blob { width } if width > 32) {
            return Err(DescriptorError::BlobTooWide);
        }
        if spec.access.is_writable() != matches!(spec.write_policy, WritePolicy::CompleteScalar)
            || (spec.access.is_writable() && !spec.wire_type.is_scalar())
        {
            return Err(DescriptorError::InvalidAccessPolicy);
        }
        if let Some(limits) = spec.limits
            && (limits.minimum().wire_type() != spec.wire_type
                || limits.maximum().wire_type() != spec.wire_type)
        {
            return Err(DescriptorError::TypeMismatch);
        }
        if let Some(default) = spec.default {
            if default.wire_type() != spec.wire_type {
                return Err(DescriptorError::TypeMismatch);
            }
            if !default.is_finite() {
                return Err(DescriptorError::NonFinite);
            }
            if spec.limits.is_some_and(|limits| !limits.contains(default)) {
                return Err(DescriptorError::DefaultOutOfBounds);
            }
        }
        Ok(Self { spec })
    }

    #[must_use]
    pub const fn variable_id(&self) -> u32 {
        self.spec.variable_id
    }

    #[must_use]
    pub const fn symbol(&self) -> &'static str {
        self.spec.symbol
    }

    #[must_use]
    pub const fn owner(&self) -> &'static str {
        self.spec.owner
    }

    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.spec.description
    }

    #[must_use]
    pub const fn wire_type(&self) -> WireType {
        self.spec.wire_type
    }

    #[must_use]
    pub const fn byte_order(&self) -> ByteOrder {
        self.spec.byte_order
    }

    #[must_use]
    pub const fn unit(&self) -> &'static str {
        self.spec.unit
    }

    #[must_use]
    pub const fn scaling(&self) -> Scaling {
        self.spec.scaling
    }

    #[must_use]
    pub const fn limits(&self) -> Option<NumericLimits> {
        self.spec.limits
    }

    #[must_use]
    pub const fn default(&self) -> Option<ScalarValue> {
        self.spec.default
    }

    #[must_use]
    pub const fn access(&self) -> Access {
        self.spec.access
    }

    #[must_use]
    pub const fn write_policy(&self) -> WritePolicy {
        self.spec.write_policy
    }

    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.spec.schema_version
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MappedVariable {
    address_extension: u8,
    address: u32,
    descriptor: VariableDescriptor,
}

impl MappedVariable {
    #[must_use]
    pub const fn new(address_extension: u8, address: u32, descriptor: VariableDescriptor) -> Self {
        Self {
            address_extension,
            address,
            descriptor,
        }
    }

    #[must_use]
    pub const fn address_extension(&self) -> u8 {
        self.address_extension
    }

    #[must_use]
    pub const fn address(&self) -> u32 {
        self.address
    }

    #[must_use]
    pub const fn descriptor(&self) -> &VariableDescriptor {
        &self.descriptor
    }

    fn end_exclusive(&self) -> Option<u32> {
        self.address
            .checked_add(u32::from(self.descriptor.wire_type().width()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapError {
    EmptyRegistry,
    DuplicateVariableId,
    DuplicateSymbol,
    AddressOverflow,
    Overlap,
    EmptyAccess,
    Unmapped,
    CrossesRegion,
    ReadOnly,
    PartialWrite,
    EncodedWidthMismatch,
}

#[derive(Clone, Copy, Debug)]
pub struct ResolvedRead<'a> {
    pub variable: &'a MappedVariable,
    pub offset: u8,
    pub length: u8,
}

/// A checked, stable virtual address registry with no pointer-backed entries.
pub struct Registry<const N: usize> {
    variables: [MappedVariable; N],
}

impl<const N: usize> Registry<N> {
    pub fn try_new(variables: [MappedVariable; N]) -> Result<Self, MapError> {
        if N == 0 {
            return Err(MapError::EmptyRegistry);
        }
        for (index, variable) in variables.iter().enumerate() {
            let Some(end) = variable.end_exclusive() else {
                return Err(MapError::AddressOverflow);
            };
            for other in &variables[..index] {
                if variable.descriptor.variable_id() == other.descriptor.variable_id() {
                    return Err(MapError::DuplicateVariableId);
                }
                if variable.descriptor.symbol() == other.descriptor.symbol() {
                    return Err(MapError::DuplicateSymbol);
                }
                if variable.address_extension == other.address_extension {
                    let other_end = other.end_exclusive().ok_or(MapError::AddressOverflow)?;
                    if variable.address < other_end && other.address < end {
                        return Err(MapError::Overlap);
                    }
                }
            }
        }
        Ok(Self { variables })
    }

    #[must_use]
    pub const fn variables(&self) -> &[MappedVariable; N] {
        &self.variables
    }

    pub fn resolve_read(
        &self,
        address_extension: u8,
        address: u32,
        length: u8,
    ) -> Result<ResolvedRead<'_>, MapError> {
        if length == 0 {
            return Err(MapError::EmptyAccess);
        }
        let end = address
            .checked_add(u32::from(length))
            .ok_or(MapError::AddressOverflow)?;
        for variable in &self.variables {
            if variable.address_extension != address_extension {
                continue;
            }
            let variable_end = variable.end_exclusive().ok_or(MapError::AddressOverflow)?;
            if address >= variable.address && address < variable_end {
                if end > variable_end {
                    return Err(MapError::CrossesRegion);
                }
                return Ok(ResolvedRead {
                    variable,
                    offset: (address - variable.address) as u8,
                    length,
                });
            }
        }
        Err(MapError::Unmapped)
    }

    pub fn resolve_write(
        &self,
        address_extension: u8,
        address: u32,
        value: &EncodedValue,
    ) -> Result<&MappedVariable, MapError> {
        let resolved = self.resolve_read(address_extension, address, value.len())?;
        if !resolved.variable.descriptor.access().is_writable() {
            return Err(MapError::ReadOnly);
        }
        if resolved.offset != 0 || value.len() != resolved.variable.descriptor.wire_type().width() {
            return Err(MapError::PartialWrite);
        }
        if !matches!(
            resolved.variable.descriptor.write_policy(),
            WritePolicy::CompleteScalar
        ) {
            return Err(MapError::ReadOnly);
        }
        Ok(resolved.variable)
    }
}
