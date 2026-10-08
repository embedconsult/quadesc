#![no_std]

use bloxide_core::transition::ActionResult;
use core::cell::RefCell;
use critical_section::Mutex;
pub use xcp_core::SessionState;
use xcp_core::{Dispatch, ProviderPort, ProviderRequest, Session, SubmitError, VirtualMap};
use xcp_messages::{Packet, PacketRequest, ProviderCompletion, ServiceRequest};

/// Fixed host/platform seam. Taking an effect only acknowledges transfer to
/// the provider mailbox; taking a response only acknowledges the complete TX
/// frame. The session owns protocol state, while the platform owns I/O.
pub struct SessionBridge {
    slots: Mutex<RefCell<BridgeSlots>>,
}

#[derive(Default)]
struct BridgeSlots {
    effect: Option<ProviderRequest>,
    response: Option<Packet>,
    settled: bool,
    loss_acknowledged: bool,
    trace: [Option<&'static str>; 32],
    trace_head: u8,
    trace_len: u8,
}

impl Default for SessionBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionBridge {
    pub const fn new() -> Self {
        Self {
            slots: Mutex::new(RefCell::new(BridgeSlots {
                effect: None,
                response: None,
                settled: true,
                loss_acknowledged: false,
                trace: [None; 32],
                trace_head: 0,
                trace_len: 0,
            })),
        }
    }

    pub fn effect(&self) -> Option<ProviderRequest> {
        critical_section::with(|cs| self.slots.borrow(cs).borrow().effect)
    }

    pub fn accept_effect(&self, expected: ProviderRequest) -> bool {
        critical_section::with(|cs| {
            let mut slots = self.slots.borrow(cs).borrow_mut();
            if slots.effect == Some(expected) {
                slots.effect = None;
                true
            } else {
                false
            }
        })
    }

    pub fn response(&self) -> Option<Packet> {
        critical_section::with(|cs| self.slots.borrow(cs).borrow().response)
    }

    pub fn settled(&self) -> bool {
        critical_section::with(|cs| self.slots.borrow(cs).borrow().settled)
    }

    pub fn loss_acknowledged(&self) -> bool {
        critical_section::with(|cs| self.slots.borrow(cs).borrow().loss_acknowledged)
    }

    fn acknowledge_loss(&self) {
        critical_section::with(|cs| {
            let mut slots = self.slots.borrow(cs).borrow_mut();
            slots.loss_acknowledged = true;
            slots.settled = false;
        });
    }

    pub fn accept_response(&self, expected: Packet) -> bool {
        critical_section::with(|cs| {
            let mut slots = self.slots.borrow(cs).borrow_mut();
            if slots.response == Some(expected) {
                slots.response = None;
                true
            } else {
                false
            }
        })
    }

    pub fn fence_wire(&self) {
        critical_section::with(|cs| self.slots.borrow(cs).borrow_mut().response = None);
    }

    /// Read a state entry emitted by the generated MachineSpec.
    pub fn take_trace(&self) -> Option<&'static str> {
        critical_section::with(|cs| {
            let mut slots = self.slots.borrow(cs).borrow_mut();
            if slots.trace_len == 0 {
                return None;
            }
            let head = slots.trace_head as usize;
            let value = slots.trace[head].take();
            slots.trace_head = ((head + 1) % slots.trace.len()) as u8;
            slots.trace_len -= 1;
            value
        })
    }
    fn push_trace(&self, name: &'static str) {
        critical_section::with(|cs| {
            let mut slots = self.slots.borrow(cs).borrow_mut();
            if slots.trace_len as usize == slots.trace.len() {
                return;
            }
            let tail = (slots.trace_head as usize + slots.trace_len as usize) % slots.trace.len();
            slots.trace[tail] = Some(name);
            slots.trace_len += 1;
        });
    }
}

macro_rules! state_entry {
    ($name:ident, $label:literal) => {
        pub fn $name(bridge: &&'static SessionBridge) {
            bridge.push_trace($label);
        }
    };
}
state_entry!(enter_disconnected, "Disconnected");
state_entry!(enter_closed, "Closed");
state_entry!(enter_synchronizing, "Synchronizing");
state_entry!(enter_ready, "Ready");
state_entry!(enter_awaiting_owner, "AwaitingOwner");
state_entry!(enter_resolving, "Resolving");
state_entry!(enter_quiescing, "Quiescing");
state_entry!(enter_recovery, "Recovery");

pub fn deliver(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    bridge: &&'static SessionBridge,
) -> ActionResult {
    let admitted = critical_section::with(|cs| {
        let mut slots = bridge.slots.borrow(cs).borrow_mut();
        if (effects.pending.is_some() && slots.effect.is_some())
            || (responses.pending.is_some() && slots.response.is_some())
        {
            return false;
        }
        if let Some(effect) = effects.pending.take() {
            slots.effect = Some(effect);
        }
        if let Some(response) = responses.pending.take() {
            slots.response = Some(response);
        }
        slots.settled = !session.has_pending_work();
        true
    });
    if admitted {
        ActionResult::Ok
    } else {
        fence_delivery(session, effects, responses);
        bridge.fence_wire();
        critical_section::with(|cs| {
            bridge.slots.borrow(cs).borrow_mut().settled = !session.has_pending_work();
        });
        ActionResult::Err
    }
}

pub fn fence_bridge(bridge: &&'static SessionBridge) {
    bridge.fence_wire();
}

pub fn mark_lost(bridge: &&'static SessionBridge) {
    bridge.acknowledge_loss();
}

/// One fixed provider-command slot. The concrete system action reads the
/// request and clears it only after its own bounded output admits the command.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EffectSlot {
    pending: Option<ProviderRequest>,
}

impl EffectSlot {
    pub const fn pending(&self) -> Option<&ProviderRequest> {
        self.pending.as_ref()
    }

    pub fn clear_submitted(&mut self) {
        self.pending = None;
    }
}

impl ProviderPort for EffectSlot {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        if self.pending.is_some() {
            Err(SubmitError::Busy)
        } else {
            self.pending = Some(request);
            Ok(())
        }
    }
}

/// One retained wire response. The transport action clears it only after the
/// bounded TX path admits the complete response PDU. A fence invalidates it
/// without affecting owner requests or retained outcome reconciliation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResponseSlot {
    pending: Option<Packet>,
}

impl ResponseSlot {
    pub const fn pending(&self) -> Option<&Packet> {
        self.pending.as_ref()
    }

    pub fn clear_submitted(&mut self) {
        self.pending = None;
    }

    fn record(&mut self, dispatch: Dispatch) -> ActionResult {
        match dispatch {
            Dispatch::Respond(packet) if self.pending.is_none() => {
                self.pending = Some(packet);
                ActionResult::Ok
            }
            Dispatch::Respond(_) => ActionResult::Err,
            Dispatch::Deferred | Dispatch::Ignored | Dispatch::Fenced => ActionResult::Ok,
        }
    }
}

/// Processes one complete PDU without awaiting. Any provider request and wire
/// response remain owned by fixed context slots for later concrete actions.
pub fn handle_packet(
    session: &mut Session,
    map: &VirtualMap<'_>,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &PacketRequest,
) -> ActionResult {
    if responses.pending.is_some() {
        fence_delivery(session, effects, responses);
        return ActionResult::Err;
    }
    let dispatch = session.handle_packet(map, &request.packet, request.now_us, effects);
    record_dispatch(session, effects, responses, dispatch)
}

pub fn handle_ready_packet(
    session: &mut Session,
    map: &VirtualMap<'_>,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &PacketRequest,
) -> ActionResult {
    if responses.pending.is_some() {
        fence_delivery(session, effects, responses);
        return ActionResult::Err;
    }
    let dispatch = session.handle_ready_phase(map, &request.packet, request.now_us, effects);
    record_dispatch(session, effects, responses, dispatch)
}

pub fn handle_connect_packet(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &PacketRequest,
) -> ActionResult {
    if responses.pending.is_some() {
        fence_delivery(session, effects, responses);
        return ActionResult::Err;
    }
    let dispatch =
        session.handle_connect_phase(SessionState::Disconnected, &request.packet, effects);
    record_dispatch(session, effects, responses, dispatch)
}

pub fn handle_recovery_packet(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &PacketRequest,
) -> ActionResult {
    if responses.pending.is_some() {
        fence_delivery(session, effects, responses);
        return ActionResult::Err;
    }
    let dispatch = session.handle_connect_phase(SessionState::Recovery, &request.packet, effects);
    record_dispatch(session, effects, responses, dispatch)
}

pub fn handle_busy_packet(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    _request: &PacketRequest,
) -> ActionResult {
    let dispatch = session.handle_busy_phase(effects);
    record_dispatch(session, effects, responses, dispatch)
}

pub fn handle_provider_completion(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    completion: &ProviderCompletion,
) -> ActionResult {
    let dispatch = session.complete(*completion, effects);
    record_dispatch(session, effects, responses, dispatch)
}

pub fn service(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &ServiceRequest,
) -> ActionResult {
    // This packet-only context has no partial transport boundary. Every
    // durable fence clears its response slot; SxI consumes fresh-fault provenance.
    let dispatch = session.service(request.now_us, effects).dispatch;
    record_dispatch(session, effects, responses, dispatch)
}

pub fn service_awaiting(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &ServiceRequest,
) -> ActionResult {
    let dispatch = session
        .service_in(SessionState::AwaitingOwner, request.now_us, effects)
        .dispatch;
    record_dispatch(session, effects, responses, dispatch)
}

pub fn service_resolving(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &ServiceRequest,
) -> ActionResult {
    let dispatch = session
        .service_in(SessionState::Resolving, request.now_us, effects)
        .dispatch;
    record_dispatch(session, effects, responses, dispatch)
}

pub fn service_recovery(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    request: &ServiceRequest,
) -> ActionResult {
    let dispatch = session
        .service_in(SessionState::Recovery, request.now_us, effects)
        .dispatch;
    record_dispatch(session, effects, responses, dispatch)
}

pub fn transport_lost(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
) -> ActionResult {
    fence_delivery(session, effects, responses);
    ActionResult::Ok
}

/// Infallible lifecycle hook: resources and admitted effects stay owned.
pub fn fence_delivery(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
) {
    responses.pending = None;
    let _ = session.fence(effects);
}

fn record_dispatch(
    session: &mut Session,
    effects: &mut EffectSlot,
    responses: &mut ResponseSlot,
    dispatch: Dispatch,
) -> ActionResult {
    if dispatch == Dispatch::Fenced {
        responses.pending = None;
    }
    let result = responses.record(dispatch);
    if result == ActionResult::Err {
        fence_delivery(session, effects, responses);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcp_core::{command, ProviderRequest, Region, SessionState};
    use xcp_messages::{Correlation, ProviderCompletion, SynchronizeResult, WriteResult};

    const REGIONS: [Region; 1] = [Region::read_only(1, 0, 4)];

    #[test]
    fn action_retains_effect_and_refuses_overwrite() {
        let map = VirtualMap::new(&REGIONS).unwrap();
        let mut session = Session::new();
        let mut effects = EffectSlot::default();
        let mut responses = ResponseSlot::default();
        let request = PacketRequest {
            packet: Packet::try_from_slice(&[0xFF, 0]).unwrap(),
            now_us: 0,
        };

        assert_eq!(
            handle_packet(&mut session, &map, &mut effects, &mut responses, &request),
            ActionResult::Ok
        );
        assert!(matches!(
            effects.pending(),
            Some(ProviderRequest::Synchronize { .. })
        ));
        assert_eq!(session.state(), SessionState::Synchronizing);
        assert_eq!(
            handle_packet(&mut session, &map, &mut effects, &mut responses, &request),
            ActionResult::Ok
        );
        assert_eq!(session.state(), SessionState::Recovery);
    }

    #[test]
    fn release_completion_is_processed_while_positive_response_is_retained() {
        let regions = [Region::calibration(1, 0x1000, 2)];
        let map = VirtualMap::new(&regions).unwrap();
        let mut session = Session::new();
        let mut effects = EffectSlot::default();
        let mut responses = ResponseSlot::default();
        let packet = |bytes: &[u8], now_us| PacketRequest {
            packet: Packet::try_from_slice(bytes).unwrap(),
            now_us,
        };

        assert_eq!(
            handle_packet(
                &mut session,
                &map,
                &mut effects,
                &mut responses,
                &packet(&[command::CONNECT, 0], 0)
            ),
            ActionResult::Ok
        );
        effects.clear_submitted();
        assert_eq!(
            handle_provider_completion(
                &mut session,
                &mut effects,
                &mut responses,
                &ProviderCompletion::Synchronized {
                    session_generation: 1,
                    result: SynchronizeResult::Ready { service_epoch: 1 }
                }
            ),
            ActionResult::Ok
        );
        responses.clear_submitted();
        assert_eq!(
            handle_packet(
                &mut session,
                &map,
                &mut effects,
                &mut responses,
                &packet(&[command::SET_MTA, 0, 0, 0, 0, 0x10, 0, 0], 0)
            ),
            ActionResult::Ok
        );
        responses.clear_submitted();
        assert_eq!(
            handle_packet(
                &mut session,
                &map,
                &mut effects,
                &mut responses,
                &packet(&[command::DOWNLOAD, 2, 1, 0], 1)
            ),
            ActionResult::Ok
        );
        let operation = match effects.pending().copied() {
            Some(ProviderRequest::Apply(request)) => request.operation,
            other => panic!("unexpected request: {other:?}"),
        };
        effects.clear_submitted();
        assert_eq!(
            handle_provider_completion(
                &mut session,
                &mut effects,
                &mut responses,
                &ProviderCompletion::Write {
                    correlation: operation.correlation(),
                    result: WriteResult::Applied {
                        changed: true,
                        active_revision: 1
                    }
                }
            ),
            ActionResult::Ok
        );
        assert!(responses.pending().is_some());
        assert!(matches!(
            effects.pending(),
            Some(ProviderRequest::ReleaseOutcome { .. })
        ));
        effects.clear_submitted();
        assert_eq!(
            handle_provider_completion(
                &mut session,
                &mut effects,
                &mut responses,
                &ProviderCompletion::Released {
                    correlation: Correlation {
                        session_generation: 1,
                        sequence: 0
                    }
                }
            ),
            ActionResult::Ok
        );
        assert!(!session.has_retirement());
        assert!(responses.pending().is_some());
    }
}
