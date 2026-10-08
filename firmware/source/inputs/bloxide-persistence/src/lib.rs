#![no_std]
#![doc = include_str!("../README.md")]

mod codec;
mod geometry;
mod record;
mod recovery;
mod service;
mod trace;
pub use trace::{TraceEvent, TraceRing};

pub use codec::{
    Classification, CorruptReason, EraseQualification, ReadIssue, SlotRead, UnsupportedKind,
    classify_slot, crc32c, decode_body, encode_record, qualify_erase,
};
pub use geometry::{Geometry, GeometryError, Slot};
pub use record::{
    BODY_BYTES, ENCODED_BODY_BYTES, FORMAT, HEADER_BYTES, MAGIC, MAX_IMAGE_BYTES,
    MAX_PAYLOAD_BYTES, OperationKey, Record, SlotImage, Snapshot, SnapshotError,
};
pub use recovery::{
    DefaultsReason, Recovery, RecoveryDisposition, RecoveryError, SavePlan, SelectedRecord,
    WritePolicy, recover,
};
pub use service::{
    BackendCommand, BackendConfig, BackendTiming, CommandHeader, CommandKind, Completion,
    CompletionError, CompletionStatus, FailureKind, FailurePhase, Health, Outcome,
    PersistenceService, QuiesceError, QuiesceRequest, RejectReason, Resolve, SaveResponse,
    ServiceMode, ServiceState, WearLease,
};

pub(crate) use codec::{classify_scanned, qualify_scanned};

/// Application-owned canonical encoding and validation policy.
///
/// Implementations must be deterministic, bounded, and allocation-free. The
/// persistence core never guesses an unknown schema and never stores Rust object
/// representations directly.
pub trait Schema {
    fn id(&self) -> u16;
    fn payload_len(&self) -> u16;
    fn defaults(&self, out: &mut [u8; MAX_PAYLOAD_BYTES]) -> Result<(), SchemaError>;
    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaError {
    InvalidDefinition,
    InvalidValue,
}
