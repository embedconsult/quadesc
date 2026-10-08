#![no_std]

/// Maximum command/response packet size in scalar profile S0.
pub const MAX_CTO: usize = 8;

/// An owned, fixed-capacity XCP command or response PDU.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Packet {
    bytes: [u8; MAX_CTO],
    len: u8,
}

impl Packet {
    /// Copies a complete packet when its length is valid for S0.
    pub fn try_from_slice(bytes: &[u8]) -> Result<Self, PacketLengthError> {
        if bytes.is_empty() || bytes.len() > MAX_CTO {
            return Err(PacketLengthError { len: bytes.len() });
        }
        let mut packet = Self {
            bytes: [0; MAX_CTO],
            len: bytes.len() as u8,
        };
        packet.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(packet)
    }

    /// Creates a packet from a fixed buffer and checked logical length.
    pub const fn from_parts(bytes: [u8; MAX_CTO], len: u8) -> Result<Self, PacketLengthError> {
        if len == 0 || len as usize > MAX_CTO {
            Err(PacketLengthError { len: len as usize })
        } else {
            Ok(Self { bytes, len })
        }
    }

    /// Returns the packet bytes without unused fixed-capacity tail bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Returns the logical packet length.
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns true only for the impossible zero-length stored packet.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Rejected packet length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketLengthError {
    /// Attempted logical length.
    pub len: usize,
}

/// Messages accepted by the declarative session blox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XcpSessionMsg {
    /// A complete transport-validated XCP command PDU.
    Packet(PacketRequest),
    /// A provider completion correlated to an earlier owned request.
    ProviderCompletion(ProviderCompletion),
    /// Retry/deadline service using the same monotonic time domain as writes.
    Service(ServiceRequest),
    /// The underlying transport can no longer deliver the pending response.
    TransportLost,
    /// The byte service has ended after transport loss and owner settlement.
    TransportSettled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketRequest {
    pub packet: Packet,
    pub now_us: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceRequest {
    pub now_us: u64,
}

/// Correlation coordinates used between the broker and provider adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Correlation {
    pub session_generation: u32,
    pub sequence: u64,
}

/// Provider completion messages. The provider adapter owns translation from
/// canonical calibration outcomes and must verify exact keys before sending.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderCompletion {
    CalibrationStored { correlation: Correlation, verified: bool },
    Synchronized {
        session_generation: u32,
        result: SynchronizeResult,
    },
    Read {
        correlation: Correlation,
        result: ReadResult,
    },
    Write {
        correlation: Correlation,
        result: WriteResult,
    },
    Quiesced {
        correlation: Correlation,
        result: QuiesceResult,
    },
    Released {
        correlation: Correlation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizeResult {
    Ready { service_epoch: u64 },
    Busy,
    WrongLifecycle,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadData {
    pub bytes: [u8; 7],
    pub len: u8,
}

impl ReadData {
    pub const fn new(bytes: [u8; 7], len: u8) -> Option<Self> {
        if len > 0 && len <= 7 {
            Some(Self { bytes, len })
        } else {
            None
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadResult {
    Data(ReadData),
    Busy,
    OutOfRange,
    AccessDenied,
    WrongLifecycle,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteResult {
    Applied {
        changed: bool,
        active_revision: u32,
    },
    /// Owner could not admit this key because a different retained outcome
    /// still occupies its one slot. This key has no retained outcome.
    Busy,
    Rejected {
        reason: RejectionReason,
    },
    Cancelled,
    Retired,
    StaleOperation,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectionReason {
    BadEncoding,
    Bounds,
    CrossField,
    ReadOnly,
    UnknownVariable,
    Expired,
    WrongLifecycle,
    RevisionMismatch,
    OutputBusy,
    OutputUnavailable,
    /// A provider policy outside the selected S0 mapping. The session is
    /// fenced instead of inventing an XCP error with a different meaning.
    UnsupportedPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuiesceResult {
    Quiesced,
    Busy,
    Uncertain,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_rejects_empty_and_oversize_inputs() {
        assert_eq!(
            Packet::try_from_slice(&[]),
            Err(PacketLengthError { len: 0 })
        );
        assert_eq!(
            Packet::try_from_slice(&[0; 9]),
            Err(PacketLengthError { len: 9 })
        );
    }

    #[test]
    fn packet_hides_unused_tail() {
        let packet = Packet::try_from_slice(&[0xFF, 0]).unwrap();
        assert_eq!(packet.as_slice(), &[0xFF, 0]);
        assert_eq!(packet.len(), 2);
    }
}
