use crate::EncodedValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationKey {
    pub service_epoch: u64,
    pub session_generation: u32,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplyField {
    pub key: OperationKey,
    pub field_id: u32,
    pub encoded_value: EncodedValue,
    pub expires_at_us: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplyRequest {
    pub field: ApplyField,
    pub expected_revision: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveRevision(u32);

impl ActiveRevision {
    pub const INITIAL: Self = Self(0);

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectReason {
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
    Policy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateError {
    Busy,
    NotReady,
    Quiescing,
    Faulted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitRecord<V: Copy> {
    pub key: OperationKey,
    pub active_values: V,
    pub active_revision: ActiveRevision,
    pub applied_at_us: u64,
}

/// Bounded admission and infallible publication at the owner linearization point.
///
/// Physical execution and its status are deliberately outside this interface.
/// `publish` cannot turn an already committed software value into a rejection.
/// An unpublished permit must cancel its reservation on Drop, without publishing
/// or changing phase/output state. Permit cancellation and publication must be
/// synchronous, bounded, infallible and allocation-free.
pub trait CommitGate<V: Copy> {
    type Permit;

    fn try_reserve(&mut self, next: &V) -> Result<Self::Permit, GateError>;
    fn publish(&mut self, permit: Self::Permit, record: CommitRecord<V>);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerOutcome<V: Copy> {
    Applied {
        key: OperationKey,
        changed: bool,
        active_values: V,
        active_revision: ActiveRevision,
        applied_at_us: u64,
    },
    Rejected {
        key: OperationKey,
        reason: RejectReason,
        active_values: V,
        active_revision: ActiveRevision,
    },
    Cancelled {
        key: OperationKey,
        active_values: V,
        active_revision: ActiveRevision,
    },
    Retired {
        key: OperationKey,
        active_values: V,
        active_revision: ActiveRevision,
    },
    StaleOperation {
        key: OperationKey,
        active_values: V,
        active_revision: ActiveRevision,
    },
    Busy {
        key: OperationKey,
        active_values: V,
        active_revision: ActiveRevision,
    },
}

impl<V: Copy> OwnerOutcome<V> {
    #[must_use]
    pub const fn key(self) -> OperationKey {
        match self {
            Self::Applied { key, .. }
            | Self::Rejected { key, .. }
            | Self::Cancelled { key, .. }
            | Self::Retired { key, .. }
            | Self::StaleOperation { key, .. }
            | Self::Busy { key, .. } => key,
        }
    }

    #[must_use]
    pub const fn is_terminal_retained(self) -> bool {
        matches!(
            self,
            Self::Applied { .. } | Self::Rejected { .. } | Self::Cancelled { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerMode {
    Running,
    Stopped,
    Reconciling { next_service_epoch: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseError {
    WrongKey,
    NothingRetained,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetError {
    ServiceEpochExhausted,
    NotReconciling,
    OutstandingOutcome,
}

/// Single-owner calibration state with one retained transaction outcome.
///
/// The active value, revision, retirement watermark and lifecycle fields are
/// private. All mutation passes through `apply` or explicit lifecycle methods.
pub struct Owner<V: Copy + Eq> {
    active: V,
    active_revision: ActiveRevision,
    service_epoch: u64,
    session_generation: u32,
    retired_through: Option<u64>,
    retained: Option<OwnerOutcome<V>>,
    mode: OwnerMode,
}

impl<V: Copy + Eq> Owner<V> {
    #[must_use]
    pub const fn new(active: V, service_epoch: u64, session_generation: u32) -> Self {
        Self {
            active,
            active_revision: ActiveRevision::INITIAL,
            service_epoch,
            session_generation,
            retired_through: None,
            retained: None,
            mode: OwnerMode::Running,
        }
    }

    #[must_use]
    pub const fn active(&self) -> V {
        self.active
    }

    #[must_use]
    pub const fn active_revision(&self) -> ActiveRevision {
        self.active_revision
    }

    #[must_use]
    pub const fn service_epoch(&self) -> u64 {
        self.service_epoch
    }

    #[must_use]
    pub const fn session_generation(&self) -> u32 {
        self.session_generation
    }

    #[must_use]
    pub const fn mode(&self) -> OwnerMode {
        self.mode
    }

    #[must_use]
    pub const fn retained(&self) -> Option<OwnerOutcome<V>> {
        self.retained
    }

    #[must_use]
    pub const fn retired_through(&self) -> Option<u64> {
        self.retired_through
    }

    /// Validate and decide an operation using a fresh final clock sample.
    ///
    /// `clock` is an integration-trusted, bounded, infallible, synchronous read
    /// of current monotonic microseconds in the request deadline's time domain.
    /// It must not return a saved entry timestamp. It is sampled on entry and,
    /// after validation, at the no-op decision or after successful reservation
    /// immediately before changed-state mutation. The final sample is used for
    /// both the publication record and retained Applied outcome.
    ///
    /// Hooks, captures and their cleanup must obey the startup-only heap rule;
    /// borrow process-lifetime resources rather than destroy them after apply.
    /// Candidate validation and gate operations must also be bounded and
    /// synchronous. No fallible callback or await may occur once commit starts.
    pub fn apply<G, F, C>(
        &mut self,
        request: ApplyRequest,
        mut clock: C,
        make_candidate: F,
        gate: &mut G,
    ) -> OwnerOutcome<V>
    where
        G: CommitGate<V>,
        C: FnMut() -> u64,
        F: FnOnce(V, u32, EncodedValue) -> Result<V, RejectReason>,
    {
        let key = request.field.key;
        if let Some(outcome) = self.classify_existing(key) {
            return outcome;
        }
        if !matches!(self.mode, OwnerMode::Running) {
            return self.stale_or_lifecycle(key);
        }
        let now_us = clock();
        if now_us >= request.field.expires_at_us {
            return self.retain_rejected(key, RejectReason::Expired);
        }
        if request
            .expected_revision
            .is_some_and(|expected| expected != self.active_revision.get())
        {
            return self.retain_rejected(key, RejectReason::RevisionMismatch);
        }

        let candidate = match make_candidate(
            self.active,
            request.field.field_id,
            request.field.encoded_value,
        ) {
            Ok(candidate) => candidate,
            Err(reason) => return self.retain_rejected(key, reason),
        };

        if candidate == self.active {
            let now_us = clock();
            if now_us >= request.field.expires_at_us {
                return self.retain_rejected(key, RejectReason::Expired);
            }
            let outcome = OwnerOutcome::Applied {
                key,
                changed: false,
                active_values: self.active,
                active_revision: self.active_revision,
                applied_at_us: now_us,
            };
            self.retained = Some(outcome);
            return outcome;
        }

        let permit = match gate.try_reserve(&candidate) {
            Ok(permit) => permit,
            Err(error) => return self.retain_rejected(key, gate_reject_reason(error)),
        };

        // Final local deadline check immediately before the infallible commit.
        let now_us = clock();
        if now_us >= request.field.expires_at_us {
            drop(permit);
            return self.retain_rejected(key, RejectReason::Expired);
        }

        self.active = candidate;
        self.active_revision = self.active_revision.next();
        let record = CommitRecord {
            key,
            active_values: self.active,
            active_revision: self.active_revision,
            applied_at_us: now_us,
        };
        gate.publish(permit, record);

        let outcome = OwnerOutcome::Applied {
            key,
            changed: true,
            active_values: self.active,
            active_revision: self.active_revision,
            applied_at_us: now_us,
        };
        self.retained = Some(outcome);
        outcome
    }

    pub fn resolve_or_cancel(&mut self, key: OperationKey) -> OwnerOutcome<V> {
        if let Some(outcome) = self.classify_existing(key) {
            return outcome;
        }
        if !matches!(self.mode, OwnerMode::Running) {
            return self.stale_or_lifecycle(key);
        }
        let outcome = OwnerOutcome::Cancelled {
            key,
            active_values: self.active,
            active_revision: self.active_revision,
        };
        self.retained = Some(outcome);
        outcome
    }

    pub fn release(&mut self, key: OperationKey) -> Result<(), ReleaseError> {
        if self.key_is_current(key)
            && self
                .retired_through
                .is_some_and(|retired| key.sequence <= retired)
        {
            return Ok(());
        }
        let Some(retained) = self.retained else {
            return Err(ReleaseError::NothingRetained);
        };
        if retained.key() != key {
            return Err(ReleaseError::WrongKey);
        }
        self.retired_through = Some(
            self.retired_through
                .map_or(key.sequence, |retired| retired.max(key.sequence)),
        );
        self.retained = None;
        Ok(())
    }

    pub fn stop(&mut self) {
        if matches!(self.mode, OwnerMode::Running) {
            self.mode = OwnerMode::Stopped;
        }
    }

    pub fn start(&mut self) {
        if matches!(self.mode, OwnerMode::Stopped) {
            self.mode = OwnerMode::Running;
        }
    }

    pub fn begin_coordinated_reset(&mut self) -> Result<u64, ResetError> {
        if let OwnerMode::Reconciling { next_service_epoch } = self.mode {
            return Ok(next_service_epoch);
        }
        let next_service_epoch = self
            .service_epoch
            .checked_add(1)
            .ok_or(ResetError::ServiceEpochExhausted)?;
        self.mode = OwnerMode::Reconciling { next_service_epoch };
        Ok(next_service_epoch)
    }

    pub fn finish_coordinated_reset(
        &mut self,
        new_session_generation: u32,
    ) -> Result<(), ResetError> {
        let OwnerMode::Reconciling { next_service_epoch } = self.mode else {
            return Err(ResetError::NotReconciling);
        };
        if self.retained.is_some() {
            return Err(ResetError::OutstandingOutcome);
        }
        self.service_epoch = next_service_epoch;
        self.session_generation = new_session_generation;
        self.retired_through = None;
        self.mode = OwnerMode::Running;
        Ok(())
    }

    fn classify_existing(&self, key: OperationKey) -> Option<OwnerOutcome<V>> {
        if self.retained.is_some_and(|outcome| outcome.key() == key) {
            return self.retained;
        }
        if !self.key_is_current(key) {
            return Some(self.stale(key));
        }
        if self
            .retired_through
            .is_some_and(|retired| key.sequence <= retired)
        {
            return Some(OwnerOutcome::Retired {
                key,
                active_values: self.active,
                active_revision: self.active_revision,
            });
        }
        if self.retained.is_some() {
            return Some(OwnerOutcome::Busy {
                key,
                active_values: self.active,
                active_revision: self.active_revision,
            });
        }
        None
    }

    fn key_is_current(&self, key: OperationKey) -> bool {
        key.service_epoch == self.service_epoch && key.session_generation == self.session_generation
    }

    fn stale_or_lifecycle(&mut self, key: OperationKey) -> OwnerOutcome<V> {
        if self.key_is_current(key) && matches!(self.mode, OwnerMode::Stopped) {
            self.retain_rejected(key, RejectReason::WrongLifecycle)
        } else {
            self.stale(key)
        }
    }

    const fn stale(&self, key: OperationKey) -> OwnerOutcome<V> {
        OwnerOutcome::StaleOperation {
            key,
            active_values: self.active,
            active_revision: self.active_revision,
        }
    }

    fn retain_rejected(&mut self, key: OperationKey, reason: RejectReason) -> OwnerOutcome<V> {
        let outcome = OwnerOutcome::Rejected {
            key,
            reason,
            active_values: self.active,
            active_revision: self.active_revision,
        };
        self.retained = Some(outcome);
        outcome
    }
}

const fn gate_reject_reason(error: GateError) -> RejectReason {
    match error {
        GateError::Busy => RejectReason::OutputBusy,
        GateError::NotReady | GateError::Quiescing | GateError::Faulted => {
            RejectReason::OutputUnavailable
        }
    }
}
