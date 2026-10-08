#![no_std]

pub mod map;
pub mod provider;
pub mod session;

pub use map::{
    Access, MapDefinitionError, ReadResolveError, Region, ResolvedRead, ResolvedWrite, VirtualMap,
    WriteResolveError,
};
pub use provider::{
    ApplyRequest, ProviderOperation, ProviderPort, ProviderRequest, ReadRequest, SubmitError,
};
pub use session::{Dispatch, ServiceResult, Session, SessionState, APPLY_DEADLINE_US};
pub use xcp_messages::{
    Correlation, Packet, ProviderCompletion, QuiesceResult, ReadData, ReadResult, RejectionReason,
    SynchronizeResult, WriteResult, MAX_CTO,
};

/// S0 resource mask: CAL/PAG only. Page commands remain unsupported.
pub const CONNECT_RESOURCE: u8 = 0x01;
/// Little-endian, byte granularity, no block mode or optional mode info.
pub const COMM_MODE_BASIC: u8 = 0x00;
pub const MAX_DTO: u8 = 8;
pub const PROTOCOL_LAYER_VERSION: u8 = 0x01;
pub const TRANSPORT_LAYER_VERSION: u8 = 0x01;

/// Supported command identifiers in the S0 command table.
pub mod command {
    pub const CONNECT: u8 = 0xFF;
    pub const DISCONNECT: u8 = 0xFE;
    pub const GET_STATUS: u8 = 0xFD;
    pub const SYNCH: u8 = 0xFC;
    pub const SET_MTA: u8 = 0xF6;
    pub const UPLOAD: u8 = 0xF5;
    pub const DOWNLOAD: u8 = 0xF0;
}

/// Public XCP error bytes selected by the S0 profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ErrorCode {
    CommandSynch = 0x00,
    CommandBusy = 0x10,
    CommandUnknown = 0x20,
    CommandSyntax = 0x21,
    OutOfRange = 0x22,
    WriteProtected = 0x23,
    AccessDenied = 0x24,
    ModeNotValid = 0x27,
    Generic = 0x31,
}
