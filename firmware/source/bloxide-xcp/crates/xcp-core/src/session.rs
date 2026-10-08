use crate::map::{ReadResolveError, VirtualMap, WriteResolveError};
use crate::provider::{
    ApplyRequest, ProviderOperation, ProviderPort, ProviderRequest, ReadRequest, SubmitError,
};
use crate::{command, ErrorCode, CONNECT_RESOURCE, MAX_DTO, PROTOCOL_LAYER_VERSION};
use crate::{COMM_MODE_BASIC, TRANSPORT_LAYER_VERSION};
use xcp_messages::{
    Correlation, Packet, ProviderCompletion, QuiesceResult, ReadResult, RejectionReason,
    SynchronizeResult, WriteResult,
};

pub const APPLY_DEADLINE_US: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Disconnected,
    Synchronizing,
    Ready,
    AwaitingOwner,
    Resolving,
    Quiescing,
    Recovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dispatch {
    Respond(Packet),
    Deferred,
    Ignored,
    Fenced,
}

/// Result of one bounded owner-control step. `dispatch` includes durable
/// delivery state; `new_fault` identifies a provider failure observed in this
/// call, even if delivery was already fenced. It is not stored across calls.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceResult {
    pub dispatch: Dispatch,
    pub new_fault: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending {
    StoreCalibration { correlation: Correlation },
    Synchronize {
        generation: u32,
        fallback: SessionState,
    },
    Read {
        correlation: Correlation,
        next_mta: u32,
        expected_len: u8,
    },
    Write(PendingWrite),
    Quiesce {
        correlation: Correlation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingWrite {
    operation: ProviderOperation,
    next_mta: u32,
    expires_at_us: u64,
    reply_allowed: bool,
    resolving: bool,
    resolve_admission: Admission,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Retirement {
    operation: ProviderOperation,
    admission: Admission,
}

/// Retry readiness must not erase evidence of an earlier uncertain handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Admission {
    NotSubmitted,
    Uncertain,
    Confirmed,
}

/// Bounded scalar-profile protocol state. It owns no transport or owner
/// resource and contains no heap-backed data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Session {
    state: SessionState,
    generation: u32,
    attempt_generation: u32,
    delivery_fenced: bool,
    service_epoch: Option<u64>,
    next_sequence: u64,
    mta: Option<u32>,
    pending: Option<Pending>,
    retirement: Option<Retirement>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub const fn new() -> Self {
        Self {
            state: SessionState::Disconnected,
            generation: 0,
            attempt_generation: 0,
            delivery_fenced: false,
            service_epoch: None,
            next_sequence: 0,
            mta: None,
            pending: None,
            retirement: None,
        }
    }

    pub const fn state(&self) -> SessionState {
        self.state
    }

    pub const fn mta(&self) -> Option<u32> {
        self.mta
    }

    pub const fn session_generation(&self) -> u32 {
        self.generation
    }

    pub const fn service_epoch(&self) -> Option<u64> {
        self.service_epoch
    }

    pub const fn has_retirement(&self) -> bool {
        self.retirement.is_some()
    }

    pub const fn has_pending_work(&self) -> bool {
        self.pending.is_some() || self.retirement.is_some()
    }

    /// The generated machine selects its next leaf from protocol facts. The
    /// legacy standalone `state` field is never read by these predicates.
    pub const fn target_recovery(&self) -> bool {
        self.delivery_fenced && !matches!(self.pending, Some(Pending::Write(_)))
    }

    pub const fn target_closed(&self) -> bool {
        !self.delivery_fenced && self.pending.is_none() && self.service_epoch.is_none()
    }

    pub const fn target_synchronizing(&self) -> bool {
        matches!(self.pending, Some(Pending::Synchronize { .. })) && !self.delivery_fenced
    }

    pub const fn target_ready(&self) -> bool {
        !self.delivery_fenced && self.pending.is_none() && self.service_epoch.is_some()
    }

    pub const fn target_awaiting_owner(&self) -> bool {
        !self.delivery_fenced
            && (matches!(self.pending, Some(Pending::Read { .. }) | Some(Pending::StoreCalibration { .. }))
                || matches!(
                    self.pending,
                    Some(Pending::Write(PendingWrite {
                        resolving: false,
                        ..
                    }))
                ))
    }

    pub const fn target_resolving(&self) -> bool {
        matches!(
            self.pending,
            Some(Pending::Write(PendingWrite {
                resolving: true,
                ..
            }))
        )
    }

    pub const fn target_quiescing(&self) -> bool {
        matches!(self.pending, Some(Pending::Quiesce { .. })) && !self.delivery_fenced
    }

    pub fn handle_packet<P: ProviderPort>(
        &mut self,
        map: &VirtualMap<'_>,
        packet: &Packet,
        now_us: u64,
        provider: &mut P,
    ) -> Dispatch {
        let bytes = packet.as_slice();
        let dispatch = match self.state {
            SessionState::Disconnected | SessionState::Recovery => {
                if bytes[0] == command::CONNECT {
                    self.start_connect(bytes, provider)
                } else {
                    Dispatch::Ignored
                }
            }
            SessionState::Ready => self.handle_ready(map, bytes, now_us, provider),
            SessionState::AwaitingOwner
            | SessionState::Resolving
            | SessionState::Synchronizing
            | SessionState::Quiescing => self.fence(provider),
        };
        if self.delivery_fenced && matches!(dispatch, Dispatch::Respond(_)) {
            Dispatch::Fenced
        } else {
            dispatch
        }
    }

    /// Selected generated Ready leaf owns this command route. The core's
    /// standalone phase field does not decide this route or the next leaf.
    pub fn handle_ready_phase<P: ProviderPort>(
        &mut self,
        map: &VirtualMap<'_>,
        packet: &Packet,
        now_us: u64,
        provider: &mut P,
    ) -> Dispatch {
        self.state = SessionState::Ready;
        let bytes = packet.as_slice();
        let dispatch = if bytes[0] == command::CONNECT {
            self.start_connect_from(bytes, SessionState::Ready, provider)
        } else {
            self.handle_ready(map, bytes, now_us, provider)
        };
        if self.delivery_fenced && matches!(dispatch, Dispatch::Respond(_)) {
            Dispatch::Fenced
        } else {
            dispatch
        }
    }

    /// Selected Disconnected/Closed/Recovery leaves accept only CONNECT.
    pub fn handle_connect_phase<P: ProviderPort>(
        &mut self,
        phase: SessionState,
        packet: &Packet,
        provider: &mut P,
    ) -> Dispatch {
        debug_assert!(matches!(
            phase,
            SessionState::Disconnected | SessionState::Recovery
        ));
        self.state = phase;
        let bytes = packet.as_slice();
        if bytes[0] == command::CONNECT {
            self.start_connect_from(bytes, SessionState::Disconnected, provider)
        } else {
            Dispatch::Ignored
        }
    }

    /// Packet during an owner wait or quiesce has uncertain response
    /// ownership; the generated busy leaves route it to the fence.
    pub fn handle_busy_phase<P: ProviderPort>(&mut self, provider: &mut P) -> Dispatch {
        self.fence(provider)
    }

    fn handle_ready<P: ProviderPort>(
        &mut self,
        map: &VirtualMap<'_>,
        bytes: &[u8],
        now_us: u64,
        provider: &mut P,
    ) -> Dispatch {
        match bytes[0] {
            command::CONNECT => self.start_connect(bytes, provider),
            command::DISCONNECT => self.start_disconnect(bytes, provider),
            command::GET_STATUS => exact_len(bytes, 1).map_or_else(error_dispatch, |_| {
                Dispatch::Respond(packet(&[0xFF, 0, 0, 0, 0, 0]))
            }),
            command::SYNCH => exact_len(bytes, 1).map_or_else(error_dispatch, |_| {
                Dispatch::Respond(error_packet(ErrorCode::CommandSynch))
            }),
            0xf9 if cfg!(feature = "calibration-store") => self.start_store_calibration(bytes, provider),
            command::SET_MTA => self.set_mta(bytes),
            command::UPLOAD => self.start_upload(map, bytes, provider),
            command::DOWNLOAD => self.start_download(map, bytes, now_us, provider),
            _ => Dispatch::Respond(error_packet(ErrorCode::CommandUnknown)),
        }
    }

    fn start_connect<P: ProviderPort>(&mut self, bytes: &[u8], provider: &mut P) -> Dispatch {
        let fallback = if self.state == SessionState::Ready {
            SessionState::Ready
        } else {
            SessionState::Disconnected
        };
        self.start_connect_from(bytes, fallback, provider)
    }

    fn start_connect_from<P: ProviderPort>(
        &mut self,
        bytes: &[u8],
        fallback: SessionState,
        provider: &mut P,
    ) -> Dispatch {
        if self.pending.is_some() || self.retirement.is_some() {
            return self.fence(provider);
        }
        if let Err(error) = exact_len(bytes, 2) {
            return error_dispatch(error);
        }
        if bytes[1] != 0 {
            return error_dispatch(ErrorCode::OutOfRange);
        }
        let Some(generation) = self.attempt_generation.checked_add(1) else {
            self.mark_fenced();
            return Dispatch::Fenced;
        };
        let request = ProviderRequest::Synchronize {
            session_generation: generation,
        };
        self.submit_pending(
            request,
            Pending::Synchronize {
                generation,
                fallback,
            },
            SessionState::Synchronizing,
            provider,
        )
    }

    /// Busy alone proves nonadmission. Both other outcomes irrevocably consume
    /// the initial identity and retain completion state before exposing a fence.
    fn submit_pending<P: ProviderPort>(
        &mut self,
        request: ProviderRequest,
        pending: Pending,
        state: SessionState,
        provider: &mut P,
    ) -> Dispatch {
        let admission = provider.try_submit(request);
        if admission == Err(SubmitError::Busy) {
            return error_dispatch(ErrorCode::CommandBusy);
        }
        match pending {
            Pending::Synchronize { generation, .. } => {
                self.attempt_generation = generation;
                self.delivery_fenced = false;
            }
            _ => self.consume_sequence(),
        }
        self.pending = Some(pending);
        self.state = state;
        if admission == Err(SubmitError::Unavailable) {
            self.mark_fenced();
            Dispatch::Fenced
        } else {
            Dispatch::Deferred
        }
    }

    fn start_disconnect<P: ProviderPort>(&mut self, bytes: &[u8], provider: &mut P) -> Dispatch {
        if let Err(error) = exact_len(bytes, 1) {
            return error_dispatch(error);
        }
        let Some(correlation) = self.next_correlation() else {
            return Dispatch::Fenced;
        };
        self.submit_pending(
            ProviderRequest::Quiesce { correlation },
            Pending::Quiesce { correlation },
            SessionState::Quiescing,
            provider,
        )
    }

    fn start_store_calibration<P: ProviderPort>(&mut self, bytes: &[u8], provider: &mut P) -> Dispatch {
        if let Err(error) = exact_len(bytes, 4) { return error_dispatch(error); }
        // Only STORE_CAL_REQ, configuration id zero. No DAQ persistence.
        if bytes[1..] != [1, 0, 0] { return error_dispatch(ErrorCode::OutOfRange); }
        let Some(correlation) = self.next_correlation() else { return Dispatch::Fenced; };
        self.submit_pending(ProviderRequest::StoreCalibration { correlation },
            Pending::StoreCalibration { correlation }, SessionState::AwaitingOwner, provider)
    }

    fn set_mta(&mut self, bytes: &[u8]) -> Dispatch {
        if let Err(error) = exact_len(bytes, 8) {
            return error_dispatch(error);
        }
        if bytes[1] != 0 || bytes[2] != 0 {
            return error_dispatch(ErrorCode::CommandSyntax);
        }
        if bytes[3] != 0 {
            return error_dispatch(ErrorCode::OutOfRange);
        }
        self.mta = Some(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]));
        positive()
    }

    fn start_upload<P: ProviderPort>(
        &mut self,
        map: &VirtualMap<'_>,
        bytes: &[u8],
        provider: &mut P,
    ) -> Dispatch {
        if let Err(error) = exact_len(bytes, 2) {
            return error_dispatch(error);
        }
        let count = bytes[1];
        if !(1..=7).contains(&count) {
            return error_dispatch(ErrorCode::OutOfRange);
        }
        let Some(address) = self.mta else {
            return error_dispatch(ErrorCode::AccessDenied);
        };
        let next_mta = match address.checked_add(u32::from(count)) {
            Some(next) => next,
            None => return error_dispatch(ErrorCode::OutOfRange),
        };
        let resolved = match map.resolve_read(address, count) {
            Ok(resolved) => resolved,
            Err(ReadResolveError::OutOfRange) => return error_dispatch(ErrorCode::OutOfRange),
            Err(ReadResolveError::AccessDenied) => return error_dispatch(ErrorCode::AccessDenied),
        };
        let Some(correlation) = self.next_correlation() else {
            return Dispatch::Fenced;
        };
        let request = ProviderRequest::Read(ReadRequest {
            correlation,
            descriptor_id: resolved.descriptor_id,
            offset: resolved.offset,
            length: count,
        });
        self.submit_pending(
            request,
            Pending::Read {
                correlation,
                next_mta,
                expected_len: count,
            },
            SessionState::AwaitingOwner,
            provider,
        )
    }

    fn start_download<P: ProviderPort>(
        &mut self,
        map: &VirtualMap<'_>,
        bytes: &[u8],
        now_us: u64,
        provider: &mut P,
    ) -> Dispatch {
        if bytes.len() < 2 || bytes.len() != usize::from(bytes[1]) + 2 {
            return error_dispatch(ErrorCode::CommandSyntax);
        }
        if !matches!(bytes[1], 2 | 4) {
            return error_dispatch(ErrorCode::OutOfRange);
        }
        if self.retirement.is_some() {
            return error_dispatch(ErrorCode::CommandBusy);
        }
        let Some(address) = self.mta else {
            return error_dispatch(ErrorCode::AccessDenied);
        };
        let next_mta = match address.checked_add(u32::from(bytes[1])) {
            Some(next) => next,
            None => return error_dispatch(ErrorCode::OutOfRange),
        };
        let resolved = match map.resolve_write(address, bytes[1]) {
            Ok(resolved) => resolved,
            Err(WriteResolveError::OutOfRange) => return error_dispatch(ErrorCode::OutOfRange),
            Err(WriteResolveError::WriteProtected) => {
                return error_dispatch(ErrorCode::WriteProtected)
            }
            Err(WriteResolveError::AccessDenied) => return error_dispatch(ErrorCode::AccessDenied),
        };
        let Some(service_epoch) = self.service_epoch else {
            return error_dispatch(ErrorCode::ModeNotValid);
        };
        let Some(correlation) = self.next_correlation() else {
            return Dispatch::Fenced;
        };
        let Some(expires_at_us) = now_us.checked_add(APPLY_DEADLINE_US) else {
            self.mark_fenced();
            return Dispatch::Fenced;
        };
        let operation = ProviderOperation {
            service_epoch,
            session_generation: correlation.session_generation,
            sequence: correlation.sequence,
        };
        let mut encoded_value = [0u8; 4];
        encoded_value[..bytes[1] as usize].copy_from_slice(&bytes[2..]);
        let request = ProviderRequest::Apply(ApplyRequest {
            operation,
            descriptor_id: resolved.descriptor_id,
            encoded_value,
            length: bytes[1],
            expires_at_us,
        });
        self.submit_pending(
            request,
            Pending::Write(PendingWrite {
                operation,
                next_mta,
                expires_at_us,
                reply_allowed: true,
                resolving: false,
                resolve_admission: Admission::NotSubmitted,
            }),
            SessionState::AwaitingOwner,
            provider,
        )
    }

    pub fn complete<P: ProviderPort>(
        &mut self,
        completion: ProviderCompletion,
        provider: &mut P,
    ) -> Dispatch {
        let dispatch = match completion {
            ProviderCompletion::CalibrationStored { correlation, verified } => {
                if self.pending != Some(Pending::StoreCalibration { correlation }) {
                    self.correlation_fault()
                } else {
                    self.pending = None;
                    if self.delivery_fenced { self.correlation_fault() }
                    else {
                        self.state = SessionState::Ready;
                        if verified { positive() } else { error_dispatch(ErrorCode::Generic) }
                    }
                }
            },
            ProviderCompletion::Synchronized {
                session_generation,
                result,
            } => self.complete_synchronize(session_generation, result),
            ProviderCompletion::Read {
                correlation,
                result,
            } => self.complete_read(correlation, result),
            ProviderCompletion::Write {
                correlation,
                result,
            } => self.complete_write(correlation, result, provider),
            ProviderCompletion::Quiesced {
                correlation,
                result,
            } => self.complete_quiesce(correlation, result),
            ProviderCompletion::Released { correlation } => self.complete_release(correlation),
        };
        if dispatch == Dispatch::Fenced {
            self.fence(provider)
        } else {
            dispatch
        }
    }

    fn complete_synchronize(&mut self, generation: u32, result: SynchronizeResult) -> Dispatch {
        let Some(Pending::Synchronize {
            generation: expected,
            fallback,
        }) = self.pending
        else {
            return self.correlation_fault();
        };
        if generation != expected {
            return self.correlation_fault();
        }
        if result == SynchronizeResult::Uncertain {
            return self.correlation_fault();
        }
        self.pending = None;
        if self.delivery_fenced {
            return self.correlation_fault();
        }
        match result {
            SynchronizeResult::Ready { service_epoch } => {
                self.generation = generation;
                self.service_epoch = Some(service_epoch);
                self.next_sequence = 0;
                self.mta = None;
                self.state = SessionState::Ready;
                Dispatch::Respond(packet(&[
                    0xFF,
                    CONNECT_RESOURCE,
                    COMM_MODE_BASIC,
                    crate::MAX_CTO as u8,
                    MAX_DTO,
                    0,
                    PROTOCOL_LAYER_VERSION,
                    TRANSPORT_LAYER_VERSION,
                ]))
            }
            SynchronizeResult::Busy => {
                self.state = fallback;
                if fallback == SessionState::Disconnected {
                    self.service_epoch = None;
                    self.mta = None;
                }
                error_dispatch(ErrorCode::CommandBusy)
            }
            SynchronizeResult::WrongLifecycle => {
                self.state = fallback;
                if fallback == SessionState::Disconnected {
                    self.service_epoch = None;
                    self.mta = None;
                }
                error_dispatch(ErrorCode::ModeNotValid)
            }
            SynchronizeResult::Uncertain => {
                self.state = SessionState::Recovery;
                Dispatch::Fenced
            }
        }
    }

    fn complete_read(&mut self, correlation: Correlation, result: ReadResult) -> Dispatch {
        let Some(Pending::Read {
            correlation: expected,
            next_mta,
            expected_len,
        }) = self.pending
        else {
            return self.correlation_fault();
        };
        if correlation != expected || result == ReadResult::Uncertain {
            return self.correlation_fault();
        }
        self.pending = None;
        if self.delivery_fenced {
            return self.correlation_fault();
        }
        match result {
            ReadResult::Data(data) if data.len == expected_len => {
                let mut bytes = [0u8; 8];
                bytes[0] = 0xFF;
                bytes[1..=usize::from(expected_len)]
                    .copy_from_slice(&data.bytes[..usize::from(expected_len)]);
                self.mta = Some(next_mta);
                self.state = SessionState::Ready;
                Dispatch::Respond(packet_parts(bytes, expected_len + 1))
            }
            ReadResult::Data(_) => self.correlation_fault(),
            ReadResult::Busy => self.read_error(ErrorCode::CommandBusy),
            ReadResult::OutOfRange => self.read_error(ErrorCode::OutOfRange),
            ReadResult::AccessDenied => self.read_error(ErrorCode::AccessDenied),
            ReadResult::WrongLifecycle => self.read_error(ErrorCode::ModeNotValid),
            ReadResult::Uncertain => {
                self.state = SessionState::Recovery;
                Dispatch::Fenced
            }
        }
    }

    fn read_error(&mut self, error: ErrorCode) -> Dispatch {
        self.state = SessionState::Ready;
        error_dispatch(error)
    }

    fn complete_write<P: ProviderPort>(
        &mut self,
        correlation: Correlation,
        result: WriteResult,
        provider: &mut P,
    ) -> Dispatch {
        let Some(Pending::Write(pending)) = self.pending else {
            return self.correlation_fault();
        };
        if correlation != pending.operation.correlation() {
            return self.fence(provider);
        }
        if result == WriteResult::Uncertain {
            let mut unresolved = pending;
            unresolved.resolve_admission = Admission::NotSubmitted;
            self.pending = Some(Pending::Write(unresolved));
            return self.fence(provider);
        }
        self.pending = None;
        match result {
            WriteResult::Applied { .. } => {
                self.mta = Some(pending.next_mta);
                self.finish_retained(pending, positive(), provider)
            }
            WriteResult::Busy => {
                self.state = if pending.reply_allowed {
                    SessionState::Ready
                } else {
                    SessionState::Recovery
                };
                if pending.reply_allowed {
                    error_dispatch(ErrorCode::CommandBusy)
                } else {
                    Dispatch::Fenced
                }
            }
            WriteResult::Rejected { reason } => {
                let error = match reason {
                    RejectionReason::BadEncoding
                    | RejectionReason::Bounds
                    | RejectionReason::CrossField => ErrorCode::OutOfRange,
                    RejectionReason::ReadOnly => ErrorCode::WriteProtected,
                    RejectionReason::UnknownVariable => ErrorCode::AccessDenied,
                    RejectionReason::Expired => ErrorCode::Generic,
                    RejectionReason::WrongLifecycle => ErrorCode::ModeNotValid,
                    RejectionReason::RevisionMismatch
                    | RejectionReason::OutputBusy
                    | RejectionReason::OutputUnavailable => ErrorCode::CommandBusy,
                    RejectionReason::UnsupportedPolicy => {
                        let mut fenced = pending;
                        fenced.reply_allowed = false;
                        return self.finish_retained(fenced, Dispatch::Fenced, provider);
                    }
                };
                self.finish_retained(pending, error_dispatch(error), provider)
            }
            WriteResult::Cancelled => {
                self.finish_retained(pending, error_dispatch(ErrorCode::Generic), provider)
            }
            WriteResult::Retired | WriteResult::StaleOperation | WriteResult::Uncertain => {
                self.state = SessionState::Recovery;
                Dispatch::Fenced
            }
        }
    }

    fn finish_retained<P: ProviderPort>(
        &mut self,
        pending: PendingWrite,
        response: Dispatch,
        provider: &mut P,
    ) -> Dispatch {
        self.state = if pending.reply_allowed {
            SessionState::Ready
        } else {
            SessionState::Recovery
        };
        self.retirement = Some(Retirement {
            operation: pending.operation,
            admission: Admission::NotSubmitted,
        });
        let _ = self.try_release(provider);
        if pending.reply_allowed && !self.delivery_fenced {
            response
        } else {
            Dispatch::Fenced
        }
    }

    fn complete_quiesce(&mut self, correlation: Correlation, result: QuiesceResult) -> Dispatch {
        let Some(Pending::Quiesce {
            correlation: expected,
        }) = self.pending
        else {
            return self.correlation_fault();
        };
        if correlation != expected {
            return self.correlation_fault();
        }
        if result == QuiesceResult::Uncertain {
            return self.correlation_fault();
        }
        self.pending = None;
        if self.delivery_fenced {
            return self.correlation_fault();
        }
        match result {
            QuiesceResult::Quiesced => {
                self.state = SessionState::Disconnected;
                self.service_epoch = None;
                self.mta = None;
                positive()
            }
            QuiesceResult::Busy => {
                self.state = SessionState::Ready;
                error_dispatch(ErrorCode::CommandBusy)
            }
            QuiesceResult::Uncertain => {
                self.state = SessionState::Recovery;
                Dispatch::Fenced
            }
        }
    }

    fn complete_release(&mut self, correlation: Correlation) -> Dispatch {
        let Some(retirement) = self.retirement else {
            return self.correlation_fault();
        };
        if correlation != retirement.operation.correlation()
            || retirement.admission == Admission::NotSubmitted
        {
            return self.correlation_fault();
        }
        self.retirement = None;
        Dispatch::Ignored
    }

    /// Drives the apply deadline and retries bounded reconciliation/release
    /// admissions. A local deadline never fabricates a negative wire result.
    pub fn service<P: ProviderPort>(&mut self, now_us: u64, provider: &mut P) -> ServiceResult {
        let mut new_fault = self.try_release(provider);
        let Some(Pending::Write(mut pending)) = self.pending else {
            return ServiceResult {
                dispatch: if self.delivery_fenced {
                    Dispatch::Fenced
                } else {
                    Dispatch::Ignored
                },
                new_fault,
            };
        };
        if !pending.resolving && now_us >= pending.expires_at_us {
            pending.resolving = true;
            self.state = SessionState::Resolving;
        }
        if (pending.resolving || self.delivery_fenced)
            && pending.resolve_admission != Admission::Confirmed
        {
            match provider.try_submit(ProviderRequest::ResolveOrCancel {
                operation: pending.operation,
            }) {
                Ok(()) => pending.resolve_admission = Admission::Confirmed,
                Err(SubmitError::Busy) => {}
                Err(SubmitError::Unavailable) => {
                    new_fault = true;
                    pending.resolve_admission = Admission::Uncertain;
                    self.state = SessionState::Recovery;
                    self.delivery_fenced = true;
                    pending.reply_allowed = false;
                }
            }
        }
        self.pending = Some(Pending::Write(pending));
        ServiceResult {
            dispatch: if self.delivery_fenced {
                Dispatch::Fenced
            } else {
                Dispatch::Deferred
            },
            new_fault,
        }
    }

    /// The generated wait/recovery leaves supply the phase for the legacy
    /// standalone API; selected next-leaf guards inspect protocol facts.
    pub fn service_in<P: ProviderPort>(
        &mut self,
        phase: SessionState,
        now_us: u64,
        provider: &mut P,
    ) -> ServiceResult {
        self.state = phase;
        self.service(now_us, provider)
    }

    /// Invalidates wire delivery while preserving owner work and retirement.
    /// Matching delayed completions reconcile silently; a new CONNECT is
    /// blocked until pending work and retained release are drained.
    pub fn fence<P: ProviderPort>(&mut self, provider: &mut P) -> Dispatch {
        self.mark_fenced();
        let _ = self.service(0, provider);
        Dispatch::Fenced
    }

    /// Transport loss uses the same durable fence as lifecycle/protocol faults.
    pub fn transport_lost<P: ProviderPort>(&mut self, provider: &mut P) -> Dispatch {
        self.fence(provider)
    }

    fn mark_fenced(&mut self) {
        self.delivery_fenced = true;
        self.state = SessionState::Recovery;
        if let Some(Pending::Write(mut pending)) = self.pending {
            pending.reply_allowed = false;
            pending.resolving = true;
            self.pending = Some(Pending::Write(pending));
            self.state = SessionState::Resolving;
        }
    }

    // Reports only a failure observed in this attempt, not the durable fence.
    fn try_release<P: ProviderPort>(&mut self, provider: &mut P) -> bool {
        let Some(mut retirement) = self.retirement else {
            return false;
        };
        if retirement.admission == Admission::Confirmed {
            return false;
        }
        let admission = provider.try_submit(ProviderRequest::ReleaseOutcome {
            operation: retirement.operation,
        });
        match admission {
            Ok(()) => retirement.admission = Admission::Confirmed,
            Err(SubmitError::Busy) => {}
            Err(SubmitError::Unavailable) => {
                retirement.admission = Admission::Uncertain;
                self.mark_fenced();
            }
        }
        self.retirement = Some(retirement);
        admission == Err(SubmitError::Unavailable)
    }

    fn next_correlation(&mut self) -> Option<Correlation> {
        if self.next_sequence == u64::MAX {
            self.mark_fenced();
            return None;
        }
        Some(Correlation {
            session_generation: self.generation,
            sequence: self.next_sequence,
        })
    }

    fn consume_sequence(&mut self) {
        self.next_sequence += 1;
    }

    fn correlation_fault(&mut self) -> Dispatch {
        self.mark_fenced();
        Dispatch::Fenced
    }
}

fn exact_len(bytes: &[u8], expected: usize) -> Result<(), ErrorCode> {
    if bytes.len() == expected {
        Ok(())
    } else {
        Err(ErrorCode::CommandSyntax)
    }
}

fn positive() -> Dispatch {
    Dispatch::Respond(packet(&[0xFF]))
}

fn error_dispatch(error: ErrorCode) -> Dispatch {
    Dispatch::Respond(error_packet(error))
}

fn error_packet(error: ErrorCode) -> Packet {
    packet(&[0xFE, error as u8])
}

fn packet(bytes: &[u8]) -> Packet {
    match Packet::try_from_slice(bytes) {
        Ok(packet) => packet,
        Err(_) => unreachable!(),
    }
}

fn packet_parts(bytes: [u8; 8], len: u8) -> Packet {
    match Packet::from_parts(bytes, len) {
        Ok(packet) => packet,
        Err(_) => unreachable!(),
    }
}
