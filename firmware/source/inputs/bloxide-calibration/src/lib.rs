#![no_std]
#![doc = include_str!("../README.md")]

#[cfg(feature = "std")]
extern crate std;

pub mod descriptor;
pub mod encoded;
#[cfg(feature = "std")]
pub mod exchange;
pub mod transaction;
pub mod value;

pub use descriptor::{
    Access, ByteOrder, DescriptorError, DescriptorSpec, MapError, MappedVariable, NumericLimits,
    Registry, ResolvedRead, ScalarValue, Scaling, VariableDescriptor, WireType, WritePolicy,
};
pub use encoded::{EncodedError, EncodedValue};
pub use transaction::{
    ActiveRevision, ApplyField, ApplyRequest, CommitGate, CommitRecord, GateError, OperationKey,
    Owner, OwnerMode, OwnerOutcome, RejectReason, ReleaseError, ResetError,
};
pub use value::{
    BoundedI32, BoundedU16, BoundedU32, Celsius, Milliseconds, Permille, Unit, Unitless,
    ValidatedScalar, ValueError,
};
