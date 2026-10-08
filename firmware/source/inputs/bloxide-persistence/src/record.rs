use crate::{Schema, SchemaError, geometry::Geometry};

pub const HEADER_BYTES: usize = 64;
pub const MAX_PAYLOAD_BYTES: usize = 256;
pub const BODY_BYTES: usize = HEADER_BYTES + MAX_PAYLOAD_BYTES;
pub const ENCODED_BODY_BYTES: usize = BODY_BYTES * 2;
pub const MAX_IMAGE_BYTES: usize = 1024;
pub const MAGIC: [u8; 8] = *b"BLXPERS2";
pub const FORMAT: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperationKey {
    pub service_epoch: u64,
    pub session_generation: u32,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    ZeroSchema,
    LengthOutOfRange,
    LengthMismatch,
    InvalidValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    schema: u16,
    len: u16,
    bytes: [u8; MAX_PAYLOAD_BYTES],
    source_epoch: u64,
    source_revision: u32,
    operation: OperationKey,
}

impl Snapshot {
    pub fn new<S: Schema>(
        schema: &S,
        bytes: &[u8],
        source_epoch: u64,
        source_revision: u32,
        operation: OperationKey,
    ) -> Result<Self, SnapshotError> {
        if schema.id() == 0 {
            return Err(SnapshotError::ZeroSchema);
        }
        let len = usize::from(schema.payload_len());
        if len == 0 || len > MAX_PAYLOAD_BYTES {
            return Err(SnapshotError::LengthOutOfRange);
        }
        if bytes.len() != len {
            return Err(SnapshotError::LengthMismatch);
        }
        schema
            .validate(bytes)
            .map_err(|_| SnapshotError::InvalidValue)?;
        let mut canonical = [0xFF; MAX_PAYLOAD_BYTES];
        canonical[..len].copy_from_slice(bytes);
        Ok(Self {
            schema: schema.id(),
            len: schema.payload_len(),
            bytes: canonical,
            source_epoch,
            source_revision,
            operation,
        })
    }

    pub fn validate_schema<S: Schema>(schema: &S) -> Result<(), SnapshotError> {
        if schema.id() == 0 {
            return Err(SnapshotError::ZeroSchema);
        }
        if !(1..=MAX_PAYLOAD_BYTES).contains(&usize::from(schema.payload_len())) {
            return Err(SnapshotError::LengthOutOfRange);
        }
        Ok(())
    }

    pub fn validate_for<S: Schema>(&self, schema: &S) -> Result<(), SnapshotError> {
        Self::validate_schema(schema)?;
        if self.schema != schema.id() || self.len != schema.payload_len() {
            return Err(SnapshotError::LengthMismatch);
        }
        schema
            .validate(self.bytes())
            .map_err(|_| SnapshotError::InvalidValue)
    }

    pub fn defaults<S: Schema>(
        schema: &S,
        source_epoch: u64,
        operation: OperationKey,
    ) -> Result<Self, SnapshotError> {
        Self::validate_schema(schema)?;
        let mut bytes = [0xFF; MAX_PAYLOAD_BYTES];
        schema.defaults(&mut bytes).map_err(|error| match error {
            SchemaError::InvalidDefinition => SnapshotError::LengthOutOfRange,
            SchemaError::InvalidValue => SnapshotError::InvalidValue,
        })?;
        Self::new(
            schema,
            &bytes[..usize::from(schema.payload_len())],
            source_epoch,
            0,
            operation,
        )
    }

    #[must_use]
    pub const fn schema(&self) -> u16 {
        self.schema
    }

    #[must_use]
    pub const fn payload_len(&self) -> u16 {
        self.len
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    #[must_use]
    pub const fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    #[must_use]
    pub const fn source_revision(&self) -> u32 {
        self.source_revision
    }

    #[must_use]
    pub const fn operation(&self) -> OperationKey {
        self.operation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    pub format: u16,
    pub sequence: u64,
    pub snapshot: Snapshot,
}

impl Record {
    #[must_use]
    pub const fn new(sequence: u64, snapshot: Snapshot) -> Self {
        Self {
            format: FORMAT,
            sequence,
            snapshot,
        }
    }

    /// Identity of the physical envelope, excluding only its unstored live epoch.
    #[must_use]
    pub fn persistent_identity_eq(&self, other: &Self) -> bool {
        let mut left = *self;
        let mut right = *other;
        left.snapshot.operation.service_epoch = 0;
        right.snapshot.operation.service_epoch = 0;
        left == right
    }

    #[must_use]
    pub fn is_same_durable_snapshot(&self, snapshot: &Snapshot) -> bool {
        self.snapshot.schema == snapshot.schema
            && self.snapshot.len == snapshot.len
            && self.snapshot.source_epoch == snapshot.source_epoch
            && self.snapshot.source_revision == snapshot.source_revision
            && self.snapshot.bytes == snapshot.bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotImage {
    bytes: [u8; MAX_IMAGE_BYTES],
    len: u16,
}

impl SlotImage {
    #[must_use]
    pub const fn erased(geometry: Geometry) -> Self {
        Self {
            bytes: [0xFF; MAX_IMAGE_BYTES],
            len: geometry.programmed_span(),
        }
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    #[must_use]
    pub(crate) const fn bytes_mut(&mut self) -> &mut [u8; MAX_IMAGE_BYTES] {
        &mut self.bytes
    }
}
