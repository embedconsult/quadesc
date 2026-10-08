#![no_std]
use bloxide_calibration::{ApplyRequest, OperationKey, OwnerOutcome};

pub const PERIOD_ID: u32 = 1;
pub const DUTY_ID: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedMsg {
    Apply(ApplyRequest),
    Release(OperationKey),
    Read,
    ReadSaved,
    /// Explicitly selected restricted P; transport framing stays outside this owner.
    ProfileP(xcp_messages::PacketRequest),
    ProfileReset,
    ProfileLost,
    Save(u64),
    ResolveSave(bloxide_persistence::OperationKey),
    /// Additive generated-session provider command; same Owner handles Apply.
    XcpProvider(xcp_core::ProviderRequest),
    /// Output service notification; the Owner still owns admission and value.
    OutputCompleted(u64),
    OutputFaulted(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedView {
    pub period_ms: u16,
    pub duty_permille: u16,
    pub revision: u32,
    pub phase_epoch_us: u64,
    pub commanded_on: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SavedView {
    Unavailable,
    NoRecord,
    Record {
        sequence: u64,
        source_epoch: u64,
        source_revision: u32,
        period_ms: u16,
        duty_permille: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedResult {
    View(LedView),
    Saved(SavedView),
    Outcome(OwnerOutcome<LedConfig>),
    Released(bool),
    ProfileP(ProfileReply),
    Save(SaveReceipt),
    SaveStatus(SaveStatus),
    XcpProvider(XcpProviderReply),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XcpProviderReply {
    Completion(xcp_messages::ProviderCompletion),
    Fenced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveReceipt {
    Unavailable,
    Accepted(bloxide_persistence::OperationKey),
    Durable(bloxide_persistence::OperationKey),
    Rejected,
    Fenced,
}

/// A receipt is only durable after the portable service's terminal readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveStatus {
    Pending,
    Durable { sequence: u64, saved_revision: u32 },
    Failed,
    Indeterminate,
    Rejected,
    Stale,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileReply {
    Packet(xcp_messages::Packet),
    Ignored,
    Fenced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedConfig {
    period_ms: bloxide_calibration::BoundedU16<bloxide_calibration::Milliseconds, 100, 10000>,
    duty_permille: bloxide_calibration::BoundedU16<bloxide_calibration::Permille, 0, 1000>,
}
impl LedConfig {
    pub fn new(
        period_ms: u16,
        duty_permille: u16,
    ) -> Result<Self, bloxide_calibration::ValueError> {
        Ok(Self {
            period_ms: bloxide_calibration::BoundedU16::try_new(period_ms)?,
            duty_permille: bloxide_calibration::BoundedU16::try_new(duty_permille)?,
        })
    }
    pub fn defaults() -> Self {
        Self::new(1000, 500).expect("valid defaults")
    }
    pub const fn period_ms(self) -> u16 {
        self.period_ms.get()
    }
    pub const fn duty_permille(self) -> u16 {
        self.duty_permille.get()
    }
}
impl bloxide_persistence_calibration::CanonicalCalibration for LedConfig {
    fn encode(self, out: &mut [u8; 256]) -> u16 {
        out[..2].copy_from_slice(&self.period_ms().to_le_bytes());
        out[2..4].copy_from_slice(&self.duty_permille().to_le_bytes());
        4
    }
}
