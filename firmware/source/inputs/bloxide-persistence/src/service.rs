use crate::{
    Classification, EraseQualification, Geometry, MAX_IMAGE_BYTES, MAX_PAYLOAD_BYTES, OperationKey,
    ReadIssue, Record, Recovery, RecoveryError, SavePlan, Schema, Slot, SlotImage, Snapshot,
    classify_scanned, encode_record, qualify_scanned, recover,
};

/// Provisioning-authority lease, never a caller's MCU boot counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WearLease(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendTiming {
    pub read: u64,
    pub permit: u64,
    pub erase: u64,
    pub program: u64,
    pub quiesce: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendConfig {
    pub lease: WearLease,
    pub timing: BackendTiming,
}

/// The backend must prove the referenced work idle and drain its completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuiesceRequest {
    pub outstanding: CommandHeader,
}

#[derive(Clone, Copy)]
struct QuiesceControl {
    request: QuiesceRequest,
    published: bool,
    deadline: u64,
    expired: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Read,
    ConsumeErasePermit { lease: WearLease, issuance: u64 },
    Erase { permit: u64 },
    Program,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandHeader {
    pub io_epoch: u64,
    pub command_sequence: u64,
    pub slot: Slot,
    pub kind: CommandKind,
    pub offset: u32,
    pub len: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendCommand {
    pub header: CommandHeader,
    pub data: [u8; MAX_PAYLOAD_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus {
    Ok,
    WearGranted { permit: u64 },
    WearDenied,
    Failed { quiescent: bool },
    Read { len: u16, issue: Option<ReadIssue> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Completion {
    pub header: CommandHeader,
    pub status: CompletionStatus,
    pub data: [u8; MAX_PAYLOAD_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionError {
    NothingOutstanding,
    CorrelationMismatch,
    WrongCompletionKind,
    InvalidReadLength,
    CommandSequenceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Idle,
    QualifyErase,
    AcquireWear,
    Erasing,
    VerifyErased,
    ProgramBody,
    VerifyBody,
    ProgramCommit,
    VerifyCommitted,
    Retained,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceMode {
    Running,
    Quiescing,
    Stopped,
    ReadOnlyFault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailurePhase {
    Admission,
    Qualification,
    Wear,
    Erase,
    Body,
    Commit,
    Verification,
    Correlation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    UnsafeErasePrestate,
    RecoveryChanged,
    WearUnavailable,
    Backend,
    Read(ReadIssue),
    Verification,
    Correlation,
    SequenceExhausted,
    Timeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectReason {
    Expired,
    WrongLifecycle,
    WrongOperationKey,
    WritesLocked,
    RecoveryAuthorizationRequired,
    SequenceExhausted,
    RateLimited,
    InvalidSnapshot,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Durable {
        key: OperationKey,
        record: Record,
        existing: bool,
    },
    Failed {
        key: OperationKey,
        phase: FailurePhase,
        reason: FailureKind,
        no_new_commit: bool,
    },
    Indeterminate {
        key: OperationKey,
        phase: FailurePhase,
        reason: FailureKind,
    },
    Rejected {
        key: OperationKey,
        reason: RejectReason,
    },
    Cancelled {
        key: OperationKey,
    },
}

impl Outcome {
    #[must_use]
    pub const fn key(self) -> OperationKey {
        match self {
            Self::Durable { key, .. }
            | Self::Failed { key, .. }
            | Self::Indeterminate { key, .. }
            | Self::Rejected { key, .. }
            | Self::Cancelled { key } => key,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveResponse {
    Accepted,
    Pending,
    Terminal(Outcome),
    Busy,
    Retired { durable: Option<Record> },
    Stale { durable: Option<Record> },
    KeyConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolve {
    Pending,
    Terminal(Outcome),
    Retired { durable: Option<Record> },
    Stale { durable: Option<Record> },
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuiesceError {
    EpochExhausted,
    NotStopped,
    OutstandingResult,
    Faulted,
    EpochNotAdvanced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Health {
    pub quarantined_a: bool,
    pub quarantined_b: bool,
    pub correlation_fault: bool,
    pub write_locked: bool,
}

impl Health {
    const fn healthy() -> Self {
        Self {
            quarantined_a: false,
            quarantined_b: false,
            correlation_fault: false,
            write_locked: false,
        }
    }

    fn quarantine(&mut self, slot: Slot) {
        match slot {
            Slot::A => self.quarantined_a = true,
            Slot::B => self.quarantined_b = true,
        }
        self.write_locked = true;
    }
}

#[derive(Clone, Copy)]
struct Pending {
    key: OperationKey,
    target: Slot,
    record: Record,
    image: SlotImage,
    body_offset: u16,
    marker_may_have_changed: bool,
    verification_started: bool,
    permit: Option<u64>,
}

#[derive(Clone, Copy)]
struct Scan {
    prefix: [u8; MAX_IMAGE_BYTES],
    offset: u32,
    all_ff: bool,
    tail_ff: bool,
    issue: Option<ReadIssue>,
}

impl Scan {
    const fn new() -> Self {
        Self {
            prefix: [0xFF; MAX_IMAGE_BYTES],
            offset: 0,
            all_ff: true,
            tail_ff: true,
            issue: None,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

pub struct PersistenceService<S: Schema + Copy> {
    geometry: Geometry,
    schema: S,
    recovery: Recovery,
    recovery_authorized: bool,
    service_epoch: u64,
    session_generation: u32,
    io_epoch: u64,
    next_command_sequence: u64,
    next_issuance: u64,
    state: ServiceState,
    mode: ServiceMode,
    pending: Option<Pending>,
    retained: Option<Outcome>,
    retained_snapshot: Option<Snapshot>,
    retired_through: Option<u64>,
    outstanding: Option<CommandHeader>,
    command_deadline: u64,
    backend: BackendConfig,
    quiesce: Option<QuiesceControl>,
    quiesce_attempted: bool,
    interrupted: bool,
    scan: Scan,
    health: Health,
    minimum_admission_interval: u64,
    last_write_admission: Option<u64>,
}

impl<S: Schema + Copy> PersistenceService<S> {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        geometry: Geometry,
        schema: S,
        recovery: Recovery,
        recovery_authorized: bool,
        service_epoch: u64,
        session_generation: u32,
        io_epoch: u64,
        minimum_admission_interval: u64,
        backend: BackendConfig,
    ) -> Self {
        let valid_registration = Snapshot::defaults(
            &schema,
            0,
            OperationKey {
                service_epoch: 0,
                session_generation: 0,
                sequence: 0,
            },
        )
        .is_ok();
        let t = backend.timing;
        let mode = if !valid_registration
            || backend.lease.0 == 0
            || [t.read, t.permit, t.erase, t.program, t.quiesce].contains(&0)
            || matches!(recovery.write_policy, crate::WritePolicy::Locked)
        {
            ServiceMode::ReadOnlyFault
        } else {
            ServiceMode::Running
        };
        Self {
            geometry,
            schema,
            recovery,
            recovery_authorized,
            service_epoch,
            session_generation,
            io_epoch,
            next_command_sequence: 1,
            next_issuance: 1,
            state: ServiceState::Idle,
            mode,
            pending: None,
            retained: None,
            retained_snapshot: None,
            retired_through: None,
            outstanding: None,
            command_deadline: 0,
            backend,
            quiesce: None,
            quiesce_attempted: false,
            interrupted: false,
            scan: Scan::new(),
            health: Health {
                write_locked: mode == ServiceMode::ReadOnlyFault,
                ..Health::healthy()
            },
            minimum_admission_interval,
            last_write_admission: None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> ServiceState {
        self.state
    }

    #[must_use]
    pub const fn mode(&self) -> ServiceMode {
        self.mode
    }

    #[must_use]
    pub const fn health(&self) -> Health {
        self.health
    }

    #[must_use]
    pub const fn retained(&self) -> Option<Outcome> {
        self.retained
    }

    #[must_use]
    pub const fn read_durable(&self) -> Option<Record> {
        match self.recovery.selected() {
            Some(selected) => Some(selected.record),
            None => None,
        }
    }

    fn key_is_current(&self, key: OperationKey) -> bool {
        key.service_epoch == self.service_epoch && key.session_generation == self.session_generation
    }

    fn classify_key(&self, key: OperationKey) -> Option<SaveResponse> {
        if let Some(outcome) = self.retained
            && outcome.key() == key
        {
            return Some(SaveResponse::Terminal(outcome));
        }
        if let Some(pending) = self.pending
            && pending.key == key
        {
            return Some(SaveResponse::Pending);
        }
        if !self.key_is_current(key) {
            return Some(SaveResponse::Stale {
                durable: self.read_durable(),
            });
        }
        if self
            .retired_through
            .is_some_and(|retired| key.sequence <= retired)
        {
            return Some(SaveResponse::Retired {
                durable: self.read_durable(),
            });
        }
        None
    }

    pub fn save<C: FnMut() -> u64>(
        &mut self,
        key: OperationKey,
        snapshot: Snapshot,
        admission_deadline: u64,
        mut clock: C,
    ) -> SaveResponse {
        if let Some(existing) = self.classify_key(key) {
            if matches!(existing, SaveResponse::Pending | SaveResponse::Terminal(_)) {
                let original = self
                    .pending
                    .map(|pending| pending.record.snapshot)
                    .or(self.retained_snapshot);
                if original.map_or(self.retained.is_some(), |original| original != snapshot) {
                    return SaveResponse::KeyConflict;
                }
            }
            return existing;
        }
        if self.pending.is_some() || self.retained.is_some() {
            return SaveResponse::Busy;
        }
        if self.mode != ServiceMode::Running || self.state != ServiceState::Idle {
            return self.retain_rejected(key, RejectReason::WrongLifecycle, Some(snapshot));
        }
        if snapshot.operation() != key {
            return self.retain_rejected(key, RejectReason::WrongOperationKey, Some(snapshot));
        }
        if clock() >= admission_deadline {
            return self.retain_rejected(key, RejectReason::Expired, Some(snapshot));
        }
        if snapshot.validate_for(&self.schema).is_err() {
            return self.retain_rejected(key, RejectReason::InvalidSnapshot, Some(snapshot));
        }
        // Validation can take time. This final sample is the synchronous capture/admission cut.
        let now = clock();
        if now >= admission_deadline {
            return self.retain_rejected(key, RejectReason::Expired, Some(snapshot));
        }
        let plan = match self.recovery.plan(snapshot, self.recovery_authorized) {
            Ok(plan) => plan,
            Err(RecoveryError::WritesLocked | RecoveryError::InvalidDefaults) => {
                return self.retain_rejected(key, RejectReason::WritesLocked, Some(snapshot));
            }
            Err(RecoveryError::RecoveryAuthorizationRequired) => {
                return self.retain_rejected(
                    key,
                    RejectReason::RecoveryAuthorizationRequired,
                    Some(snapshot),
                );
            }
            Err(RecoveryError::SequenceExhausted) => {
                return self.retain_rejected(key, RejectReason::SequenceExhausted, Some(snapshot));
            }
        };
        match plan {
            SavePlan::DurableExisting(selected) => {
                self.retained = Some(Outcome::Durable {
                    key,
                    record: selected.record,
                    existing: true,
                });
                self.retained_snapshot = Some(snapshot);
                self.state = ServiceState::Retained;
                SaveResponse::Terminal(self.retained.expect("just retained"))
            }
            SavePlan::Write { target, record } => {
                if self
                    .last_write_admission
                    .is_some_and(|last| now < last.saturating_add(self.minimum_admission_interval))
                {
                    return self.retain_rejected(key, RejectReason::RateLimited, Some(snapshot));
                }
                self.last_write_admission = Some(now);
                self.pending = Some(Pending {
                    key,
                    target,
                    record,
                    image: encode_record(&record, self.geometry),
                    body_offset: 0,
                    marker_may_have_changed: false,
                    verification_started: false,
                    permit: None,
                });
                self.quiesce_attempted = false;
                self.interrupted = false;
                self.scan.reset();
                self.state = ServiceState::QualifyErase;
                SaveResponse::Accepted
            }
        }
    }

    fn retain_rejected(
        &mut self,
        key: OperationKey,
        reason: RejectReason,
        snapshot: Option<Snapshot>,
    ) -> SaveResponse {
        let outcome = Outcome::Rejected { key, reason };
        self.retained = Some(outcome);
        self.retained_snapshot = snapshot;
        self.state = ServiceState::Retained;
        SaveResponse::Terminal(outcome)
    }

    #[must_use]
    pub fn resolve(&self, key: OperationKey) -> Resolve {
        if let Some(outcome) = self.retained.filter(|outcome| outcome.key() == key) {
            return Resolve::Terminal(outcome);
        }
        if self.pending.is_some_and(|pending| pending.key == key) {
            return Resolve::Pending;
        }
        if !self.key_is_current(key) {
            return Resolve::Stale {
                durable: self.read_durable(),
            };
        }
        if self
            .retired_through
            .is_some_and(|retired| key.sequence <= retired)
        {
            return Resolve::Retired {
                durable: self.read_durable(),
            };
        }
        Resolve::Unknown
    }

    pub fn resolve_or_cancel(&mut self, key: OperationKey) -> Resolve {
        let resolved = self.resolve(key);
        if resolved != Resolve::Unknown {
            return resolved;
        }
        if self.pending.is_some() || self.retained.is_some() || !self.key_is_current(key) {
            return resolved;
        }
        let outcome = Outcome::Cancelled { key };
        self.retained = Some(outcome);
        self.retained_snapshot = None;
        self.state = ServiceState::Retained;
        Resolve::Terminal(outcome)
    }

    pub fn release(&mut self, key: OperationKey) -> Result<(), QuiesceError> {
        if !self.key_is_current(key) {
            return Err(QuiesceError::OutstandingResult);
        }
        if self.retired_through.is_some_and(|n| key.sequence <= n) {
            return Ok(());
        }
        if !self.ownership_settled() {
            return Err(QuiesceError::OutstandingResult);
        }
        let Some(outcome) = self.retained else {
            return Err(QuiesceError::OutstandingResult);
        };
        if outcome.key() != key {
            return Err(QuiesceError::OutstandingResult);
        }
        self.retired_through = Some(
            self.retired_through
                .map_or(key.sequence, |retired| retired.max(key.sequence)),
        );
        self.retained = None;
        self.retained_snapshot = None;
        self.state = ServiceState::Idle;
        if self.mode == ServiceMode::Quiescing {
            self.mode = ServiceMode::Stopped;
        }
        Ok(())
    }

    pub fn request_quiesce(&mut self) {
        if matches!(self.mode, ServiceMode::Running) {
            self.mode = ServiceMode::Quiescing;
            if self.pending.is_none() && self.retained.is_none() && self.outstanding.is_none() {
                self.mode = ServiceMode::Stopped;
            }
        }
    }

    pub fn stop(&mut self) {
        self.request_quiesce();
    }

    pub fn reset(&mut self) {
        self.request_quiesce();
    }

    pub fn resume(
        &mut self,
        service_epoch: u64,
        session_generation: u32,
        io_epoch: u64,
    ) -> Result<(), QuiesceError> {
        if self.mode != ServiceMode::Stopped {
            return Err(QuiesceError::NotStopped);
        }
        if self.pending.is_some() || self.retained.is_some() || self.outstanding.is_some() {
            return Err(QuiesceError::OutstandingResult);
        }
        if self.health.write_locked {
            return Err(QuiesceError::Faulted);
        }
        if service_epoch <= self.service_epoch || io_epoch <= self.io_epoch {
            return Err(QuiesceError::EpochNotAdvanced);
        }
        self.service_epoch = service_epoch;
        self.session_generation = session_generation;
        self.io_epoch = io_epoch;
        self.retired_through = None;
        self.mode = ServiceMode::Running;
        Ok(())
    }

    fn make_header(
        &mut self,
        slot: Slot,
        kind: CommandKind,
        offset: u32,
        len: u16,
    ) -> Result<CommandHeader, CompletionError> {
        let command_sequence = self.next_command_sequence;
        self.next_command_sequence = self
            .next_command_sequence
            .checked_add(1)
            .ok_or(CompletionError::CommandSequenceExhausted)?;
        Ok(CommandHeader {
            io_epoch: self.io_epoch,
            command_sequence,
            slot,
            kind,
            offset,
            len,
        })
    }

    pub fn take_command(&mut self, now: u64) -> Result<Option<BackendCommand>, CompletionError> {
        self.advance_time(now);
        if self.quiesce.is_some() {
            return Ok(None);
        }
        if self.outstanding.is_none()
            && self.pending.is_some()
            && self.health.write_locked
            && self.state != ServiceState::VerifyCommitted
        {
            self.reconcile_idle();
        }
        if self.outstanding.is_some() || self.pending.is_none() {
            return Ok(None);
        }
        let pending = self.pending.expect("checked above");
        let (kind, offset, len) = match self.state {
            ServiceState::QualifyErase
            | ServiceState::VerifyErased
            | ServiceState::VerifyBody
            | ServiceState::VerifyCommitted => {
                if self.scan.offset >= self.geometry.erase_bytes() {
                    return Ok(None);
                }
                let remaining = self.geometry.erase_bytes() - self.scan.offset;
                (
                    CommandKind::Read,
                    self.scan.offset,
                    remaining.min(256) as u16,
                )
            }
            ServiceState::AcquireWear => (
                CommandKind::ConsumeErasePermit {
                    lease: self.backend.lease,
                    issuance: self.next_issuance,
                },
                0,
                0,
            ),
            ServiceState::Erasing => (
                CommandKind::Erase {
                    permit: pending.permit.expect("granted permit"),
                },
                0,
                0,
            ),
            ServiceState::ProgramBody => (
                CommandKind::Program,
                u32::from(pending.body_offset),
                self.geometry.granule_bytes(),
            ),
            ServiceState::ProgramCommit => (
                CommandKind::Program,
                u32::from(self.geometry.marker_offset()),
                self.geometry.granule_bytes(),
            ),
            ServiceState::Idle | ServiceState::Retained => return Ok(None),
        };
        let header = match self.make_header(pending.target, kind, offset, len) {
            Ok(header) => header,
            Err(error) => {
                self.finish_failed(
                    self.failure_phase(),
                    FailureKind::SequenceExhausted,
                    !pending.marker_may_have_changed,
                    true,
                );
                return Err(error);
            }
        };
        let mut data = [0xFF; MAX_PAYLOAD_BYTES];
        if kind == CommandKind::Program {
            let start = offset as usize;
            let end = start + usize::from(len);
            data[..usize::from(len)].copy_from_slice(&pending.image.bytes()[start..end]);
        }
        if self.state == ServiceState::ProgramCommit {
            self.pending
                .as_mut()
                .expect("active save")
                .marker_may_have_changed = true;
        }
        let t = self.backend.timing;
        let duration = match kind {
            CommandKind::Read => t.read,
            CommandKind::ConsumeErasePermit { .. } => t.permit,
            CommandKind::Erase { .. } => t.erase,
            CommandKind::Program => t.program,
        };
        self.command_deadline = now.saturating_add(duration);
        self.outstanding = Some(header);
        Ok(Some(BackendCommand { header, data }))
    }

    pub fn complete(&mut self, completion: Completion) -> Result<(), CompletionError> {
        let Some(expected) = self.outstanding else {
            return Err(CompletionError::NothingOutstanding);
        };
        if completion.header != expected {
            self.fence_completion_fault();
            return Err(CompletionError::CorrelationMismatch);
        }
        let status_matches = matches!(
            (expected.kind, completion.status),
            (CommandKind::Read, CompletionStatus::Read { .. })
                | (CommandKind::Read, CompletionStatus::Failed { .. })
                | (
                    CommandKind::ConsumeErasePermit { .. },
                    CompletionStatus::WearGranted { .. }
                )
                | (
                    CommandKind::ConsumeErasePermit { .. },
                    CompletionStatus::WearDenied
                )
                | (
                    CommandKind::ConsumeErasePermit { .. },
                    CompletionStatus::Failed { .. }
                )
                | (CommandKind::Erase { .. }, CompletionStatus::Ok)
                | (CommandKind::Erase { .. }, CompletionStatus::Failed { .. })
                | (CommandKind::Program, CompletionStatus::Ok)
                | (CommandKind::Program, CompletionStatus::Failed { .. })
        );
        if !status_matches {
            self.fence_completion_fault();
            return Err(CompletionError::WrongCompletionKind);
        }
        if let CompletionStatus::Read { len, .. } = completion.status
            && (len != expected.len || usize::from(len) > completion.data.len())
        {
            self.fence_completion_fault();
            return Err(CompletionError::InvalidReadLength);
        }
        if matches!(
            completion.status,
            CompletionStatus::Failed { quiescent: false }
        ) {
            self.begin_uncertain(FailureKind::Backend);
            return Ok(());
        }
        self.outstanding = None;
        if self.interrupted {
            // An unsent control request can be withdrawn. A published one must drain.
            if self.quiesce.is_some_and(|q| !q.published) {
                self.quiesce = None;
            }
            if self.quiesce.is_none() {
                self.reconcile_idle();
            }
            return Ok(());
        }
        if self.health.write_locked && self.state != ServiceState::VerifyCommitted {
            self.reconcile_idle();
            return Ok(());
        }
        match completion.status {
            CompletionStatus::Failed { quiescent } => {
                self.handle_backend_failure(quiescent);
                return Ok(());
            }
            CompletionStatus::WearDenied => {
                self.finish_failed(
                    FailurePhase::Wear,
                    FailureKind::WearUnavailable,
                    true,
                    false,
                );
                return Ok(());
            }
            CompletionStatus::Read { len, issue } => {
                if let Some(issue) = issue {
                    self.scan.issue = Some(issue);
                    let before_marker = !self.pending.expect("active save").marker_may_have_changed;
                    self.finish_failed(
                        self.failure_phase(),
                        FailureKind::Read(issue),
                        before_marker,
                        true,
                    );
                    return Ok(());
                }
                self.consume_read(&completion.data[..usize::from(len)]);
                if self.scan.offset == self.geometry.erase_bytes() {
                    self.finish_scan();
                }
                return Ok(());
            }
            CompletionStatus::WearGranted { permit } => {
                if self.state != ServiceState::AcquireWear {
                    return Err(CompletionError::WrongCompletionKind);
                }
                if permit == 0 || self.next_issuance == u64::MAX {
                    self.finish_failed(
                        FailurePhase::Wear,
                        FailureKind::SequenceExhausted,
                        true,
                        true,
                    );
                    return Ok(());
                }
                self.pending.as_mut().expect("active save").permit = Some(permit);
                self.next_issuance += 1;
                self.state = ServiceState::Erasing;
                return Ok(());
            }
            CompletionStatus::Ok => {}
        }
        match self.state {
            ServiceState::Erasing => {
                self.scan.reset();
                self.state = ServiceState::VerifyErased;
            }
            ServiceState::ProgramBody => {
                let pending = self.pending.as_mut().expect("active save");
                pending.body_offset += self.geometry.granule_bytes();
                if pending.body_offset == self.geometry.body_span() {
                    self.scan.reset();
                    self.state = ServiceState::VerifyBody;
                }
            }
            ServiceState::ProgramCommit => {
                self.start_verification();
            }
            _ => return Err(CompletionError::WrongCompletionKind),
        }
        Ok(())
    }

    fn fence_completion_fault(&mut self) {
        self.health.correlation_fault = true;
        self.health.write_locked = true;
        self.mode = ServiceMode::ReadOnlyFault;
    }

    fn consume_read(&mut self, bytes: &[u8]) {
        let start = self.scan.offset as usize;
        for (relative, &byte) in bytes.iter().enumerate() {
            let absolute = start + relative;
            self.scan.all_ff &= byte == 0xFF;
            if absolute < usize::from(self.geometry.programmed_span()) {
                self.scan.prefix[absolute] = byte;
            } else {
                self.scan.tail_ff &= byte == 0xFF;
            }
        }
        self.scan.offset += u32::try_from(bytes.len()).expect("read chunks are at most 256 bytes");
    }

    fn finish_scan(&mut self) {
        let pending = self.pending.expect("scan belongs to active save");
        let prefix = &self.scan.prefix[..usize::from(self.geometry.programmed_span())];
        match self.state {
            ServiceState::QualifyErase => {
                let classification = classify_scanned(
                    prefix,
                    self.scan.all_ff,
                    self.scan.tail_ff,
                    self.scan.issue,
                    self.geometry,
                    &self.schema,
                );
                let cached = self.recovery.target_classification();
                let unchanged = match (classification, cached) {
                    (Classification::Valid(a), Classification::Valid(b)) => {
                        a.persistent_identity_eq(&b)
                    }
                    _ => classification == cached,
                };
                if !unchanged {
                    self.finish_failed(
                        FailurePhase::Qualification,
                        FailureKind::RecoveryChanged,
                        true,
                        true,
                    );
                    return;
                }
                let selected = self.recovery.selected().map(|selected| selected.record);
                let qualification = qualify_scanned(
                    prefix,
                    self.scan.tail_ff,
                    self.geometry,
                    &classification,
                    selected.as_ref(),
                );
                if qualification == EraseQualification::Unsafe {
                    self.finish_failed(
                        FailurePhase::Qualification,
                        FailureKind::UnsafeErasePrestate,
                        true,
                        true,
                    );
                } else {
                    self.state = ServiceState::AcquireWear;
                }
            }
            ServiceState::VerifyErased => {
                if self.scan.all_ff {
                    self.pending.as_mut().expect("active save").body_offset = 0;
                    self.state = ServiceState::ProgramBody;
                } else {
                    self.finish_failed(FailurePhase::Erase, FailureKind::Verification, true, true);
                }
            }
            ServiceState::VerifyBody => {
                let body_end = usize::from(self.geometry.body_span());
                let marker_end = usize::from(self.geometry.programmed_span());
                if prefix[..body_end] != pending.image.bytes()[..body_end]
                    || prefix[body_end..marker_end]
                        .iter()
                        .any(|&byte| byte != 0xFF)
                    || !self.scan.tail_ff
                    || pending.record.snapshot.validate_for(&self.schema).is_err()
                    || !matches!(crate::codec::classify_body_scanned(prefix, self.scan.tail_ff, self.geometry, &self.schema),
                        Classification::Valid(decoded) if decoded.persistent_identity_eq(&pending.record))
                {
                    self.finish_failed(FailurePhase::Body, FailureKind::Verification, true, true);
                } else {
                    self.state = ServiceState::ProgramCommit;
                }
            }
            ServiceState::VerifyCommitted => {
                let classification = classify_scanned(
                    prefix,
                    self.scan.all_ff,
                    self.scan.tail_ff,
                    self.scan.issue,
                    self.geometry,
                    &self.schema,
                );
                let exact_record = matches!(
                    classification,
                    Classification::Valid(decoded)
                        if decoded.sequence == pending.record.sequence
                            && decoded.persistent_identity_eq(&pending.record)
                );
                if prefix != pending.image.bytes() || !exact_record {
                    self.finish_failed(
                        FailurePhase::Verification,
                        FailureKind::Verification,
                        // Healthy full classification proves absence only for an invalid record.
                        !matches!(
                            classification,
                            Classification::Valid(_)
                                | Classification::Unsupported(_)
                                | Classification::Unreadable(_)
                        ),
                        true,
                    );
                } else {
                    self.finish_durable();
                }
            }
            _ => {}
        }
    }

    fn failure_phase(&self) -> FailurePhase {
        match self.state {
            ServiceState::QualifyErase => FailurePhase::Qualification,
            ServiceState::AcquireWear => FailurePhase::Wear,
            ServiceState::Erasing | ServiceState::VerifyErased => FailurePhase::Erase,
            ServiceState::ProgramBody | ServiceState::VerifyBody => FailurePhase::Body,
            ServiceState::ProgramCommit => FailurePhase::Commit,
            ServiceState::VerifyCommitted => FailurePhase::Verification,
            ServiceState::Idle | ServiceState::Retained => FailurePhase::Admission,
        }
    }

    fn handle_backend_failure(&mut self, quiescent: bool) {
        debug_assert!(quiescent);
        if self.pending.expect("active save").marker_may_have_changed
            && !self.pending.expect("active save").verification_started
        {
            self.health
                .quarantine(self.pending.expect("active save").target);
            self.mode = ServiceMode::ReadOnlyFault;
            self.start_verification();
        } else {
            let before_marker = !self.pending.expect("active save").marker_may_have_changed;
            self.finish_failed(
                self.failure_phase(),
                FailureKind::Backend,
                before_marker,
                true,
            );
        }
    }

    fn start_verification(&mut self) {
        let pending = self.pending.as_mut().expect("active save");
        if pending.verification_started {
            self.finish_failed(self.failure_phase(), FailureKind::Backend, false, true);
        } else {
            pending.verification_started = true;
            self.retained = None;
            self.scan.reset();
            self.state = ServiceState::VerifyCommitted;
        }
    }

    fn reconcile_idle(&mut self) {
        debug_assert!(self.outstanding.is_none() && self.quiesce.is_none());
        self.interrupted = false;
        if self.pending.expect("active save").marker_may_have_changed {
            self.start_verification();
        } else {
            self.finish_failed(self.failure_phase(), FailureKind::Backend, true, true);
        }
    }

    /// True only after command/control/capture reconciliation; diagnostic outcomes
    /// while false are observable but cannot be released or used to acknowledge drain.
    #[must_use]
    pub const fn ownership_settled(&self) -> bool {
        self.outstanding.is_none() && self.quiesce.is_none() && self.pending.is_none()
    }

    fn begin_uncertain(&mut self, reason: FailureKind) {
        let pending = self.pending.expect("outstanding owns capture");
        self.health.quarantine(pending.target);
        self.mode = ServiceMode::ReadOnlyFault;
        self.interrupted = true;
        self.retained = Some(Outcome::Indeterminate {
            key: pending.key,
            phase: self.failure_phase(),
            reason,
        });
        self.retained_snapshot = Some(pending.record.snapshot);
        if !self.quiesce_attempted {
            self.quiesce_attempted = true;
            self.quiesce = Some(QuiesceControl {
                request: QuiesceRequest {
                    outstanding: self.outstanding.expect("unresolved work"),
                },
                published: false,
                deadline: 0,
                expired: false,
            });
        }
    }

    /// The owner pump must advance this monotonic clock even when I/O is silent.
    pub fn advance_time(&mut self, now: u64) {
        if self.outstanding.is_some() && !self.interrupted && now >= self.command_deadline {
            self.begin_uncertain(FailureKind::Timeout);
        }
        if let Some(q) = self.quiesce.as_mut()
            && q.published
            && now >= q.deadline
        {
            q.expired = true;
            // Timeout proves neither idle nor drainage. Keep both identities.
        }
    }

    pub fn take_quiesce_request(&mut self, now: u64) -> Option<QuiesceRequest> {
        self.advance_time(now);
        let q = self.quiesce.as_mut()?;
        if q.published {
            return None;
        }
        q.published = true;
        q.deadline = now.saturating_add(self.backend.timing.quiesce);
        Some(q.request)
    }

    pub fn complete_quiesce(
        &mut self,
        request: QuiesceRequest,
        idle_and_drained: bool,
    ) -> Result<(), CompletionError> {
        let Some(q) = self.quiesce else {
            return Err(CompletionError::NothingOutstanding);
        };
        if !q.published || q.request != request {
            self.fence_completion_fault();
            return Err(CompletionError::CorrelationMismatch);
        }
        self.quiesce = None;
        if idle_and_drained {
            self.outstanding = None;
        }
        if self.outstanding.is_none() {
            self.reconcile_idle();
        }
        Ok(())
    }

    fn finish_failed(
        &mut self,
        phase: FailurePhase,
        reason: FailureKind,
        no_new_commit: bool,
        quarantine: bool,
    ) {
        debug_assert!(self.outstanding.is_none() && self.quiesce.is_none());
        let pending = self.pending.take().expect("failure belongs to active save");
        if quarantine {
            self.health.quarantine(pending.target);
            self.mode = ServiceMode::ReadOnlyFault;
        }
        let outcome = if no_new_commit {
            Outcome::Failed {
                key: pending.key,
                phase,
                reason,
                no_new_commit: true,
            }
        } else {
            Outcome::Indeterminate {
                key: pending.key,
                phase,
                reason,
            }
        };
        self.retained = Some(outcome);
        self.retained_snapshot = Some(pending.record.snapshot);
        self.state = ServiceState::Retained;
    }

    fn finish_durable(&mut self) {
        let pending = self.pending.take().expect("durable active save");
        match pending.target {
            Slot::A => self.recovery.a = Classification::Valid(pending.record),
            Slot::B => self.recovery.b = Classification::Valid(pending.record),
        }
        self.recovery = recover(self.recovery.a, self.recovery.b, &self.schema)
            .expect("existing validated defaults remain valid");
        self.retained = Some(Outcome::Durable {
            key: pending.key,
            record: pending.record,
            existing: false,
        });
        self.retained_snapshot = Some(pending.record.snapshot);
        self.state = ServiceState::Retained;
    }
}
