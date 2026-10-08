use xcp_messages::Correlation;

/// Lossless coordinates that an adapter combines into the calibration
/// package's canonical OperationKey.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderOperation {
    pub service_epoch: u64,
    pub session_generation: u32,
    pub sequence: u64,
}

impl ProviderOperation {
    pub const fn correlation(self) -> Correlation {
        Correlation {
            session_generation: self.session_generation,
            sequence: self.sequence,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadRequest {
    pub correlation: Correlation,
    pub descriptor_id: u32,
    pub offset: u32,
    pub length: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplyRequest {
    pub operation: ProviderOperation,
    pub descriptor_id: u32,
    pub encoded_value: [u8; 4],
    pub length: u8,
    pub expires_at_us: u64,
}

/// Owned commands submitted synchronously to an adapter. Implementations may
/// enqueue these in a fixed mailbox, but cannot expose owner state to XCP.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderRequest {
    Synchronize { session_generation: u32 },
    StoreCalibration { correlation: Correlation },
    Read(ReadRequest),
    Apply(ApplyRequest),
    ResolveOrCancel { operation: ProviderOperation },
    Quiesce { correlation: Correlation },
    ReleaseOutcome { operation: ProviderOperation },
}

/// Nonblocking owned-command admission used by synchronous HSM actions.
/// `Unavailable` may follow ownership transfer: adapters must retain/reconcile
/// that request and deliver its definitive correlated completion. Initial
/// commands are not replayed. ResolveOrCancel and ReleaseOutcome can be retried
/// with the same exact key under the owner's idempotent retirement fence.
pub trait ProviderPort {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    /// The bounded command path has no slot. No owner mutation was admitted.
    Busy,
    /// Delivery/owner availability is uncertain; the protocol must fence.
    Unavailable,
}
