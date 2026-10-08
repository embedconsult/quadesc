#![no_std]
#![forbid(unsafe_code)]

//! Restricted, explicitly selected P session. The application owns the T07
//! domain, persistence service and backend pump; this type owns protocol state.

use bloxide_calibration::Owner;
use bloxide_persistence::{
    Completion, CompletionError, FailureKind, OperationKey, Outcome, PersistenceService, ReadIssue,
    Record, RejectReason, Resolve, SaveResponse, Schema, ServiceMode, ServiceState, Snapshot,
};
use bloxide_persistence_calibration::{Broker, CanonicalCalibration, capture_owner};
use xcp_messages::Packet;

pub const VIEW_BYTES: usize = 640;
pub const IDENTITY_BYTES: usize = 128;
pub const VIEW_BASE: u32 = 0x1_0000;
pub const IDENTITY_BASE: u32 = 0x1_0400;
pub const LED_BASE: u32 = 0x1000;
pub const SAVE_DEADLINE_US: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reply {
    Packet(Packet),
    Ignored,
    Fenced,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarError {
    Bounds,
    Policy,
    Busy,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityError {
    Layout,
    Unbound,
}

/// Implemented by composition using its one existing Owner; writes must call
/// the domain's canonical validated apply path, never replace Owner state.
pub trait Domain<V: CanonicalCalibration + Copy + Eq> {
    fn owner(&self) -> &Owner<V>;
    fn read_scalar(&self, address: u32) -> Option<[u8; 2]>;
    fn write_scalar(
        &mut self,
        address: u32,
        value: [u8; 2],
        now_us: u64,
    ) -> Result<(), ScalarError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Admission {
    pub disarmed: bool,
    pub maintenance: bool,
    pub schema_known: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainProof {
    pub backend_idle: bool,
    pub completions_drained: bool,
    pub transport_idle: bool,
    pub replies_drained: bool,
    pub all_clients_recorded: bool,
}
impl DrainProof {
    pub const fn complete(self) -> bool {
        self.backend_idle
            && self.completions_drained
            && self.transport_idle
            && self.replies_drained
            && self.all_clients_recorded
    }
}

#[derive(Clone, Copy)]
struct Summary {
    key: Option<OperationKey>,
    capture: Option<Snapshot>,
    outcome: Option<Outcome>,
    settled: bool,
}
impl Summary {
    const EMPTY: Self = Self {
        key: None,
        capture: None,
        outcome: None,
        settled: false,
    };
}

/// Fixed memory is created before application Start and retained across Reset.
/// A logical Reset fences the wire but keeps summaries and persistence custody.
pub struct ProfileP {
    connected: bool,
    generation: u32,
    service_epoch: u64,
    freeze: bool,
    request_generation: Option<u32>,
    request_bit: bool,
    write_fenced: bool,
    diagnostic_only: bool,
    wire_fenced: bool,
    prepared_generation: Option<u32>,
    staged_key: Option<OperationKey>,
    staged_capture: Option<Snapshot>,
    staged_client: Option<usize>,
    mta: Option<u32>,
    latch: [[u8; VIEW_BYTES]; 4],
    latch_valid: [bool; 4],
    latch_cursor: [u16; 4],
    view_generation: u64,
    summary: [Summary; 4],
    identity: [u8; IDENTITY_BYTES],
}

impl ProfileP {
    /// Structural check only. Composition and the dedicated client must verify
    /// the actual acyclic image/A2L/descriptor/policy digests before writes.
    pub fn new(identity: [u8; IDENTITY_BYTES], service_epoch: u64) -> Result<Self, IdentityError> {
        if service_epoch == 0
            || &identity[..8] != b"BLXPP001"
            || u16::from_le_bytes([identity[8], identity[9]]) != 1
            || u16::from_le_bytes([identity[10], identity[11]]) != 640
            || u32::from_le_bytes(identity[12..16].try_into().unwrap()) != 0x5000_0001
            || u32::from_le_bytes(identity[112..116].try_into().unwrap()) != VIEW_BASE
            || u32::from_le_bytes(identity[116..120].try_into().unwrap()) != LED_BASE
            || u16::from_le_bytes([identity[120], identity[121]]) != 4
            || u16::from_le_bytes([identity[122], identity[123]]) != 1
            || identity[124..128] != [4, 1, 1, 0]
        {
            return Err(IdentityError::Layout);
        }
        if identity[16..112]
            .chunks_exact(32)
            .any(|hash| hash.iter().all(|byte| *byte == hash[0]))
        {
            return Err(IdentityError::Unbound);
        }
        Ok(Self {
            connected: false,
            generation: 0,
            service_epoch,
            freeze: false,
            request_generation: None,
            request_bit: false,
            write_fenced: false,
            diagnostic_only: false,
            wire_fenced: false,
            prepared_generation: None,
            staged_key: None,
            staged_capture: None,
            staged_client: None,
            mta: None,
            latch: [[0; VIEW_BYTES]; 4],
            latch_valid: [false; 4],
            latch_cursor: [0; 4],
            view_generation: 0,
            summary: [Summary::EMPTY; 4],
            identity,
        })
    }
    pub const fn generation(&self) -> u32 {
        self.generation
    }
    pub const fn service_epoch(&self) -> u64 {
        self.service_epoch
    }
    pub const fn identity(&self) -> &[u8; IDENTITY_BYTES] {
        &self.identity
    }
    pub const fn request_bit(&self) -> bool {
        self.request_bit
    }
    pub const fn is_fenced(&self) -> bool {
        self.wire_fenced
    }
    pub const fn retained_key(&self, client: usize) -> Option<OperationKey> {
        if client < 4 {
            self.summary[client].key
        } else {
            None
        }
    }
    pub fn fence_wire(&mut self) {
        self.wire_fenced = true;
        self.latch_valid = [false; 4];
        self.latch_cursor = [0; 4];
        self.mta = None;
    }
    pub fn logical_reset(&mut self) {
        self.fence_wire();
        self.connected = false;
        self.freeze = false;
    }

    /// Called by the asynchronous backend pump outside HSM actions.
    pub fn complete_backend<S: Schema + Copy>(
        &mut self,
        service: &mut PersistenceService<S>,
        completion: Completion,
    ) -> Result<(), CompletionError> {
        let result = service.complete(completion);
        if result.is_err() {
            self.fence_wire();
            self.write_fenced = true;
        }
        result
    }

    /// Stage one internal candidate before the shared application's save call.
    /// A prior accepted summary remains visible until finish_client_save admits
    /// this exact capture. The service has only one active operation at a time.
    pub fn retain_client_capture(&mut self, client: usize, capture: Snapshot) -> bool {
        if !(1..4).contains(&client)
            || self.staged_key.is_some()
            || self.summary[client]
                .key
                .is_some_and(|_| !self.summary[client].settled)
        {
            return false;
        }
        self.staged_key = Some(capture.operation());
        self.staged_capture = Some(capture);
        self.staged_client = Some(client);
        true
    }

    /// Complete an internal client's synchronous save admission in the same
    /// serialized turn. Rejected candidates retire without replacing history.
    pub fn finish_client_save<S: Schema + Copy>(
        &mut self,
        client: usize,
        response: SaveResponse,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
    ) -> bool {
        if !(1..4).contains(&client) || self.staged_client != Some(client) {
            return false;
        }
        let (Some(key), Some(capture)) = (self.staged_key, self.staged_capture) else {
            return false;
        };
        let admitted = match response {
            SaveResponse::Accepted => matches!(service.resolve(key), Resolve::Pending),
            SaveResponse::Terminal(outcome @ Outcome::Durable { .. }) if outcome.key() == key => {
                service.resolve(key) == Resolve::Terminal(outcome)
            }
            SaveResponse::Terminal(outcome @ Outcome::Rejected { .. })
                if outcome.key() == key && service.resolve(key) == Resolve::Terminal(outcome) =>
            {
                if self.retire_staged(client, service, broker) {
                    return true;
                }
                self.fence_wire();
                return false;
            }
            _ => false,
        };
        if !admitted {
            self.fence_wire();
            return false;
        }
        self.summary[client] = Summary {
            key: Some(key),
            capture: Some(capture),
            outcome: None,
            settled: false,
        };
        self.clear_staged();
        if matches!(response, SaveResponse::Terminal(_)) {
            return self.observe(client, service, broker);
        }
        true
    }

    fn clear_staged(&mut self) {
        self.staged_key = None;
        self.staged_capture = None;
        self.staged_client = None;
    }

    fn retire_staged<S: Schema + Copy, const N: usize>(
        &mut self,
        client: usize,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<N>,
    ) -> bool {
        let Some(key) = self.staged_key else {
            return false;
        };
        let Resolve::Terminal(outcome) = service.resolve(key) else {
            return false;
        };
        if outcome.key() != key || !service.ownership_settled() {
            return false;
        }
        if broker.record_terminal(client, outcome).is_err()
            || service.release(key).is_err()
            || broker.release(client).ok().map(|k| k.sequence) != Some(key.sequence)
        {
            return false;
        }
        self.clear_staged();
        true
    }

    pub fn observe_all<S: Schema + Copy>(
        &mut self,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
    ) {
        for client in 0..4 {
            self.observe(client, service, broker);
        }
    }

    /// Reconcile a staged key whose synchronous save result was not definitive.
    /// An old accepted summary is preserved on cancellation/rejection.
    pub fn reconcile_staged<S: Schema + Copy>(
        &mut self,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
    ) -> bool {
        let (Some(key), Some(client)) = (self.staged_key, self.staged_client) else {
            return true;
        };
        let Resolve::Terminal(outcome) = service.resolve_or_cancel(key) else {
            return false;
        };
        if outcome.key() != key || !service.ownership_settled() {
            return false;
        }
        if broker.record_terminal(client, outcome).is_err() {
            return false;
        }
        if service.release(key).is_err() {
            return false;
        }
        if broker.release(client).ok().map(|k| k.sequence) != Some(key.sequence) {
            return false;
        }
        if matches!(
            outcome,
            Outcome::Durable { .. } | Outcome::Failed { .. } | Outcome::Indeterminate { .. }
        ) {
            self.summary[client] = Summary {
                key: Some(key),
                capture: self.staged_capture,
                outcome: Some(outcome),
                settled: true,
            };
            self.write_fenced = true;
        }
        self.clear_staged();
        true
    }

    /// Exact +1 preflight and both real handshakes are required before a normal
    /// new CONNECT. Any partial handshake remains fenced for reconciliation.
    pub fn advance_namespace<S: Schema + Copy>(
        &mut self,
        proof: DrainProof,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
        io_epoch: u64,
    ) -> bool {
        self.fence_wire();
        self.connected = false;
        let (Some(next_epoch), Some(next_generation)) = (
            self.service_epoch.checked_add(1),
            self.generation.checked_add(1),
        ) else {
            return false;
        };
        if self.staged_key.is_some()
            || !proof.complete()
            || !service.ownership_settled()
            || service.mode() != ServiceMode::Stopped
            || service.health().write_locked
            || self.summary.iter().any(|s| s.key.is_some() && !s.settled)
        {
            return false;
        }
        if broker.advance_epoch(next_epoch, next_generation).is_err() {
            return false;
        }
        if service
            .resume(next_epoch, next_generation, io_epoch)
            .is_err()
        {
            return false;
        }
        self.service_epoch = next_epoch;
        self.prepared_generation = Some(next_generation);
        self.wire_fenced = false;
        self.diagnostic_only = false;
        true
    }

    /// Observe the actual service and copy its exact terminal before releasing
    /// the service and broker. Unsettled Indeterminate stays in original custody.
    pub fn observe<const N: usize, S: Schema + Copy>(
        &mut self,
        client: usize,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<N>,
    ) -> bool {
        if client >= 4 || client >= N {
            return false;
        }
        let Some(key) = self.summary[client].key else {
            return false;
        };
        let outcome = match service.resolve(key) {
            Resolve::Terminal(outcome) if outcome.key() == key => outcome,
            _ => return false,
        };
        if !service.ownership_settled() {
            if matches!(outcome, Outcome::Indeterminate { .. }) {
                self.summary[client].outcome = Some(outcome);
                self.write_fenced = true;
            }
            return false;
        }
        if broker.record_terminal(client, outcome).is_err() {
            self.fence_wire();
            return false;
        }
        self.summary[client].outcome = Some(outcome);
        if service.release(key).is_err() {
            self.fence_wire();
            return false;
        }
        if broker.release(client).ok().map(|k| k.sequence) != Some(key.sequence) {
            self.fence_wire();
            return false;
        }
        self.summary[client].settled = true;
        if client == 0 && self.request_generation == Some(self.generation) {
            self.request_bit = !matches!(outcome, Outcome::Durable { .. });
        }
        if matches!(
            outcome,
            Outcome::Failed { .. } | Outcome::Indeterminate { .. }
        ) {
            self.write_fenced = true;
        }
        true
    }

    /// Application supplies actual backend and transport drain evidence. A fault
    /// reconnect is diagnostic only; it does not clear the persistence epoch.
    pub fn diagnostic_reconnect<S: Schema + Copy>(
        &mut self,
        proof: DrainProof,
        service: &PersistenceService<S>,
    ) -> bool {
        if self.staged_key.is_some()
            || !proof.complete()
            || !service.ownership_settled()
            || !self.write_fenced
        {
            return false;
        }
        if self.generation == u32::MAX {
            self.fence_wire();
            return false;
        }
        let next = self.generation + 1;
        self.connected = false;
        self.prepared_generation = Some(next);
        self.freeze = false;
        self.request_bit = false;
        self.request_generation = None;
        self.wire_fenced = false;
        self.diagnostic_only = true;
        self.mta = None;
        self.latch_valid = [false; 4];
        self.latch_cursor = [0; 4];
        true
    }

    #[allow(clippy::too_many_arguments)] // Explicit borrowed owner/service/broker turn.
    pub fn handle<V, S, D, C>(
        &mut self,
        packet: &Packet,
        domain: &mut D,
        schema: &S,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
        admission: Admission,
        now_us: u64,
        mut clock: C,
    ) -> Reply
    where
        V: CanonicalCalibration + Copy + Eq,
        S: Schema + Copy,
        D: Domain<V>,
        C: FnMut() -> u64,
    {
        let b = packet.as_slice();
        if self.wire_fenced {
            return Reply::Fenced;
        }
        if b[0] == 0xff {
            return self.connect(b, service);
        }
        if !self.connected {
            return Reply::Ignored;
        }
        match b[0] {
            0xfe => {
                if b.len() != 1 {
                    err(0x21)
                } else if self.summary[0].key.is_some() && !self.summary[0].settled {
                    self.fence_wire();
                    Reply::Fenced
                } else {
                    self.connected = false;
                    self.freeze = false;
                    self.mta = None;
                    self.latch_valid[0] = false;
                    ok(&[0xff])
                }
            }
            0xfd => {
                if b.len() == 1 {
                    ok(&[0xff, self.request_bit as u8, 0, 0, 0, 0])
                } else {
                    err(0x21)
                }
            }
            0xfc => {
                if b.len() == 1 {
                    err(0x00)
                } else {
                    err(0x21)
                }
            }
            0xf9 => self.save(
                b,
                domain.owner(),
                schema,
                service,
                broker,
                admission,
                now_us,
                &mut clock,
            ),
            0xf6 => self.set_mta(b, service),
            0xf5 => self.upload(b, domain),
            0xf0 => self.download(b, domain, admission, now_us),
            0xe9 => {
                if b.len() == 1 {
                    ok(&[0xff, 1, 1])
                } else {
                    err(0x21)
                }
            }
            0xe8 => self.get_segment(b),
            0xe7 => self.get_page_info(b),
            0xea => self.get_page(b),
            0xeb => self.set_page(b),
            0xe6 => self.set_freeze(b),
            0xe5 => self.get_freeze(b),
            _ => err(0x20),
        }
    }

    fn connect<S: Schema + Copy>(&mut self, b: &[u8], service: &PersistenceService<S>) -> Reply {
        if b.len() != 2 {
            return err(0x21);
        }
        if b[1] != 0 {
            return err(0x22);
        }
        if self.connected
            || !service.ownership_settled()
            || (self.generation > 0 && self.prepared_generation.is_none())
            || self.staged_key.is_some()
        {
            self.fence_wire();
            return Reply::Fenced;
        }
        let Some(next) = self
            .prepared_generation
            .take()
            .or_else(|| self.generation.checked_add(1))
        else {
            self.fence_wire();
            return Reply::Fenced;
        };
        self.generation = next;
        self.connected = true;
        self.freeze = false;
        self.request_bit = false;
        self.request_generation = None;
        self.mta = None;
        self.latch_valid[0] = false;
        ok(&[0xff, 1, 0, 8, 8, 0, 1, 1])
    }

    #[allow(clippy::too_many_arguments)] // Preserve the admission clock and owner borrow explicitly.
    fn save<V, S, C>(
        &mut self,
        b: &[u8],
        owner: &Owner<V>,
        schema: &S,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
        admission: Admission,
        now_us: u64,
        clock: &mut C,
    ) -> Reply
    where
        V: CanonicalCalibration + Copy + Eq,
        S: Schema + Copy,
        C: FnMut() -> u64,
    {
        if b.len() != 4 {
            return err(0x21);
        }
        if b[1] != 1 || b[2] != 0 || b[3] != 0 {
            return err(0x22);
        }
        if !admission.disarmed
            || !admission.maintenance
            || owner.mode() != bloxide_calibration::OwnerMode::Running
        {
            return err(0x27);
        }
        if !self.freeze {
            return err(0x27);
        }
        if self.diagnostic_only {
            return err(0x27);
        }
        if self.write_fenced
            || self.staged_key.is_some()
            || self.request_bit
            || (self.summary[0].key.is_some() && !self.summary[0].settled)
        {
            return err(0x10);
        }
        if !admission.schema_known || service.health().write_locked {
            return err(0x24);
        }
        if service.mode() != ServiceMode::Running {
            return err(0x27);
        }
        if service.state() != ServiceState::Idle {
            return err(0x10);
        }
        let Some(deadline) = now_us.checked_add(SAVE_DEADLINE_US) else {
            return err(0x31);
        };
        if clock() >= deadline {
            return err(0x31);
        }
        let key = match broker.reserve(0) {
            Ok(k) => OperationKey {
                service_epoch: k.service_epoch,
                session_generation: k.session_generation,
                sequence: k.sequence,
            },
            Err(_) => return err(0x10),
        };
        let calibration_key = bloxide_calibration::OperationKey {
            service_epoch: key.service_epoch,
            session_generation: key.session_generation,
            sequence: key.sequence,
        };
        let capture = match capture_owner(owner, schema, calibration_key, None) {
            Ok(c) => c,
            Err(_) => {
                self.cancel_staged(key, service, broker);
                return if self.wire_fenced {
                    Reply::Fenced
                } else {
                    err(0x24)
                };
            }
        };
        let result = service.save(key, capture, deadline, clock);
        match result {
            SaveResponse::Accepted | SaveResponse::Terminal(Outcome::Durable { .. }) => {
                self.summary[0] = Summary {
                    key: Some(key),
                    capture: Some(capture),
                    outcome: None,
                    settled: false,
                };
                self.request_generation = Some(self.generation);
                self.request_bit = true;
                if matches!(result, SaveResponse::Terminal(_)) {
                    self.observe(0, service, broker);
                }
                ok(&[0xff])
            }
            SaveResponse::Terminal(Outcome::Rejected { reason, .. }) => {
                self.cancel_staged(key, service, broker);
                if self.wire_fenced {
                    return Reply::Fenced;
                }
                match reason {
                    RejectReason::Expired => err(0x31),
                    RejectReason::RateLimited | RejectReason::SequenceExhausted => err(0x10),
                    RejectReason::WrongLifecycle => err(0x27),
                    _ => err(0x24),
                }
            }
            _ => {
                self.staged_key = Some(key);
                self.staged_capture = Some(capture);
                self.staged_client = Some(0);
                self.fence_wire();
                Reply::Fenced
            }
        }
    }

    fn cancel_staged<S: Schema + Copy>(
        &mut self,
        key: OperationKey,
        service: &mut PersistenceService<S>,
        broker: &mut Broker<4>,
    ) {
        if let Resolve::Terminal(outcome) = service.resolve_or_cancel(key)
            && service.ownership_settled()
            && broker.record_terminal(0, outcome).is_ok()
            && service.release(key).is_ok()
            && broker.release(0).ok().map(|k| k.sequence) == Some(key.sequence)
        {
            return;
        }
        self.staged_key = Some(key);
        self.staged_client = Some(0);
        self.fence_wire();
    }

    fn get_segment(&self, b: &[u8]) -> Reply {
        if b.len() != 5 || b[4] != 0 {
            return err(0x21);
        }
        if b[1] > 1 {
            return err(0x22);
        }
        if b[2] != 0 {
            return err(0x28);
        }
        if b[1] == 0 {
            match b[3] {
                0 => ok(&[0xff, 0, 0, 0, 0, 0x10, 0, 0]),
                1 => ok(&[0xff, 0, 0, 0, 4, 0, 0, 0]),
                _ => err(0x22),
            }
        } else if b[3] == 0 {
            ok(&[0xff, 1, 0, 0, 0, 0])
        } else {
            err(0x22)
        }
    }
    fn get_page_info(&self, b: &[u8]) -> Reply {
        if b.len() != 4 || b[1] != 0 {
            return err(0x21);
        }
        if b[2] != 0 {
            return err(0x28);
        }
        if b[3] != 0 {
            return err(0x26);
        }
        ok(&[0xff, 0x3f, 0])
    }
    fn get_page(&self, b: &[u8]) -> Reply {
        if b.len() != 3 {
            return err(0x21);
        }
        if b[1] != 1 && b[1] != 2 {
            return err(0x27);
        }
        if b[2] != 0 {
            return err(0x28);
        }
        ok(&[0xff, 0, 0, 0])
    }
    fn set_page(&self, b: &[u8]) -> Reply {
        if b.len() != 4 {
            return err(0x21);
        }
        if b[1] < 1 || b[1] > 3 {
            return err(0x27);
        }
        if b[2] != 0 {
            return err(0x28);
        }
        if b[3] != 0 {
            return err(0x26);
        }
        if self.unresolved() {
            return err(0x10);
        }
        if self.diagnostic_only {
            return err(0x27);
        }
        ok(&[0xff])
    }
    fn set_freeze(&mut self, b: &[u8]) -> Reply {
        if b.len() != 3 {
            return err(0x21);
        }
        if b[1] > 1 {
            return err(0x22);
        }
        if b[2] != 0 {
            return err(0x28);
        }
        if self.unresolved() {
            return err(0x10);
        }
        if self.diagnostic_only {
            return err(0x27);
        }
        self.freeze = b[1] == 1;
        ok(&[0xff])
    }
    fn get_freeze(&self, b: &[u8]) -> Reply {
        if b.len() != 3 {
            return err(0x21);
        }
        if b[1] != 0 {
            return err(0x21);
        }
        if b[2] != 0 {
            return err(0x28);
        }
        ok(&[0xff, 0, self.freeze as u8])
    }
    fn unresolved(&self) -> bool {
        self.summary[0].key.is_some() && !self.summary[0].settled
    }

    fn set_mta<S: Schema + Copy>(&mut self, b: &[u8], service: &PersistenceService<S>) -> Reply {
        if b.len() != 8 || b[1] != 0 || b[2] != 0 {
            return err(0x21);
        }
        if b[3] != 0 {
            return err(0x22);
        }
        let address = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        if address == VIEW_BASE {
            if !self.make_view(0, service) {
                return Reply::Fenced;
            }
            self.mta = Some(address);
            self.latch_cursor[0] = 0;
            return ok(&[0xff]);
        }
        if (VIEW_BASE..VIEW_BASE + VIEW_BYTES as u32).contains(&address) {
            if !self.latch_valid[0] {
                return err(0x24);
            }
            self.mta = Some(address);
            self.latch_cursor[0] = (address - VIEW_BASE) as u16;
            return ok(&[0xff]);
        }
        self.latch_valid[0] = false;
        self.latch_cursor[0] = 0;
        self.mta = Some(address);
        ok(&[0xff])
    }
    fn upload<V: CanonicalCalibration + Copy + Eq, D: Domain<V>>(
        &mut self,
        b: &[u8],
        domain: &D,
    ) -> Reply {
        if b.len() != 2 {
            return err(0x21);
        }
        let n = b[1] as usize;
        if !(1..=7).contains(&n) {
            return err(0x22);
        }
        let Some(address) = self.mta else {
            return err(0x24);
        };
        let Some(end) = address.checked_add(n as u32) else {
            return err(0x22);
        };
        let mut out = [0u8; 8];
        out[0] = 0xff;
        if (VIEW_BASE..VIEW_BASE + VIEW_BYTES as u32).contains(&address) {
            if !self.latch_valid[0] || end > VIEW_BASE + VIEW_BYTES as u32 {
                return err(0x24);
            }
            let start = (address - VIEW_BASE) as usize;
            out[1..=n].copy_from_slice(&self.latch[0][start..start + n]);
            self.latch_cursor[0] = (end - VIEW_BASE) as u16;
            if end == VIEW_BASE + VIEW_BYTES as u32 {
                self.latch_valid[0] = false;
            }
        } else if (IDENTITY_BASE..IDENTITY_BASE + IDENTITY_BYTES as u32).contains(&address) {
            if end > IDENTITY_BASE + IDENTITY_BYTES as u32 {
                return err(0x24);
            }
            let start = (address - IDENTITY_BASE) as usize;
            out[1..=n].copy_from_slice(&self.identity[start..start + n]);
        } else if address == LED_BASE || address == LED_BASE + 2 {
            if n != 2 {
                return err(0x24);
            }
            let Some(bytes) = domain.read_scalar(address) else {
                return err(0x24);
            };
            out[1..3].copy_from_slice(&bytes);
        } else {
            return err(0x24);
        }
        self.mta = Some(end);
        ok(&out[..n + 1])
    }
    fn download<V: CanonicalCalibration + Copy + Eq, D: Domain<V>>(
        &mut self,
        b: &[u8],
        domain: &mut D,
        admission: Admission,
        now_us: u64,
    ) -> Reply {
        if b.len() < 2 || b.len() != b[1] as usize + 2 {
            return err(0x21);
        }
        if b[1] != 2 {
            return err(0x22);
        }
        let Some(address) = self.mta else {
            return err(0x24);
        };
        if (VIEW_BASE..VIEW_BASE + VIEW_BYTES as u32).contains(&address)
            || (IDENTITY_BASE..IDENTITY_BASE + IDENTITY_BYTES as u32).contains(&address)
        {
            return err(0x23);
        }
        if address != LED_BASE && address != LED_BASE + 2 {
            return err(0x24);
        }
        if self.diagnostic_only
            || matches!(self.summary[0].outcome, Some(Outcome::Indeterminate { .. }))
            || !admission.disarmed
            || !admission.maintenance
        {
            return err(0x27);
        }
        match domain.write_scalar(address, [b[2], b[3]], now_us) {
            Ok(()) => {
                self.mta = address.checked_add(2);
                ok(&[0xff])
            }
            Err(ScalarError::Bounds) => err(0x22),
            Err(ScalarError::Policy) => err(0x27),
            Err(ScalarError::Busy) => err(0x10),
        }
    }

    pub fn make_view<S: Schema + Copy>(
        &mut self,
        client: usize,
        service: &PersistenceService<S>,
    ) -> bool {
        if client >= 4 {
            return false;
        }
        let Some(generation) = self.view_generation.checked_add(1) else {
            self.fence_wire();
            return false;
        };
        self.view_generation = generation;
        let view = &mut self.latch[client];
        *view = [0; VIEW_BYTES];
        view[0..8].copy_from_slice(b"BLXPVW01");
        put16(view, 8, 1);
        put16(view, 10, VIEW_BYTES as u16);
        let summary = self.summary[client];
        let durable = service.read_durable();
        let mut flags = 0u32;
        if summary.key.is_some() {
            flags |= 1;
        }
        if summary.capture.is_some() {
            flags |= 2;
        }
        if durable.is_some() {
            flags |= 4;
        }
        if summary.settled {
            flags |= 8;
        }
        put32(view, 12, flags);
        put64(view, 16, generation);
        put64(view, 24, self.service_epoch);
        if let Some(key) = summary.key {
            put64(view, 40, key.service_epoch);
            put32(view, 48, key.session_generation);
            put64(view, 56, key.sequence);
        }
        put32(view, 32, self.generation);
        let state = match summary.outcome {
            None if summary.key.is_some() => 1,
            None if service.mode() == ServiceMode::ReadOnlyFault => 5,
            None => 0,
            Some(Outcome::Durable { .. }) => 2,
            Some(Outcome::Failed { .. }) => 3,
            Some(Outcome::Indeterminate { .. }) => 4,
            Some(_) => 5,
        };
        put32(view, 36, state);
        if let Some(Outcome::Failed { reason, .. } | Outcome::Indeterminate { reason, .. }) =
            summary.outcome
        {
            put32(view, 52, reason_code(reason));
        }
        view[128..384].fill(0xff);
        view[384..640].fill(0xff);
        if let Some(c) = summary.capture {
            put64(view, 64, c.source_epoch());
            put32(view, 72, c.source_revision());
            put16(view, 76, c.schema());
            put16(view, 78, c.payload_len());
            view[128..128 + c.bytes().len()].copy_from_slice(c.bytes());
        }
        if let Some(r) = durable {
            fill_durable(view, r);
        }
        view[120] = if client == 0 {
            self.request_bit as u8
        } else {
            0
        };
        view[121] = if client == 0 { self.freeze as u8 } else { 0 };
        self.latch_valid[client] = true;
        self.latch_cursor[client] = 0;
        true
    }

    /// Internal clients use their own retained latch and sequential cursor.
    /// Errors leave the cursor and bytes untouched.
    pub fn upload_client(&mut self, client: usize, count: u8) -> Option<Packet> {
        if !(1..4).contains(&client) || !(1..=7).contains(&count) || !self.latch_valid[client] {
            return None;
        }
        let start = usize::from(self.latch_cursor[client]);
        let end = start.checked_add(count as usize)?;
        if end > VIEW_BYTES {
            return None;
        }
        let mut bytes = [0u8; 8];
        bytes[0] = 0xff;
        bytes[1..=count as usize].copy_from_slice(&self.latch[client][start..end]);
        self.latch_cursor[client] = end as u16;
        if end == VIEW_BYTES {
            self.latch_valid[client] = false;
        }
        Packet::from_parts(bytes, count + 1).ok()
    }
}

fn fill_durable(v: &mut [u8; VIEW_BYTES], r: Record) {
    let s = r.snapshot;
    put64(v, 80, r.sequence);
    put64(v, 88, s.source_epoch());
    put32(v, 96, s.source_revision());
    put16(v, 100, s.schema());
    put16(v, 102, s.payload_len());
    put32(v, 104, s.operation().session_generation);
    put64(v, 112, s.operation().sequence);
    v[384..384 + s.bytes().len()].copy_from_slice(s.bytes());
}
fn reason_code(r: FailureKind) -> u32 {
    match r {
        FailureKind::Backend => 1,
        FailureKind::Verification => 2,
        FailureKind::WearUnavailable => 3,
        FailureKind::RecoveryChanged => 9,
        FailureKind::Correlation => 5,
        FailureKind::Timeout => 6,
        FailureKind::Read(ReadIssue::Io) => 1,
        FailureKind::Read(ReadIssue::CorrectedEcc | ReadIssue::UncorrectableEcc) => 7,
        FailureKind::UnsafeErasePrestate => 9,
        FailureKind::SequenceExhausted => 10,
    }
}
fn put16(v: &mut [u8], o: usize, n: u16) {
    v[o..o + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(v: &mut [u8], o: usize, n: u32) {
    v[o..o + 4].copy_from_slice(&n.to_le_bytes());
}
fn put64(v: &mut [u8], o: usize, n: u64) {
    v[o..o + 8].copy_from_slice(&n.to_le_bytes());
}
fn ok(b: &[u8]) -> Reply {
    Reply::Packet(Packet::try_from_slice(b).expect("bounded P reply"))
}
fn err(code: u8) -> Reply {
    ok(&[0xfe, code])
}
