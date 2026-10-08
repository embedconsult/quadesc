#![no_std]
pub mod calibration_record;
#[cfg(feature = "host-supervision-trace")]
extern crate std;
use bloxide_calibration::{
    ApplyField, ApplyRequest, CommitGate, CommitRecord, EncodedValue, GateError, OperationKey,
    Owner, OwnerMode, OwnerOutcome, RejectReason,
};
use bloxide_persistence::{
    BackendCommand, Completion, CompletionError, OperationKey as PersistenceKey, Outcome,
    PersistenceService, Resolve, SaveResponse, Schema, SchemaError,
};
use bloxide_persistence_calibration::{capture_owner, Broker, CanonicalCalibration};
use core::cell::RefCell;
use critical_section::Mutex;
use led_messages::{
    LedConfig, LedMsg, LedResult, LedView, ProfileReply, SaveReceipt, SaveStatus, SavedView,
    XcpProviderReply, DUTY_ID, PERIOD_ID,
};
use xcp_core::ProviderRequest;
use xcp_messages::{
    ProviderCompletion, QuiesceResult, ReadData, ReadResult, RejectionReason as XcpReject,
    SynchronizeResult, WriteResult,
};
use xcp_profile_p::{
    Admission as ProfileAdmission, Domain, ProfileP, Reply, ScalarError, SAVE_DEADLINE_US,
};

/// Reviewed four-byte LED payload used by the selected restricted P profile.
#[derive(Clone, Copy)]
pub struct LedSchema;
impl Schema for LedSchema {
    fn id(&self) -> u16 {
        1
    }
    fn payload_len(&self) -> u16 {
        4
    }
    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        LedConfig::defaults().encode(out);
        Ok(())
    }
    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError> {
        if bytes.len() != 4 {
            return Err(SchemaError::InvalidValue);
        }
        LedConfig::new(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            u16::from_le_bytes([bytes[2], bytes[3]]),
        )
        .map(|_| ())
        .map_err(|_| SchemaError::InvalidValue)
    }
}

struct ProfileState {
    profile: Option<ProfileP>,
    service: PersistenceService<LedSchema>,
    broker: Broker<4>,
    uart_key: Option<PersistenceKey>,
    uart_outcome: Option<Outcome>,
    uart_wire_fenced: bool,
}

/// Fixed profile/service custody. Backend commands are taken and completed by
/// the application pump outside the generated action; it has no Owner access.
pub struct ProfileSlot {
    state: Mutex<RefCell<ProfileState>>,
}
impl ProfileSlot {
    fn capture_uart_terminal(state: &mut ProfileState) {
        let Some(key) = state.uart_key else {
            return;
        };
        let Resolve::Terminal(outcome) = state.service.resolve(key) else {
            return;
        };
        if !state.service.ownership_settled() {
            return;
        }
        state.uart_outcome = Some(outcome);
        if state.profile.is_none() && state.broker.result(1).is_none() {
            if state.broker.record_terminal(1, outcome).is_err() {
                state.uart_wire_fenced = true;
            }
        }
    }
    pub fn new(
        profile: ProfileP,
        service: PersistenceService<LedSchema>,
        broker: Broker<4>,
    ) -> Self {
        Self {
            state: Mutex::new(RefCell::new(ProfileState {
                profile: Some(profile),
                service,
                broker,
                uart_key: None,
                uart_outcome: None,
                uart_wire_fenced: false,
            })),
        }
    }
    /// Target UART composition without a selected XCP identity. The same
    /// service/Broker and one generated Owner remain authoritative.
    pub fn uart_only(service: PersistenceService<LedSchema>, broker: Broker<4>) -> Self {
        Self {
            state: Mutex::new(RefCell::new(ProfileState {
                profile: None,
                service,
                broker,
                uart_key: None,
                uart_outcome: None,
                uart_wire_fenced: false,
            })),
        }
    }
    pub fn take_command(&self, now_us: u64) -> Result<Option<BackendCommand>, CompletionError> {
        critical_section::with(|cs| {
            self.state
                .borrow(cs)
                .borrow_mut()
                .service
                .take_command(now_us)
        })
    }
    pub fn advance_time(&self, now_us: u64) {
        critical_section::with(|cs| {
            self.state
                .borrow(cs)
                .borrow_mut()
                .service
                .advance_time(now_us)
        });
    }
    pub fn take_quiesce_request(&self, now_us: u64) -> Option<bloxide_persistence::QuiesceRequest> {
        critical_section::with(|cs| {
            self.state
                .borrow(cs)
                .borrow_mut()
                .service
                .take_quiesce_request(now_us)
        })
    }
    pub fn complete_quiesce(
        &self,
        request: bloxide_persistence::QuiesceRequest,
        idle_and_drained: bool,
    ) -> Result<(), CompletionError> {
        critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            state.service.complete_quiesce(request, idle_and_drained)?;
            Self::capture_uart_terminal(&mut state);
            Ok(())
        })
    }
    pub fn complete(&self, completion: Completion) -> Result<(), CompletionError> {
        critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            let ProfileState {
                profile,
                service,
                ..
            } = &mut *state;
            if let Some(profile) = profile.as_mut() {
                profile.complete_backend(service, completion)?;
            } else {
                service.complete(completion)?;
            }
            Self::capture_uart_terminal(&mut state);
            let ProfileState { profile, service, broker, .. } = &mut *state;
            if let Some(profile) = profile.as_mut() {
                profile.observe_all(service, broker);
            }
            Ok(())
        })
    }
    /// Retire the exact UART result only after its terminal response was
    /// flushed. Failed writes keep custody for reconciliation.
    pub fn release_uart(&self, key: PersistenceKey) -> bool {
        critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            if state.profile.is_some()
                || state.uart_wire_fenced
                || state.uart_key != Some(key)
                || state.uart_outcome.map(Outcome::key) != Some(key)
                || !state.service.ownership_settled()
            {
                return false;
            }
            if state.service.release(key).is_err() {
                return false;
            }
            if state.broker.release(1).is_err() {
                return false;
            }
            state.uart_key = None;
            state.uart_outcome = None;
            true
        })
    }
    pub fn ownership_settled(&self) -> bool {
        critical_section::with(|cs| self.state.borrow(cs).borrow().service.ownership_settled())
    }
    pub fn service_mode(&self) -> bloxide_persistence::ServiceMode {
        critical_section::with(|cs| self.state.borrow(cs).borrow().service.mode())
    }
    pub fn fence_wire(&self) {
        critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            state.uart_wire_fenced = true;
            if let Some(profile) = state.profile.as_mut() {
                profile.fence_wire();
            }
        });
    }
    pub fn logical_reset(&self) {
        critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            if let Some(profile) = state.profile.as_mut() {
                profile.logical_reset();
            }
            if state.profile.is_none() {
                state.service.reset();
            }
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputSchedule {
    pub config: LedConfig,
    pub revision: u32,
    pub phase_epoch_us: u64,
    pub commanded_on: bool,
    pub service_epoch: u64,
    pub session_generation: u32,
    pub output_epoch: u64,
    pub sequence: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Starting,
    Running,
    Quiescing,
    Quiesced,
    Faulted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoDisposition {
    Installed(OutputSchedule),
    CancelledByLifecycle(OutputSchedule),
    Faulted(OutputSchedule),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultStage {
    Installation,
    Edge,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafeOff {
    Pending,
    Verified,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFault {
    pub schedule: OutputSchedule,
    pub stage: FaultStage,
    pub safe_off: SafeOff,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputStatus {
    pub admission: Admission,
    pub accepted_epoch: u64,
    pub requested_epoch: Option<u64>,
    pub queued: bool,
    pub queued_schedule: Option<OutputSchedule>,
    pub in_flight: bool,
    pub completions: u8,
    pub pending_completions: [Option<IoDisposition>; 2],
    pub last_drained: Option<IoDisposition>,
    pub fault: Option<OutputFault>,
    pub fault_acknowledged: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotState {
    Empty,
    Reserved,
    Ready(OutputSchedule),
}
struct State {
    slot: SlotState,
    admission: Admission,
    accepted_epoch: u64,
    requested_epoch: Option<u64>,
    in_flight: Option<OutputSchedule>,
    completion: [Option<IoDisposition>; 2],
    last_drained: Option<IoDisposition>,
    fault: Option<OutputFault>,
    fault_acknowledged: bool,
}
impl State {
    fn push_completion(&mut self, disposition: IoDisposition) -> bool {
        for entry in &mut self.completion {
            if entry.is_none() {
                *entry = Some(disposition);
                return true;
            }
        }
        false
    }
    fn completion_full(&self) -> bool {
        self.completion.iter().all(Option::is_some)
    }
    fn completion_empty(&self) -> bool {
        self.completion.iter().all(Option::is_none)
    }
}
pub struct ScheduleSlot {
    state: Mutex<RefCell<State>>,
}
pub struct ScheduleProducer {
    slot: &'static ScheduleSlot,
}
pub struct ScheduleConsumer {
    slot: &'static ScheduleSlot,
}
pub struct OutputStatusReader {
    slot: &'static ScheduleSlot,
}
pub struct Reservation {
    slot: &'static ScheduleSlot,
    armed: bool,
}
impl ScheduleSlot {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                slot: SlotState::Empty,
                admission: Admission::Starting,
                accepted_epoch: 1,
                requested_epoch: None,
                in_flight: None,
                completion: [None, None],
                last_drained: None,
                fault: None,
                fault_acknowledged: false,
            })),
        }
    }
    pub fn split(&'static mut self) -> (ScheduleProducer, ScheduleConsumer) {
        (
            ScheduleProducer { slot: self },
            ScheduleConsumer { slot: self },
        )
    }
}
impl ScheduleProducer {
    pub fn prime_initial(&mut self, config: LedConfig, phase_epoch_us: u64) {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            assert!(s.admission == Admission::Starting && s.slot == SlotState::Empty);
            s.slot = SlotState::Ready(OutputSchedule {
                config,
                revision: 0,
                phase_epoch_us,
                commanded_on: config.duty_permille() != 0,
                service_epoch: 1,
                session_generation: 0,
                output_epoch: s.accepted_epoch,
                sequence: 0,
            });
        });
    }
    pub fn status(&self) -> OutputStatus {
        status(self.slot)
    }
    pub fn take_completion(&mut self) -> Option<IoDisposition> {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            let first = s.completion[0].take();
            s.completion[0] = s.completion[1].take();
            if let Some(disposition) = first {
                s.last_drained = Some(disposition);
            }
            first
        })
    }
    pub fn request_quiesce(&mut self) {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if matches!(
                s.admission,
                Admission::Running | Admission::Starting | Admission::Faulted
            ) {
                s.admission = Admission::Quiescing;
            }
        });
    }
    /// Reconcile a fault after its queued work and completion records are drained.
    /// Unknown inactive-output readback cannot authorize a later epoch.
    pub fn acknowledge_fault(&mut self) -> bool {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if s.admission != Admission::Quiescing
                || s.slot != SlotState::Empty
                || s.in_flight.is_some()
                || !s.completion_empty()
                || !s
                    .fault
                    .is_some_and(|fault| fault.safe_off == SafeOff::Verified)
            {
                return false;
            }
            s.fault_acknowledged = true;
            true
        })
    }
    pub fn request_resume(&mut self, next_epoch: u64) -> Result<(), GateError> {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if s.admission != Admission::Quiesced || next_epoch <= s.accepted_epoch {
                return Err(GateError::Quiescing);
            }
            if s.requested_epoch
                .is_some_and(|pending| pending != next_epoch)
            {
                return Err(GateError::Busy);
            }
            s.requested_epoch = Some(next_epoch);
            Ok(())
        })
    }
}
impl ScheduleConsumer {
    /// A read-only fixed-state view for the diagnostic service.
    pub fn status_reader(&self) -> OutputStatusReader {
        OutputStatusReader { slot: self.slot }
    }
    /// Close admission before attempting a best-effort inactive output. A
    /// failed installation owns a reserved completion; a later edge failure
    /// retains its exact command in the fault latch after installation.
    pub fn report_fault(&mut self, schedule: OutputSchedule, stage: FaultStage) {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if s.fault.is_some() {
                return;
            }
            if stage == FaultStage::Installation {
                assert_eq!(s.in_flight, Some(schedule));
                assert!(s.push_completion(IoDisposition::Faulted(schedule)));
                s.in_flight = None;
            }
            s.admission = Admission::Faulted;
            s.fault_acknowledged = false;
            s.fault = Some(OutputFault {
                schedule,
                stage,
                safe_off: SafeOff::Pending,
            });
        });
    }
    pub fn report_safe_off(&mut self, verified: bool) {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if let Some(fault) = &mut s.fault {
                fault.safe_off = if verified {
                    SafeOff::Verified
                } else {
                    SafeOff::Unknown
                };
            }
        });
    }
    pub fn start(&mut self) {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if s.admission == Admission::Starting {
                s.admission = Admission::Running;
            }
        });
    }
    pub fn try_take(&mut self) -> Option<OutputSchedule> {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if s.completion_full() || s.in_flight.is_some() {
                return None;
            }
            match s.slot {
                SlotState::Ready(schedule) if s.admission == Admission::Running => {
                    s.slot = SlotState::Empty;
                    s.in_flight = Some(schedule);
                    Some(schedule)
                }
                _ => None,
            }
        })
    }
    pub fn report_completed(&mut self, schedule: OutputSchedule) {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            assert_eq!(s.in_flight, Some(schedule));
            assert!(s.push_completion(IoDisposition::Installed(schedule)));
            s.in_flight = None;
        });
    }
    /// Retire a queued command before acknowledging quiescence. Caller supplies an
    /// independently verified inactive-output result, never inferred from this slot.
    pub fn begin_quiesce(&mut self, physical_off_confirmed: bool) -> bool {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if !matches!(s.admission, Admission::Quiescing | Admission::Faulted) {
                return false;
            }
            if let SlotState::Ready(schedule) = s.slot {
                if !s.push_completion(IoDisposition::CancelledByLifecycle(schedule)) {
                    return false;
                }
                s.slot = SlotState::Empty;
            }
            if s.admission == Admission::Faulted {
                return false;
            }
            if s.slot != SlotState::Empty
                || s.in_flight.is_some()
                || !s.completion_empty()
                || !physical_off_confirmed
                || (s.fault.is_some() && !s.fault_acknowledged)
                || s.fault
                    .is_some_and(|fault| fault.safe_off != SafeOff::Verified)
            {
                return false;
            }
            s.admission = Admission::Quiesced;
            true
        })
    }
    /// An explicit service acknowledgement opens the new output epoch.
    pub fn accept_resume(&mut self) -> bool {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            if s.admission != Admission::Quiesced
                || s.slot != SlotState::Empty
                || s.in_flight.is_some()
                || !s.completion_empty()
            {
                return false;
            }
            let Some(epoch) = s.requested_epoch.take() else {
                return false;
            };
            s.accepted_epoch = epoch;
            s.admission = Admission::Running;
            s.fault = None;
            s.fault_acknowledged = false;
            true
        })
    }
}
impl OutputStatusReader {
    pub fn status(&self) -> OutputStatus {
        status(self.slot)
    }
}
fn status(slot: &ScheduleSlot) -> OutputStatus {
    critical_section::with(|cs| {
        let s = slot.state.borrow(cs).borrow();
        OutputStatus {
            admission: s.admission,
            accepted_epoch: s.accepted_epoch,
            requested_epoch: s.requested_epoch,
            queued: matches!(s.slot, SlotState::Ready(_)),
            queued_schedule: match s.slot {
                SlotState::Ready(schedule) => Some(schedule),
                _ => None,
            },
            in_flight: s.in_flight.is_some(),
            completions: s.completion.iter().filter(|entry| entry.is_some()).count() as u8,
            pending_completions: s.completion,
            last_drained: s.last_drained,
            fault: s.fault,
            fault_acknowledged: s.fault_acknowledged,
        }
    })
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.armed {
            critical_section::with(|cs| {
                let mut s = self.slot.state.borrow(cs).borrow_mut();
                if s.slot == SlotState::Reserved {
                    s.slot = SlotState::Empty;
                }
            });
        }
    }
}
impl CommitGate<LedConfig> for ScheduleProducer {
    type Permit = Reservation;
    fn try_reserve(&mut self, _: &LedConfig) -> Result<Self::Permit, GateError> {
        critical_section::with(|cs| {
            let mut s = self.slot.state.borrow(cs).borrow_mut();
            match s.admission {
                Admission::Running => (),
                Admission::Starting => return Err(GateError::NotReady),
                Admission::Quiescing | Admission::Quiesced => return Err(GateError::Quiescing),
                Admission::Faulted => return Err(GateError::Faulted),
            };
            if s.slot != SlotState::Empty {
                return Err(GateError::Busy);
            }
            s.slot = SlotState::Reserved;
            Ok(Reservation {
                slot: self.slot,
                armed: true,
            })
        })
    }
    fn publish(&mut self, mut permit: Self::Permit, record: CommitRecord<LedConfig>) {
        critical_section::with(|cs| {
            let mut s = permit.slot.state.borrow(cs).borrow_mut();
            assert!(s.slot == SlotState::Reserved);
            s.slot = SlotState::Ready(OutputSchedule {
                config: record.active_values,
                revision: record.active_revision.get(),
                phase_epoch_us: record.applied_at_us,
                commanded_on: record.active_values.duty_permille() != 0,
                service_epoch: record.key.service_epoch,
                session_generation: record.key.session_generation,
                output_epoch: s.accepted_epoch,
                sequence: record.key.sequence,
            });
        });
        permit.armed = false;
    }
}

pub struct LedController {
    calibration_store: Option<fn(LedConfig, u32) -> bool>,
    owner: Owner<LedConfig>,
    profile: Option<&'static ProfileSlot>,
    next_profile_sequence: u64,
    output: ScheduleProducer,
    clock_us: fn() -> u64,
    phase_epoch_us: u64,
    commanded_on: bool,
    last_result: Option<LedResult>,
    reply: Option<ReplyWriter>,
    completed_count: u64,
    last_disposition: Option<IoDisposition>,
}
impl LedController {
    pub fn new(output: ScheduleProducer, clock_us: fn() -> u64) -> Self {
        Self::new_with_config(output, clock_us, LedConfig::defaults())
    }
    /// Boot construction from a separately classified portable recovery result.
    /// The one Owner and its initial output schedule receive the same values.
    pub fn new_with_config(
        mut output: ScheduleProducer,
        clock_us: fn() -> u64,
        initial: LedConfig,
    ) -> Self {
        let phase_epoch_us = clock_us();
        output.prime_initial(initial, phase_epoch_us);
        Self {
            owner: Owner::new(initial, 1, 1),
            calibration_store: None,
            profile: None,
            next_profile_sequence: 1,
            output,
            clock_us,
            phase_epoch_us,
            commanded_on: initial.duty_permille() != 0,
            last_result: None,
            reply: None,
            completed_count: 0,
            last_disposition: None,
        }
    }
    pub fn view(&self) -> LedView {
        let c = self.owner.active();
        LedView {
            period_ms: c.period_ms(),
            duty_permille: c.duty_permille(),
            revision: self.owner.active_revision().get(),
            phase_epoch_us: self.phase_epoch_us,
            commanded_on: self.commanded_on,
        }
    }
    pub fn last_result(&self) -> Option<LedResult> {
        self.last_result
    }
    pub fn output_status(&self) -> OutputStatus {
        self.output.status()
    }
    pub fn request_quiesce(&mut self) {
        self.output.request_quiesce();
    }
    pub fn acknowledge_fault(&mut self) -> bool {
        self.output.acknowledge_fault()
    }
    pub fn request_resume(&mut self, next_epoch: u64) -> Result<(), GateError> {
        self.output.request_resume(next_epoch)
    }
    pub fn attach_calibration_store(&mut self, store: fn(LedConfig, u32) -> bool) {
        assert!(self.calibration_store.is_none());
        self.calibration_store = Some(store);
    }
    pub fn attach_reply(&mut self, reply: ReplyWriter) {
        assert!(self.reply.is_none());
        self.reply = Some(reply);
    }
    /// Called once before Start for an explicitly selected P host composition.
    pub fn attach_profile(&mut self, profile: &'static ProfileSlot) {
        assert!(self.profile.is_none());
        self.profile = Some(profile);
    }
    fn record_result(&mut self, result: LedResult) -> LedResult {
        self.last_result = Some(result);
        if let Some(reply) = &mut self.reply {
            reply.publish(result);
        }
        result
    }
    pub fn take_completion(&mut self) -> Option<IoDisposition> {
        self.output.take_completion()
    }
    pub fn completion_summary(&self) -> (u64, Option<IoDisposition>) {
        (self.completed_count, self.last_disposition)
    }
    fn drain_output(&mut self) {
        for _ in 0..2 {
            let Some(disposition) = self.output.take_completion() else {
                break;
            };
            self.completed_count = self.completed_count.saturating_add(1);
            self.last_disposition = Some(disposition);
        }
    }
    pub fn dispatch(&mut self, msg: LedMsg) -> LedResult {
        match msg {
            LedMsg::Read => self.read(),
            LedMsg::ReadSaved => self.read_saved(),
            LedMsg::Apply(req) => self.apply(req),
            LedMsg::Release(key) => self.release(key),
            LedMsg::ProfileP(req) => self.profile_p(req),
            LedMsg::ProfileReset => self.profile_reset(),
            LedMsg::ProfileLost => self.profile_lost(),
            LedMsg::Save(now_us) => self.save(now_us),
            LedMsg::ResolveSave(key) => self.resolve_save(key),
            LedMsg::XcpProvider(request) => self.xcp_provider(request),
            LedMsg::OutputCompleted(_) => {
                self.drain_output();
                LedResult::View(self.view())
            }
            LedMsg::OutputFaulted(_) => {
                self.request_quiesce();
                LedResult::View(self.view())
            }
        }
    }
    /// Adapter calls into this actor's sole Owner; no provider-side settings
    /// cache, revision counter, or output producer exists.
    pub fn xcp_provider(&mut self, request: ProviderRequest) -> LedResult {
        let completion = match request {
            ProviderRequest::StoreCalibration { correlation } => {
                let verified = correlation.session_generation == self.owner.session_generation()
                    && self.owner.mode() == OwnerMode::Running
                    && self.owner.retained().is_none()
                    && self.calibration_store.is_some_and(|store| store(self.owner.active(), self.owner.active_revision().get()));
                ProviderCompletion::CalibrationStored { correlation, verified }
            },
            ProviderRequest::Synchronize { session_generation } => {
                let result = if self.owner.retained().is_some() {
                    SynchronizeResult::Busy
                } else if !matches!(self.owner.mode(), OwnerMode::Running) {
                    SynchronizeResult::WrongLifecycle
                } else if session_generation == self.owner.session_generation() {
                    SynchronizeResult::Ready { service_epoch: self.owner.service_epoch() }
                } else {
                    match self.owner.begin_coordinated_reset()
                        .and_then(|_| self.owner.finish_coordinated_reset(session_generation))
                    {
                        Ok(()) => SynchronizeResult::Ready { service_epoch: self.owner.service_epoch() },
                        Err(_) => SynchronizeResult::WrongLifecycle,
                    }
                };
                ProviderCompletion::Synchronized { session_generation, result }
            }
            ProviderRequest::Read(read) => {
                let result = if !matches!(self.owner.mode(), OwnerMode::Running) {
                    ReadResult::WrongLifecycle
                } else {
                    let scalar = match read.descriptor_id {
                        PERIOD_ID => Some(self.owner.active().period_ms().to_le_bytes()),
                        DUTY_ID => Some(self.owner.active().duty_permille().to_le_bytes()),
                        _ => None,
                    };
                    match scalar {
                        None => ReadResult::AccessDenied,
                        Some(bytes) if read.length == 0
                            || read.offset.checked_add(u32::from(read.length)).is_none_or(|end| end > 2) =>
                        {
                            let _ = bytes;
                            ReadResult::OutOfRange
                        }
                        Some(bytes) => {
                            let mut data = [0u8; 7];
                            let start = read.offset as usize;
                            let len = read.length as usize;
                            data[..len].copy_from_slice(&bytes[start..start + len]);
                            ReadResult::Data(ReadData::new(data, read.length).expect("checked scalar length"))
                        }
                    }
                };
                ProviderCompletion::Read { correlation: read.correlation, result }
            }
            ProviderRequest::Apply(write) => {
                if write.length != 2 {
                    return self.record_result(LedResult::XcpProvider(XcpProviderReply::Fenced));
                }
                let key = OperationKey {
                    service_epoch: write.operation.service_epoch,
                    session_generation: write.operation.session_generation,
                    sequence: write.operation.sequence,
                };
                let result = match EncodedValue::try_from_slice(&write.encoded_value[..2]) {
                    Ok(encoded_value) => match self.apply_inner(ApplyRequest {
                        field: ApplyField {
                            key,
                            field_id: write.descriptor_id,
                            encoded_value,
                            expires_at_us: write.expires_at_us,
                        },
                        expected_revision: None,
                    }) {
                        LedResult::Outcome(outcome) => xcp_write_result(outcome),
                        _ => WriteResult::Uncertain,
                    },
                    Err(_) => WriteResult::Rejected { reason: XcpReject::BadEncoding },
                };
                ProviderCompletion::Write { correlation: write.operation.correlation(), result }
            }
            ProviderRequest::ResolveOrCancel { operation } => {
                let key = OperationKey {
                    service_epoch: operation.service_epoch,
                    session_generation: operation.session_generation,
                    sequence: operation.sequence,
                };
                ProviderCompletion::Write {
                    correlation: operation.correlation(),
                    result: xcp_write_result(self.owner.resolve_or_cancel(key)),
                }
            }
            ProviderRequest::Quiesce { correlation } => ProviderCompletion::Quiesced {
                correlation,
                result: if self.owner.retained().is_some()
                    || !matches!(self.owner.mode(), OwnerMode::Running)
                {
                    QuiesceResult::Busy
                } else {
                    QuiesceResult::Quiesced
                },
            },
            ProviderRequest::ReleaseOutcome { operation } => {
                let key = OperationKey {
                    service_epoch: operation.service_epoch,
                    session_generation: operation.session_generation,
                    sequence: operation.sequence,
                };
                if self.owner.release(key).is_err() {
                    return self.record_result(LedResult::XcpProvider(XcpProviderReply::Fenced));
                }
                ProviderCompletion::Released { correlation: operation.correlation() }
            }
        };
        self.record_result(LedResult::XcpProvider(XcpProviderReply::Completion(completion)))
    }
    pub fn apply(&mut self, request: ApplyRequest) -> LedResult {
        let result = self.apply_inner(request);
        self.record_result(result)
    }
    fn apply_inner(&mut self, request: ApplyRequest) -> LedResult {
        self.drain_output();
        let outcome = self
            .owner
            .apply(request, self.clock_us, candidate, &mut self.output);
        if let OwnerOutcome::Applied {
            changed: true,
            applied_at_us,
            active_values,
            ..
        } = outcome
        {
            self.phase_epoch_us = applied_at_us;
            self.commanded_on = active_values.duty_permille() != 0;
        }
        let result = LedResult::Outcome(outcome);
        result
    }
    pub fn read(&mut self) -> LedResult {
        self.drain_output();
        let result = LedResult::View(self.view());
        self.record_result(result)
    }
    pub fn read_saved(&mut self) -> LedResult {
        let saved = match self.profile {
            None => SavedView::Unavailable,
            Some(slot) => critical_section::with(|cs| {
                let state = slot.state.borrow(cs).borrow();
                match state.service.read_durable() {
                    None => SavedView::NoRecord,
                    Some(record) => {
                        let bytes = record.snapshot.bytes();
                        SavedView::Record {
                            sequence: record.sequence,
                            source_epoch: record.snapshot.source_epoch(),
                            source_revision: record.snapshot.source_revision(),
                            period_ms: u16::from_le_bytes([bytes[0], bytes[1]]),
                            duty_permille: u16::from_le_bytes([bytes[2], bytes[3]]),
                        }
                    }
                }
            }),
        };
        self.record_result(LedResult::Saved(saved))
    }
    pub fn release(&mut self, key: OperationKey) -> LedResult {
        let result = LedResult::Released(self.owner.release(key).is_ok());
        self.record_result(result)
    }
    pub fn profile_p(&mut self, request: xcp_messages::PacketRequest) -> LedResult {
        let Some(slot) = self.profile else {
            return self.record_result(LedResult::ProfileP(ProfileReply::Fenced));
        };
        let clock = self.clock_us;
        let output = self.output.status();
        let selected_admission = ProfileAdmission {
            disarmed: output.fault.is_none() && output.admission == Admission::Running,
            maintenance: true,
            schema_known: true,
        };
        let reply = critical_section::with(|cs| {
            let mut state = slot.state.borrow(cs).borrow_mut();
            let ProfileState {
                profile,
                service,
                broker,
                ..
            } = &mut *state;
            let Some(profile) = profile.as_mut() else {
                return Reply::Fenced;
            };
            profile.handle(
                &request.packet,
                self,
                &LedSchema,
                service,
                broker,
                selected_admission,
                request.now_us,
                clock,
            )
        });
        let reply = match reply {
            Reply::Packet(packet) => ProfileReply::Packet(packet),
            Reply::Ignored => ProfileReply::Ignored,
            Reply::Fenced => ProfileReply::Fenced,
        };
        self.record_result(LedResult::ProfileP(reply))
    }
    pub fn profile_reset(&mut self) -> LedResult {
        if let Some(slot) = self.profile {
            slot.logical_reset();
        }
        self.record_result(LedResult::ProfileP(ProfileReply::Fenced))
    }
    pub fn profile_lost(&mut self) -> LedResult {
        if let Some(slot) = self.profile {
            slot.fence_wire();
        }
        self.record_result(LedResult::ProfileP(ProfileReply::Fenced))
    }
    pub fn save(&mut self, now_us: u64) -> LedResult {
        let Some(slot) = self.profile else {
            return self.record_result(LedResult::Save(SaveReceipt::Unavailable));
        };
        let output = self.output.status();
        if output.fault.is_some()
            || output.admission != Admission::Running
            || self.owner.mode() != OwnerMode::Running
        {
            return self.record_result(LedResult::Save(SaveReceipt::Rejected));
        }
        let receipt = critical_section::with(|cs| {
            let mut state = slot.state.borrow(cs).borrow_mut();
            let ProfileState {
                profile,
                service,
                broker,
                uart_key,
                uart_outcome,
                uart_wire_fenced,
            } = &mut *state;
            if *uart_wire_fenced || service.health().write_locked || !service.ownership_settled() {
                return SaveReceipt::Rejected;
            }
            let Some(deadline) = now_us.checked_add(SAVE_DEADLINE_US) else {
                return SaveReceipt::Rejected;
            };
            if (self.clock_us)() >= deadline {
                return SaveReceipt::Rejected;
            }
            let Ok(broker_key) = broker.reserve(1) else {
                return SaveReceipt::Rejected;
            };
            let key = PersistenceKey {
                service_epoch: broker_key.service_epoch,
                session_generation: broker_key.session_generation,
                sequence: broker_key.sequence,
            };
            let Ok(capture) = capture_owner(&self.owner, &LedSchema, broker_key, None) else {
                if let Some(profile) = profile.as_mut() {
                    profile.fence_wire();
                }
                *uart_wire_fenced = true;
                return SaveReceipt::Fenced;
            };
            if let Some(profile) = profile.as_mut() {
                if !profile.retain_client_capture(1, capture) {
                    profile.fence_wire();
                    *uart_wire_fenced = true;
                    return SaveReceipt::Fenced;
                }
            }
            let response = service.save(key, capture, deadline, self.clock_us);
            if let Some(profile) = profile.as_mut() {
                if !profile.finish_client_save(1, response, service, broker) {
                    *uart_wire_fenced = true;
                    return SaveReceipt::Fenced;
                }
            } else if let SaveResponse::Terminal(outcome) = response {
                if broker.record_terminal(1, outcome).is_err() {
                    *uart_wire_fenced = true;
                    return SaveReceipt::Fenced;
                }
                if !matches!(outcome, Outcome::Durable { .. }) {
                    if service.release(key).is_err() || broker.release(1).is_err() {
                        *uart_wire_fenced = true;
                        return SaveReceipt::Fenced;
                    }
                }
            }
            match response {
                SaveResponse::Accepted => {
                    *uart_key = Some(key);
                    *uart_outcome = None;
                    SaveReceipt::Accepted(key)
                }
                SaveResponse::Terminal(outcome @ Outcome::Durable { .. }) => {
                    *uart_key = Some(key);
                    *uart_outcome = Some(outcome);
                    SaveReceipt::Durable(key)
                }
                SaveResponse::Terminal(Outcome::Rejected { .. }) => SaveReceipt::Rejected,
                _ => SaveReceipt::Fenced,
            }
        });
        self.record_result(LedResult::Save(receipt))
    }

    pub fn resolve_save(&mut self, key: PersistenceKey) -> LedResult {
        let status = match self.profile {
            None => SaveStatus::Unavailable,
            Some(slot) => critical_section::with(|cs| {
                let state = slot.state.borrow(cs).borrow();
                if state.uart_wire_fenced {
                    return SaveStatus::Indeterminate;
                }
                if state.uart_key != Some(key) {
                    return SaveStatus::Stale;
                }
                let outcome = state
                    .uart_outcome
                    .or_else(|| match state.service.resolve(key) {
                        Resolve::Terminal(outcome) if state.service.ownership_settled() => {
                            Some(outcome)
                        }
                        _ => None,
                    });
                match outcome {
                    Some(Outcome::Durable { record, .. }) => SaveStatus::Durable {
                        sequence: record.sequence,
                        saved_revision: record.snapshot.source_revision(),
                    },
                    Some(Outcome::Indeterminate { .. }) => SaveStatus::Indeterminate,
                    Some(Outcome::Failed { .. }) => SaveStatus::Failed,
                    Some(Outcome::Rejected { .. } | Outcome::Cancelled { .. }) => {
                        SaveStatus::Rejected
                    }
                    None => SaveStatus::Pending,
                }
            }),
        };
        self.record_result(LedResult::SaveStatus(status))
    }
}
impl Domain<LedConfig> for LedController {
    fn owner(&self) -> &Owner<LedConfig> {
        &self.owner
    }
    fn read_scalar(&self, address: u32) -> Option<[u8; 2]> {
        match address {
            0x1000 => Some(self.owner.active().period_ms().to_le_bytes()),
            0x1002 => Some(self.owner.active().duty_permille().to_le_bytes()),
            _ => None,
        }
    }
    fn write_scalar(
        &mut self,
        address: u32,
        value: [u8; 2],
        now_us: u64,
    ) -> Result<(), ScalarError> {
        let field_id = match address {
            0x1000 => PERIOD_ID,
            0x1002 => DUTY_ID,
            _ => return Err(ScalarError::Bounds),
        };
        let key = OperationKey {
            service_epoch: self.owner.service_epoch(),
            session_generation: self.owner.session_generation(),
            sequence: self.next_profile_sequence,
        };
        self.next_profile_sequence = self
            .next_profile_sequence
            .checked_add(1)
            .ok_or(ScalarError::Policy)?;
        let result = self.apply_inner(ApplyRequest {
            field: ApplyField {
                key,
                field_id,
                encoded_value: EncodedValue::try_from_slice(&value)
                    .map_err(|_| ScalarError::Bounds)?,
                expires_at_us: now_us.saturating_add(6_000_000),
            },
            expected_revision: None,
        });
        if !matches!(result, LedResult::Outcome(ref outcome) if outcome.key() == key && outcome.is_terminal_retained())
        {
            return Err(ScalarError::Busy);
        }
        if self.owner.release(key).is_err() {
            return Err(ScalarError::Policy);
        }
        match result {
            LedResult::Outcome(OwnerOutcome::Applied { .. }) => Ok(()),
            LedResult::Outcome(OwnerOutcome::Rejected {
                reason: RejectReason::Bounds | RejectReason::BadEncoding,
                ..
            }) => Err(ScalarError::Bounds),
            _ => Err(ScalarError::Policy),
        }
    }
}

#[cfg(test)]
mod uart_only_tests {
    extern crate std;
    use super::*;
    use bloxide_persistence::{
        classify_slot, recover, BackendConfig, BackendTiming, Classification, Geometry, ReadIssue,
        Slot, SlotRead, WearLease,
    };
    use bloxide_persistence_sim::Simulator;
    use std::{boxed::Box, vec::Vec};

    fn clock() -> u64 {
        100
    }
    fn controller(
        initial: LedConfig,
        slot: &'static ProfileSlot,
    ) -> (LedController, ScheduleConsumer) {
        let storage = Box::leak(Box::new(ScheduleSlot::new()));
        let (producer, mut consumer) = storage.split();
        let mut owner = LedController::new_with_config(producer, clock, initial);
        owner.attach_profile(slot);
        consumer.start();
        let first = consumer.try_take().unwrap();
        consumer.report_completed(first);
        (owner, consumer)
    }

    #[test]
    fn unknown_wear_and_unreadable_boot_deny_uart_mutation() {
        let geometry = Geometry::new(2048, 256).unwrap();
        let recovery = recover(
            Classification::Unreadable(ReadIssue::Io),
            Classification::Unreadable(ReadIssue::Io),
            &LedSchema,
        )
        .unwrap();
        let service = PersistenceService::new(
            geometry,
            LedSchema,
            recovery,
            false,
            1,
            1,
            1,
            0,
            BackendConfig {
                lease: WearLease(0),
                timing: BackendTiming {
                    read: 0,
                    permit: 0,
                    erase: 0,
                    program: 0,
                    quiesce: 0,
                },
            },
        );
        let slot = Box::leak(Box::new(ProfileSlot::uart_only(
            service,
            Broker::<4>::new(1, 1),
        )));
        assert_eq!(
            slot.service_mode(),
            bloxide_persistence::ServiceMode::ReadOnlyFault
        );
        let (mut owner, mut consumer) = controller(LedConfig::defaults(), slot);
        let applied = owner.apply(ApplyRequest {
            field: ApplyField {
                key: OperationKey {
                    service_epoch: 1,
                    session_generation: 1,
                    sequence: 1,
                },
                field_id: PERIOD_ID,
                encoded_value: EncodedValue::try_from_slice(&1600u16.to_le_bytes()).unwrap(),
                expires_at_us: 1000,
            },
            expected_revision: None,
        });
        assert!(matches!(
            applied,
            LedResult::Outcome(OwnerOutcome::Applied { changed: true, .. })
        ));
        let schedule = consumer.try_take().unwrap();
        consumer.report_completed(schedule);
        owner.release(OperationKey {
            service_epoch: 1,
            session_generation: 1,
            sequence: 1,
        });
        assert_eq!(owner.save(100), LedResult::Save(SaveReceipt::Rejected));
        assert_eq!(owner.read_saved(), LedResult::Saved(SavedView::NoRecord));
        slot.logical_reset();
        assert_eq!(owner.save(100), LedResult::Save(SaveReceipt::Rejected));
        assert!(slot.take_command(100).unwrap().is_none());
    }

    #[test]
    fn uart_only_service_resolves_durable_then_reboots_from_classified_media() {
        let geometry = Geometry::new(2048, 256).unwrap();
        let mut media = Simulator::new(geometry, 2, 44);
        let backend = media.open_session().unwrap();
        let recovery = recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap();
        let service =
            PersistenceService::new(geometry, LedSchema, recovery, false, 1, 1, 1, 0, backend);
        let slot = Box::leak(Box::new(ProfileSlot::uart_only(
            service,
            Broker::<4>::new(1, 1),
        )));
        let (mut owner, mut consumer) = controller(LedConfig::defaults(), slot);
        let applied = owner.apply(ApplyRequest {
            field: ApplyField {
                key: OperationKey {
                    service_epoch: 1,
                    session_generation: 1,
                    sequence: 1,
                },
                field_id: PERIOD_ID,
                encoded_value: EncodedValue::try_from_slice(&1600u16.to_le_bytes()).unwrap(),
                expires_at_us: 1000,
            },
            expected_revision: None,
        });
        assert!(matches!(
            applied,
            LedResult::Outcome(OwnerOutcome::Applied { changed: true, .. })
        ));
        let schedule = consumer.try_take().unwrap();
        consumer.report_completed(schedule);
        owner.release(OperationKey {
            service_epoch: 1,
            session_generation: 1,
            sequence: 1,
        });
        let key = match owner.save(100) {
            LedResult::Save(SaveReceipt::Accepted(key)) => key,
            other => panic!("expected accepted receipt: {other:?}"),
        };
        assert_eq!(
            owner.resolve_save(key),
            LedResult::SaveStatus(SaveStatus::Pending)
        );
        let mut kinds = Vec::new();
        for _ in 0..128 {
            if slot.ownership_settled() {
                break;
            }
            let command = slot.take_command(100).unwrap().unwrap();
            kinds.push(command.header.kind);
            slot.complete(media.execute(command)).unwrap();
        }
        assert!(slot.ownership_settled());
        assert!(kinds
            .iter()
            .any(|kind| matches!(kind, bloxide_persistence::CommandKind::Erase { .. })));
        assert_eq!(
            owner.resolve_save(key),
            LedResult::SaveStatus(SaveStatus::Durable {
                sequence: 1,
                saved_revision: 1
            })
        );
        assert!(slot.release_uart(key));
        assert!(!slot.release_uart(key));
        let a = classify_slot(
            SlotRead {
                bytes: media.slot(Slot::A),
                issue: None,
            },
            geometry,
            &LedSchema,
        );
        let b = classify_slot(
            SlotRead {
                bytes: media.slot(Slot::B),
                issue: None,
            },
            geometry,
            &LedSchema,
        );
        let reboot = recover(a, b, &LedSchema).unwrap();
        let selected = reboot.selected().unwrap();
        let bytes = selected.record.snapshot.bytes();
        let initial = LedConfig::new(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            u16::from_le_bytes([bytes[2], bytes[3]]),
        )
        .unwrap();
        let (rebooted, _) = controller(initial, slot);
        assert_eq!(rebooted.view().period_ms, 1600);
        assert_eq!(
            rebooted.view().duty_permille,
            LedConfig::defaults().duty_permille()
        );
        assert_eq!(rebooted.view().revision, 0);
    }

    #[test]
    fn uart_only_failed_read_and_indeterminate_commit_have_distinct_terminals() {
        let geometry = Geometry::new(2048, 256).unwrap();
        let make = |media: &mut Simulator| {
            let recovery =
                recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap();
            let service = PersistenceService::new(
                geometry,
                LedSchema,
                recovery,
                false,
                1,
                1,
                1,
                0,
                media.open_session().unwrap(),
            );
            Box::leak(Box::new(ProfileSlot::uart_only(
                service,
                Broker::<4>::new(1, 1),
            )))
        };
        let mut media = Simulator::new(geometry, 2, 44);
        let slot = make(&mut media);
        let (mut owner, _) = controller(LedConfig::defaults(), slot);
        let key = match owner.save(100) {
            LedResult::Save(SaveReceipt::Accepted(key)) => key,
            other => panic!("{other:?}"),
        };
        let command = slot.take_command(100).unwrap().unwrap();
        assert!(matches!(
            command.header.kind,
            bloxide_persistence::CommandKind::Read
        ));
        slot.complete(bloxide_persistence::Completion {
            header: command.header,
            status: bloxide_persistence::CompletionStatus::Read {
                len: command.header.len,
                issue: Some(ReadIssue::Io),
            },
            data: [0; 256],
        })
        .unwrap();
        assert_eq!(
            owner.resolve_save(key),
            LedResult::SaveStatus(SaveStatus::Failed)
        );
        assert!(slot.release_uart(key));

        let mut media = Simulator::new(geometry, 2, 55);
        let slot = make(&mut media);
        let (mut owner, _) = controller(LedConfig::defaults(), slot);
        let key = match owner.save(100) {
            LedResult::Save(SaveReceipt::Accepted(key)) => key,
            other => panic!("{other:?}"),
        };
        let mut injected = false;
        for _ in 0..128 {
            let command = slot.take_command(100).unwrap().unwrap();
            if matches!(
                command.header.kind,
                bloxide_persistence::CommandKind::Program
            ) && command.header.offset == 768
            {
                slot.complete(bloxide_persistence::Completion {
                    header: command.header,
                    status: bloxide_persistence::CompletionStatus::Failed { quiescent: false },
                    data: [0; 256],
                })
                .unwrap();
                injected = true;
                break;
            }
            slot.complete(media.execute(command)).unwrap();
        }
        assert!(injected);
        assert_eq!(
            owner.resolve_save(key),
            LedResult::SaveStatus(SaveStatus::Pending)
        );
        let q = slot.take_quiesce_request(100).unwrap();
        slot.complete_quiesce(q, true).unwrap();
        assert_eq!(
            owner.resolve_save(key),
            LedResult::SaveStatus(SaveStatus::Pending)
        );
        let verification = slot.take_command(100).unwrap().unwrap();
        assert!(matches!(
            verification.header.kind,
            bloxide_persistence::CommandKind::Read
        ));
        slot.complete(bloxide_persistence::Completion {
            header: verification.header,
            status: bloxide_persistence::CompletionStatus::Read {
                len: verification.header.len,
                issue: Some(ReadIssue::Io),
            },
            data: [0; 256],
        })
        .unwrap();
        assert_eq!(
            owner.resolve_save(key),
            LedResult::SaveStatus(SaveStatus::Indeterminate)
        );
        assert!(slot.release_uart(key));
    }
}
fn candidate(
    active: LedConfig,
    field_id: u32,
    encoded: bloxide_calibration::EncodedValue,
) -> Result<LedConfig, RejectReason> {
    let bytes: [u8; 2] = encoded.exact().map_err(|_| RejectReason::BadEncoding)?;
    let value = u16::from_le_bytes(bytes);
    match field_id {
        PERIOD_ID => LedConfig::new(value, active.duty_permille()),
        DUTY_ID => LedConfig::new(active.period_ms(), value),
        _ => return Err(RejectReason::UnknownVariable),
    }
    .map_err(|_| RejectReason::Bounds)
}
pub fn apply(controller: &mut LedController, request: &ApplyRequest) {
    controller.apply(*request);
}
pub fn read(controller: &mut LedController) {
    controller.read();
}
pub fn read_saved(controller: &mut LedController) {
    controller.read_saved();
}
pub fn release(controller: &mut LedController, key: &OperationKey) {
    controller.release(*key);
}
pub fn profile_p(controller: &mut LedController, request: &xcp_messages::PacketRequest) {
    controller.profile_p(*request);
}
pub fn profile_reset(controller: &mut LedController) {
    controller.profile_reset();
}
pub fn profile_lost(controller: &mut LedController) {
    controller.profile_lost();
}
pub fn save(controller: &mut LedController, now_us: &u64) {
    controller.save(*now_us);
}
pub fn resolve_save(controller: &mut LedController, key: &PersistenceKey) {
    controller.resolve_save(*key);
}
pub fn xcp_provider(controller: &mut LedController, request: &ProviderRequest) {
    controller.xcp_provider(*request);
}
pub fn output_completed(controller: &mut LedController) {
    controller.drain_output();
}
pub fn output_faulted(controller: &mut LedController) {
    controller.request_quiesce();
}
/// Stop closes output admission without replacing or resetting Owner state.
pub fn on_lifecycle_stop(controller: &mut LedController) {
    controller.request_quiesce();
    #[cfg(feature = "host-supervision-trace")]
    std::eprintln!("LED_OWNER_SUPERVISED_STOP output_admission=Quiescing");
}

fn xcp_write_result(outcome: OwnerOutcome<LedConfig>) -> WriteResult {
    match outcome {
        OwnerOutcome::Applied { changed, active_revision, .. } => WriteResult::Applied {
            changed,
            active_revision: active_revision.get(),
        },
        OwnerOutcome::Rejected { reason, .. } => WriteResult::Rejected {
            reason: match reason {
                RejectReason::BadEncoding => XcpReject::BadEncoding,
                RejectReason::Bounds => XcpReject::Bounds,
                RejectReason::CrossField => XcpReject::CrossField,
                RejectReason::UnknownVariable => XcpReject::UnknownVariable,
                RejectReason::Expired => XcpReject::Expired,
                RejectReason::WrongLifecycle => XcpReject::WrongLifecycle,
                RejectReason::RevisionMismatch => XcpReject::RevisionMismatch,
                RejectReason::OutputBusy => XcpReject::OutputBusy,
                RejectReason::OutputUnavailable => XcpReject::OutputUnavailable,
                RejectReason::ReadOnly => XcpReject::ReadOnly,
                RejectReason::Policy => XcpReject::UnsupportedPolicy,
            },
        },
        OwnerOutcome::Cancelled { .. } => WriteResult::Cancelled,
        OwnerOutcome::Retired { .. } => WriteResult::Retired,
        OwnerOutcome::StaleOperation { .. } => WriteResult::StaleOperation,
        OwnerOutcome::Busy { .. } => WriteResult::Busy,
    }
}

/// Absolute-time logical phase. A late poll computes current phase without replaying edges.
pub fn phase_at(config: LedConfig, epoch_us: u64, now_us: u64) -> (bool, Option<u64>) {
    let duty = config.duty_permille();
    if duty == 0 {
        return (false, None);
    }
    if duty == 1000 {
        return (true, None);
    }
    let period = u64::from(config.period_ms()) * 1000;
    let on = u64::from(config.period_ms()) * u64::from(duty);
    let elapsed = now_us.saturating_sub(epoch_us);
    let position = elapsed % period;
    let period_start = now_us.saturating_sub(position);
    if position < on {
        (true, Some(period_start.saturating_add(on)))
    } else {
        (false, Some(period_start.saturating_add(period)))
    }
}

/// One retained reply for the sole diagnostic transport. No runtime handle enters messages.
pub struct ReplySlot {
    state: Mutex<RefCell<ReplyState>>,
}
struct ReplyState {
    result: Option<LedResult>,
    overflow: bool,
}
pub struct ReplyWriter {
    slot: &'static ReplySlot,
}
pub struct ReplyReader {
    slot: &'static ReplySlot,
}
impl ReplySlot {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(ReplyState {
                result: None,
                overflow: false,
            })),
        }
    }
    pub fn split(&'static mut self) -> (ReplyWriter, ReplyReader) {
        (ReplyWriter { slot: self }, ReplyReader { slot: self })
    }
}
impl ReplyWriter {
    fn publish(&mut self, result: LedResult) {
        critical_section::with(|cs| {
            let mut state = self.slot.state.borrow(cs).borrow_mut();
            if state.result.is_some() {
                state.overflow = true;
            } else {
                state.result = Some(result);
            }
        });
    }
}
impl ReplyReader {
    pub fn take(&mut self) -> Result<Option<LedResult>, ()> {
        critical_section::with(|cs| {
            let mut state = self.slot.state.borrow(cs).borrow_mut();
            if state.overflow {
                return Err(());
            }
            Ok(state.result.take())
        })
    }
}
