#![no_std]

//! Bounded SxI framing and transport admission for scalar XCP profile S0.
//!
//! The wire format has no delimiter or escaping. After malformed or truncated
//! input, only an explicitly observed idle interval creates a new boundary.

use xcp_core::{Dispatch, ProviderPort, Session, SessionState, VirtualMap};
use xcp_messages::{Packet, ProviderCompletion};

pub const MAX_PDU: usize = 8;
pub const MAX_FRAME: usize = 11;
pub const INTER_BYTE_TIMEOUT_US: u64 = 20_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame {
    bytes: [u8; MAX_FRAME],
    len: u8,
}

impl Frame {
    pub fn try_from_parts(counter: u8, pdu: &Packet) -> Self {
        let pdu = pdu.as_slice();
        let mut bytes = [0; MAX_FRAME];
        bytes[0] = pdu.len() as u8;
        bytes[1] = counter;
        bytes[2..2 + pdu.len()].copy_from_slice(pdu);
        let checksum_index = pdu.len() + 2;
        bytes[checksum_index] = checksum(&bytes[..checksum_index]);
        Self {
            bytes,
            len: (pdu.len() + 3) as u8,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        false
    }

    pub const fn counter(&self) -> u8 {
        self.bytes[1]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Encoder {
    next_counter: u8,
}

impl Encoder {
    pub const fn new() -> Self {
        Self { next_counter: 0 }
    }

    pub const fn next_counter(&self) -> u8 {
        self.next_counter
    }

    pub fn encode(&mut self, pdu: &Packet) -> Frame {
        let frame = Frame::try_from_parts(self.next_counter, pdu);
        self.next_counter = self.next_counter.wrapping_add(1);
        frame
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CounterObservation {
    First { observed: u8 },
    InOrder { observed: u8 },
    Repeat { observed: u8 },
    Gap { expected: u8, observed: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceivedFrame {
    pub counter: u8,
    pub counter_observation: CounterObservation,
    pub packet: Packet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardReason {
    InvalidLength { observed: u8 },
    Checksum,
    Uart,
    Truncated,
    ClockRegression,
    AdmissionFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseEvent {
    Pending,
    Frame(ReceivedFrame),
    Discarded(DiscardReason),
    Recovered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Accepting,
    DiscardUntilIdle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Parser {
    bytes: [u8; MAX_FRAME],
    len: u8,
    expected_len: u8,
    last_byte_us: Option<u64>,
    last_counter: Option<u8>,
    mode: Mode,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub const fn new() -> Self {
        Self {
            bytes: [0; MAX_FRAME],
            len: 0,
            expected_len: 0,
            last_byte_us: None,
            last_counter: None,
            mode: Mode::Accepting,
        }
    }

    pub const fn is_discarding(&self) -> bool {
        matches!(self.mode, Mode::DiscardUntilIdle)
    }

    pub const fn buffered_len(&self) -> usize {
        self.len as usize
    }

    pub fn feed_byte(&mut self, now_us: u64, byte: u8) -> ParseEvent {
        if self.clock_regressed(now_us) {
            self.enter_discard(now_us);
            return ParseEvent::Discarded(DiscardReason::ClockRegression);
        }
        // Expiry precedes timestamp refresh: an arriving suffix cannot extend
        // an already expired frame, even if the scheduler did not poll.
        if !self.is_discarding() && self.len > 0 && self.idle_elapsed(now_us) {
            self.enter_discard(now_us);
            return ParseEvent::Discarded(DiscardReason::Truncated);
        }
        self.last_byte_us = Some(now_us);
        if self.is_discarding() {
            return ParseEvent::Pending;
        }

        if self.len == 0 {
            if !(1..=MAX_PDU as u8).contains(&byte) {
                self.enter_discard(now_us);
                return ParseEvent::Discarded(DiscardReason::InvalidLength { observed: byte });
            }
            self.expected_len = byte + 3;
        }

        self.bytes[self.len as usize] = byte;
        self.len += 1;
        if self.len < self.expected_len {
            return ParseEvent::Pending;
        }

        let frame_len = self.len as usize;
        let expected_sum = self.bytes[frame_len - 1];
        if checksum(&self.bytes[..frame_len - 1]) != expected_sum {
            self.enter_discard(now_us);
            return ParseEvent::Discarded(DiscardReason::Checksum);
        }

        let counter = self.bytes[1];
        let pdu_len = self.bytes[0] as usize;
        let mut pdu_bytes = [0; MAX_PDU];
        pdu_bytes[..pdu_len].copy_from_slice(&self.bytes[2..2 + pdu_len]);
        let packet = match Packet::from_parts(pdu_bytes, pdu_len as u8) {
            Ok(packet) => packet,
            Err(_) => unreachable!(),
        };
        let counter_observation = self.observe_counter(counter);
        self.clear_frame();
        ParseEvent::Frame(ReceivedFrame {
            counter,
            counter_observation,
            packet,
        })
    }

    pub fn uart_error(&mut self, now_us: u64) -> ParseEvent {
        if self.clock_regressed(now_us) {
            self.enter_discard(now_us);
            ParseEvent::Discarded(DiscardReason::ClockRegression)
        } else {
            self.enter_discard(now_us);
            ParseEvent::Discarded(DiscardReason::Uart)
        }
    }

    pub fn poll(&mut self, now_us: u64) -> ParseEvent {
        if self.clock_regressed(now_us) {
            self.enter_discard(now_us);
            return ParseEvent::Discarded(DiscardReason::ClockRegression);
        }
        if self.mode == Mode::Accepting && self.len > 0 && self.idle_elapsed(now_us) {
            self.mode = Mode::DiscardUntilIdle;
            self.clear_frame();
            return ParseEvent::Discarded(DiscardReason::Truncated);
        }
        ParseEvent::Pending
    }

    pub fn observe_idle(&mut self, now_us: u64) -> ParseEvent {
        if self.clock_regressed(now_us) {
            self.enter_discard(now_us);
            return ParseEvent::Discarded(DiscardReason::ClockRegression);
        }
        if self.mode == Mode::DiscardUntilIdle && self.idle_elapsed(now_us) {
            self.mode = Mode::Accepting;
            self.clear_frame();
            return ParseEvent::Recovered;
        }
        ParseEvent::Pending
    }

    pub fn force_discard(&mut self, now_us: u64) {
        self.enter_discard(now_us);
    }

    fn force_discard_from_last_byte(&mut self) {
        self.mode = Mode::DiscardUntilIdle;
        if self.last_byte_us.is_none() {
            self.last_byte_us = Some(0);
        }
        self.clear_frame();
    }

    fn observe_counter(&mut self, observed: u8) -> CounterObservation {
        let observation = match self.last_counter {
            None => CounterObservation::First { observed },
            Some(last) if observed == last => CounterObservation::Repeat { observed },
            Some(last) if observed == last.wrapping_add(1) => {
                CounterObservation::InOrder { observed }
            }
            Some(last) => CounterObservation::Gap {
                expected: last.wrapping_add(1),
                observed,
            },
        };
        self.last_counter = Some(observed);
        observation
    }

    fn idle_elapsed(&self, now_us: u64) -> bool {
        self.last_byte_us
            .is_some_and(|last| now_us >= last && now_us - last >= INTER_BYTE_TIMEOUT_US)
    }

    fn clock_regressed(&self, now_us: u64) -> bool {
        self.last_byte_us.is_some_and(|last| now_us < last)
    }

    fn enter_discard(&mut self, now_us: u64) {
        self.mode = Mode::DiscardUntilIdle;
        self.last_byte_us = Some(now_us);
        self.clear_frame();
    }

    fn clear_frame(&mut self) {
        self.len = 0;
        self.expected_len = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Diagnostics {
    pub valid_frames: u64,
    pub discarded_frames: u64,
    pub counter_repeats: u64,
    pub counter_gaps: u64,
    pub rx_backpressure: u64,
    pub tx_backpressure: u64,
    pub fences: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterEvent {
    Pending,
    FrameQueued,
    ControlProgress,
    ResponseQueued,
    Backpressured,
    Fenced(DiscardReason),
    Recovered,
}

/// Owned authority for one send in one adapter scope. Completion consumes it.
#[derive(Debug)]
pub struct TxLease<'id> {
    send: u64,
    frame: Frame,
    // Invariant, so nested scopes cannot coerce tokens into each other.
    brand: core::marker::PhantomData<fn(&'id ()) -> &'id ()>,
}

impl TxLease<'_> {
    pub const fn frame(&self) -> &Frame {
        &self.frame
    }
}

#[doc = include_str!("ownership.md")]
/// Initialize once, then move into the owning service within this scope.
/// Logical lifecycle reset uses `Adapter::lifecycle_fence`, never reconstruction.
/// The higher-ranked closure supplies a fresh invariant brand without allocation,
/// an instance counter, a global registry, or pointer identity.
pub fn with_adapter<R>(run: impl for<'id> FnOnce(Adapter<'id>) -> R) -> R {
    run(Adapter {
        session: Session::new(),
        parser: Parser::new(),
        encoder: Encoder::new(),
        rx: None,
        tx: None,
        active_send: None,
        next_send: 0,
        epoch: 1,
        exhausted: false,
        session_fenced: false,
        diagnostics: Diagnostics::default(),
        brand: core::marker::PhantomData,
    })
}

#[derive(Debug)]
pub struct Adapter<'id> {
    session: Session,
    parser: Parser,
    encoder: Encoder,
    rx: Option<ReceivedFrame>,
    tx: Option<Frame>,
    active_send: Option<u64>,
    next_send: u64,
    epoch: u64,
    exhausted: bool,
    // Only deduplicates repeated Session::service fence reports. Input faults
    // and completion faults still establish their own wire boundary.
    session_fenced: bool,
    diagnostics: Diagnostics,
    brand: core::marker::PhantomData<fn(&'id ()) -> &'id ()>,
}

impl<'id> Adapter<'id> {
    pub const fn session(&self) -> &Session {
        &self.session
    }

    pub const fn parser(&self) -> &Parser {
        &self.parser
    }

    pub const fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }

    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    pub const fn has_rx(&self) -> bool {
        self.rx.is_some()
    }

    pub const fn has_tx(&self) -> bool {
        self.tx.is_some() || self.active_send.is_some()
    }

    pub fn feed_byte<P: ProviderPort>(
        &mut self,
        now_us: u64,
        byte: u8,
        provider: &mut P,
    ) -> AdapterEvent {
        match self.parser.feed_byte(now_us, byte) {
            ParseEvent::Frame(frame) => {
                self.diagnostics.valid_frames = self.diagnostics.valid_frames.saturating_add(1);
                match frame.counter_observation {
                    CounterObservation::Repeat { .. } => {
                        self.diagnostics.counter_repeats =
                            self.diagnostics.counter_repeats.saturating_add(1);
                    }
                    CounterObservation::Gap { .. } => {
                        self.diagnostics.counter_gaps =
                            self.diagnostics.counter_gaps.saturating_add(1);
                    }
                    CounterObservation::First { .. } | CounterObservation::InOrder { .. } => {}
                }
                let session_accepts_packet = matches!(
                    self.session.state(),
                    SessionState::Disconnected | SessionState::Recovery | SessionState::Ready
                );
                if self.exhausted || self.rx.is_some() || self.has_tx() || !session_accepts_packet {
                    self.diagnostics.rx_backpressure =
                        self.diagnostics.rx_backpressure.saturating_add(1);
                    self.parser.force_discard(now_us);
                    self.fence(provider, DiscardReason::AdmissionFull)
                } else {
                    self.rx = Some(frame);
                    AdapterEvent::FrameQueued
                }
            }
            ParseEvent::Discarded(reason) => {
                self.diagnostics.discarded_frames =
                    self.diagnostics.discarded_frames.saturating_add(1);
                self.fence(provider, reason)
            }
            ParseEvent::Recovered => AdapterEvent::Recovered,
            ParseEvent::Pending => AdapterEvent::Pending,
        }
    }

    pub fn uart_error<P: ProviderPort>(&mut self, now_us: u64, provider: &mut P) -> AdapterEvent {
        let ParseEvent::Discarded(reason) = self.parser.uart_error(now_us) else {
            unreachable!()
        };
        self.diagnostics.discarded_frames = self.diagnostics.discarded_frames.saturating_add(1);
        self.fence(provider, reason)
    }

    pub fn poll<P: ProviderPort>(&mut self, now_us: u64, provider: &mut P) -> AdapterEvent {
        match self.parser.poll(now_us) {
            ParseEvent::Discarded(reason) => {
                self.diagnostics.discarded_frames =
                    self.diagnostics.discarded_frames.saturating_add(1);
                self.fence(provider, reason)
            }
            ParseEvent::Pending => AdapterEvent::Pending,
            ParseEvent::Recovered => AdapterEvent::Recovered,
            ParseEvent::Frame(_) => unreachable!(),
        }
    }

    pub fn observe_idle<P: ProviderPort>(&mut self, now_us: u64, provider: &mut P) -> AdapterEvent {
        match self.parser.observe_idle(now_us) {
            ParseEvent::Recovered => AdapterEvent::Recovered,
            ParseEvent::Discarded(reason) => {
                self.diagnostics.discarded_frames =
                    self.diagnostics.discarded_frames.saturating_add(1);
                self.fence(provider, reason)
            }
            ParseEvent::Pending => AdapterEvent::Pending,
            ParseEvent::Frame(_) => unreachable!(),
        }
    }

    /// One bounded fair service step: owner reconciliation first, then one RX.
    pub fn service<P: ProviderPort>(
        &mut self,
        map: &VirtualMap<'_>,
        now_us: u64,
        provider: &mut P,
    ) -> AdapterEvent {
        let control = self.session.service(now_us, provider);
        // A new provider fault revokes even an already recovered byte boundary.
        // Busy/unchanged durable reports preserve it. Do not run owner control
        // again via transport_lost: this step has already fenced Session.
        let progress = match control.dispatch {
            Dispatch::Fenced if self.session_fenced && !control.new_fault => AdapterEvent::Pending,
            Dispatch::Fenced => {
                self.parser.force_discard_from_last_byte();
                self.invalidate_transport_without_parser();
                self.session_fenced = true;
                return AdapterEvent::Fenced(DiscardReason::AdmissionFull);
            }
            other => self.handle_dispatch(other, provider),
        };
        if self.exhausted {
            return AdapterEvent::Fenced(DiscardReason::AdmissionFull);
        }
        if self.has_tx() {
            self.diagnostics.tx_backpressure = self.diagnostics.tx_backpressure.saturating_add(1);
            return AdapterEvent::Backpressured;
        }
        if let Some(frame) = self.rx.take() {
            let dispatch = self
                .session
                .handle_packet(map, &frame.packet, now_us, provider);
            // Only a genuinely admitted CONNECT resets Session's delivery fence.
            if dispatch == Dispatch::Deferred && self.session.state() == SessionState::Synchronizing
            {
                self.session_fenced = false;
            }
            return self.handle_dispatch(dispatch, provider);
        }
        progress
    }

    pub fn complete<P: ProviderPort>(
        &mut self,
        completion: ProviderCompletion,
        provider: &mut P,
    ) -> AdapterEvent {
        let dispatch = self.session.complete(completion, provider);
        self.handle_dispatch(dispatch, provider)
    }

    pub fn take_tx(&mut self) -> Option<TxLease<'id>> {
        if self.active_send.is_some() || self.exhausted {
            return None;
        }
        let frame = self.tx.take()?;
        self.active_send = Some(self.next_send);
        Some(TxLease {
            send: self.next_send,
            frame,
            brand: core::marker::PhantomData,
        })
    }

    pub fn lease_is_current(&self, lease: &TxLease<'id>) -> bool {
        !self.exhausted && self.active_send == Some(lease.send)
    }

    pub fn complete_tx(&mut self, lease: TxLease<'id>) -> bool {
        if self.lease_is_current(&lease) {
            self.active_send = None;
            true
        } else {
            false
        }
    }

    pub fn transport_lost<P: ProviderPort>(&mut self, now_us: u64, provider: &mut P) {
        self.invalidate_transport(now_us);
        self.session_fenced = true;
        let _ = self.session.transport_lost(provider);
    }

    pub fn lifecycle_fence<P: ProviderPort>(&mut self, now_us: u64, provider: &mut P) {
        self.transport_lost(now_us, provider);
    }

    fn handle_dispatch<P: ProviderPort>(
        &mut self,
        dispatch: Dispatch,
        provider: &mut P,
    ) -> AdapterEvent {
        match dispatch {
            Dispatch::Respond(packet) => {
                if self.has_tx() || self.exhausted {
                    self.diagnostics.tx_backpressure =
                        self.diagnostics.tx_backpressure.saturating_add(1);
                    return self.fence(provider, DiscardReason::AdmissionFull);
                }
                let Some(send) = self.next_send.checked_add(1) else {
                    self.exhausted = true;
                    return self.fence(provider, DiscardReason::AdmissionFull);
                };
                self.next_send = send;
                self.tx = Some(self.encoder.encode(&packet));
                AdapterEvent::ResponseQueued
            }
            Dispatch::Deferred => AdapterEvent::ControlProgress,
            Dispatch::Ignored => AdapterEvent::Pending,
            Dispatch::Fenced => self.fence(provider, DiscardReason::AdmissionFull),
        }
    }

    fn fence<P: ProviderPort>(&mut self, provider: &mut P, reason: DiscardReason) -> AdapterEvent {
        self.parser.force_discard_from_last_byte();
        self.invalidate_transport_without_parser();
        self.session_fenced = true;
        let _ = self.session.transport_lost(provider);
        AdapterEvent::Fenced(reason)
    }

    fn invalidate_transport(&mut self, now_us: u64) {
        self.parser.force_discard(now_us);
        self.invalidate_transport_without_parser();
    }

    fn invalidate_transport_without_parser(&mut self) {
        self.rx = None;
        self.tx = None;
        self.active_send = None;
        self.diagnostics.fences = self.diagnostics.fences.saturating_add(1);
        match self.epoch.checked_add(1) {
            Some(epoch) => self.epoch = epoch,
            None => self.exhausted = true,
        }
    }
}

const fn checksum(bytes: &[u8]) -> u8 {
    let mut index = 0;
    let mut sum = 0u8;
    while index < bytes.len() {
        sum = sum.wrapping_add(bytes[index]);
        index += 1;
    }
    sum
}

#[cfg(test)]
mod exhaustion;
