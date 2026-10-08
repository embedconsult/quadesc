//! Conditional MAIN-slot admission and single-owner flash transport.
//!
//! Production NVMNW sequencing is conditional on a checked evidence record.
//! The selected target has no such record: chip ECC, RAM closure, timing and
//! durable wear authority remain unproved. A command is never activated by
//! `basic_readiness` or a caller's Boolean.
use crate::io::RegisterIo;
#[allow(dead_code)]
mod registers;

pub const MAIN_END: u32 = 0x80000;
pub const BANK1_START: u32 = 0x40000;
pub const SLOT_BYTES: u32 = 2048;
pub const GRANULE_BYTES: u32 = 256;
pub const BODY_END: u32 = 768;
pub const MARKER_END: u32 = 1024;

/// Read-only startup snapshot. This is a diagnostic prerequisite, not a
/// write qualification or a promise that a slot read is ECC-safe. The A/B
/// protection register names do not establish which physical bank is mapped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlashObservation {
    pub factory_sramflash: u32,
    pub fri_read_control: u32,
    pub fri_interface_control: u32,
    pub command_status: u32,
    pub command_we_protect_a: u32,
    pub command_we_protect_b: u32,
    pub protect_nonmain: u32,
}
/// Read-only facts required in addition to the seven-word diagnostic.
/// SECSTATUS is read-only; FLBANKSWP is write-only and is never read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlashSafetySnapshot {
    pub diagnostic: FlashObservation,
    pub security_status: u32,
    pub static_bank1_a: u32,
    pub static_bank1_b: u32,
    pub ecc_sec_flag: u32,
    pub ecc_ded_flag: u32,
    pub sysctl_flashsec: u32,
    pub sysctl_systemcfg: u32,
}
/// Sticky controller and ECC attribution captured before any later clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlashErrorSnapshot {
    pub command_status: u32,
    pub sec_flag: u32,
    pub sec_address: u32,
    pub sec_master: u32,
    pub ded_flag: u32,
    pub ded_address: u32,
    pub ded_master: u32,
    pub sysctl_ris: u32,
}
impl FlashObservation {
    /// Necessary conditions only. The datasheet excludes RWAIT=0 at 32 MHz.
    pub const fn basic_readiness(self) -> bool {
        let size_kib = self.factory_sramflash & 0xfff;
        let banks = (self.factory_sramflash >> 12) & 3;
        let rwait = (self.fri_read_control >> 8) & 15;
        size_kib == 512
            && banks == 2
            && rwait >= 1
            && self.command_status & 4 == 0
            && self.protect_nonmain & 3 == 3
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot {
    A,
    B,
}
impl Slot {
    pub const fn base(self) -> u32 {
        match self {
            Self::A => 0x78000,
            Self::B => 0x7c000,
        }
    }
    pub const fn end(self) -> u32 {
        self.base() + SLOT_BYTES
    }
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Read,
    ConsumeErasePermit { lease: u64, issuance: u64 },
    Erase { permit: u64 },
    Program,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub io_epoch: u64,
    pub command_sequence: u64,
    pub slot: Slot,
    pub kind: Kind,
    pub offset: u32,
    pub len: u16,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub header: Header,
    pub data: [u8; 256],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Ok,
    WearGranted { permit: u64 },
    WearDenied,
    Failed { quiescent: bool },
    Read { len: u16, issue: Option<ReadIssue> },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadIssue {
    Io,
    CorrectedEcc,
    UncorrectableEcc,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Completion {
    pub header: Header,
    pub status: Status,
    pub data: [u8; 256],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuiesceRequest {
    pub outstanding: Header,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidRange,
    InvalidKind,
    Unqualified,
    NoAuthority,
    SelectedSlot,
    Stale,
    Busy,
    Conflict,
    Exhausted,
    Uncertain,
    NoCompletion,
}

/// Checked range; only a slot-relative offset enters this API. No caller address.
pub fn address(header: Header) -> Result<Option<u32>, Error> {
    let offset = header.offset;
    let end = match header.kind {
        Kind::Read => {
            if header.len == 0 || header.len > 256 {
                return Err(Error::InvalidRange);
            }
            offset
                .checked_add(u32::from(header.len))
                .ok_or(Error::InvalidRange)?
        }
        Kind::Program => {
            if header.len != 256 || !offset.is_multiple_of(GRANULE_BYTES) {
                return Err(Error::InvalidRange);
            }
            let end = offset
                .checked_add(GRANULE_BYTES)
                .ok_or(Error::InvalidRange)?;
            if end > MARKER_END {
                return Err(Error::InvalidRange);
            }
            end
        }
        Kind::Erase { permit } => {
            if permit == 0 || offset != 0 || header.len != 0 {
                return Err(Error::InvalidKind);
            }
            SLOT_BYTES
        }
        Kind::ConsumeErasePermit { lease, issuance } => {
            if lease == 0 || issuance == 0 || offset != 0 || header.len != 0 {
                return Err(Error::InvalidKind);
            }
            return Ok(None);
        }
    };
    if end > SLOT_BYTES {
        return Err(Error::InvalidRange);
    }
    let start = header
        .slot
        .base()
        .checked_add(offset)
        .ok_or(Error::InvalidRange)?;
    let physical_end = header
        .slot
        .base()
        .checked_add(end)
        .ok_or(Error::InvalidRange)?;
    if start < BANK1_START || physical_end > MAIN_END || physical_end > header.slot.end() {
        return Err(Error::InvalidRange);
    }
    Ok(Some(start))
}

/// Controller admission is tied to a specific checked evidence record.
#[derive(Clone, Copy)]
pub struct Qualification {
    bits: u16,
}
impl Qualification {
    const ALL: u16 = (1 << 9) - 1;
    pub const fn unqualified() -> Self {
        Self { bits: 0 }
    }
    pub const fn writes_qualified(self) -> bool {
        self.bits == Self::ALL
    }
    /// Source checks only. The evidence identifiers name independently
    /// retained fixture reports; their contents require review before a
    /// selected target supplies this record. No target currently does.
    pub fn from_evidence(
        snapshot: FlashSafetySnapshot,
        evidence: &AdmissionEvidence,
    ) -> Result<Self, Error> {
        if snapshot != evidence.snapshot
            || !snapshot.diagnostic.basic_readiness()
            || snapshot.diagnostic.command_status & 0x11f0 != 0
            || snapshot.diagnostic.fri_interface_control & 4 != 0
            || snapshot.security_status & 0x1000 != 0
            || snapshot.ecc_sec_flag & 1 != 0
            || snapshot.ecc_ded_flag & 3 != 0
            || snapshot.sysctl_flashsec & 4 != 0
            || snapshot.static_bank1_a != evidence.expected_static_bank1_a
            || snapshot.static_bank1_b != evidence.expected_static_bank1_b
            || evidence.command_deadline_us == 0
            || evidence.reports.iter().any(|digest| *digest == [0; 32])
        {
            return Err(Error::Unqualified);
        }
        Ok(Self { bits: Self::ALL })
    }
    #[cfg(test)]
    fn test_with_missing(missing: u16) -> Self {
        Self {
            bits: Self::ALL & !missing,
        }
    }
}
/// Exact reported source identities: MAIN inventory, bank/protection mapping,
/// ECC/read behavior, RAM/IRQ closure, timing/rail, and durable wear authority.
/// This carries no boot credit or caller-selected permit.
pub struct AdmissionEvidence {
    snapshot: FlashSafetySnapshot,
    expected_static_bank1_a: u32,
    expected_static_bank1_b: u32,
    reports: [[u8; 32]; 6],
    command_deadline_us: u64,
}

/// Real controller ownership. Constructed only while splitting Peripherals.
/// RegisterIo is held privately; there is no clone, raw-address or actor API.
pub struct FlashController<I: RegisterIo> {
    sequence: registers::Sequence<I>,
    admission: Qualification,
    max_command_duration_us: u64,
    terminal: Option<ControllerState>,
    last_error: Option<FlashErrorSnapshot>,
}
impl<I: RegisterIo> FlashController<I> {
    pub(crate) fn new(io: I) -> Self {
        Self {
            sequence: registers::Sequence::new(io),
            admission: Qualification::unqualified(),
            max_command_duration_us: 0,
            terminal: None,
            last_error: None,
        }
    }
    /// Activation is bound to fresh register values and explicit report
    /// identities. It remains unavailable in the selected target composition.
    pub fn activate(&mut self, evidence: &AdmissionEvidence) -> Result<Qualification, Error> {
        if !self.sequence.idle() || self.terminal.is_some() {
            return Err(Error::Busy);
        }
        self.admission = Qualification::from_evidence(self.observe_safety()?, evidence)?;
        self.max_command_duration_us = evidence.command_deadline_us;
        Ok(self.admission)
    }
    /// Bench-only fixed record read after ownership; no write or production admission.
    pub fn read_bench_record(&mut self) -> Result<[u8;32],Error> {
        let observation=self.observe()?;
        if observation.command_status & 4 != 0 {return Err(Error::Busy);}
        let mut bytes=[0u8;32];
        for i in 0..8 {bytes[i*4..i*4+4].copy_from_slice(&self.sequence.read_reg(0x78000+i*4).to_le_bytes());}
        Ok(bytes)
    }
    pub fn observe_safety(&mut self) -> Result<FlashSafetySnapshot, Error> {
        Ok(FlashSafetySnapshot {
            diagnostic: self.observe()?,
            security_status: self.sequence.read_reg(0x400b_2048),
            static_bank1_a: self.sequence.read_reg(0x4004_7608),
            static_bank1_b: self.sequence.read_reg(0x4004_760c),
            ecc_sec_flag: self.sequence.read_reg(0x4002_d030),
            ecc_ded_flag: self.sequence.read_reg(0x4002_d050),
            sysctl_flashsec: self.sequence.read_reg(0x400b_0030),
            sysctl_systemcfg: self.sequence.read_reg(0x400b_0180),
        })
    }
    fn capture_error(&self) -> FlashErrorSnapshot {
        FlashErrorSnapshot {
            command_status: self.sequence.read_reg(0x4004_33d0),
            sec_flag: self.sequence.read_reg(0x4002_d030),
            sec_address: self.sequence.read_reg(0x4002_d038),
            sec_master: self.sequence.read_reg(0x4002_d03c),
            ded_flag: self.sequence.read_reg(0x4002_d050),
            ded_address: self.sequence.read_reg(0x4002_d058),
            ded_master: self.sequence.read_reg(0x4002_d05c),
            sysctl_ris: self.sequence.read_reg(0x400b_0030),
        }
    }
    pub const fn last_error(&self) -> Option<FlashErrorSnapshot> {
        self.last_error
    }
    /// Fixed chunk read through the private register owner. A DED reset
    /// cannot return and must be handled by the separately reviewed NMI path.
    pub fn read_chunk(&mut self, header: Header) -> Result<[u8; 256], ReadIssue> {
        if !matches!(header.kind, Kind::Read) || address(header).is_err() {
            return Err(ReadIssue::Io);
        }
        if !self.admission.writes_qualified() || !self.sequence.idle() {
            return Err(ReadIssue::Io);
        }
        if self.sequence.read_reg(0x4002_d030) & 1 != 0
            || self.sequence.read_reg(0x4002_d050) & 3 != 0
            || self.sequence.read_reg(0x400b_0030) & 4 != 0
        {
            self.last_error = Some(self.capture_error());
            return Err(ReadIssue::Io);
        }
        let mut bytes = [0; 256];
        let start = header.slot.base() as usize + header.offset as usize;
        for index in 0..usize::from(header.len) {
            let address = start + index;
            let word = self.sequence.read_reg(address & !3);
            bytes[index] = word.to_le_bytes()[address & 3];
            if self.sequence.read_reg(0x4002_d050) & 3 != 0 {
                self.last_error = Some(self.capture_error());
                return Err(ReadIssue::UncorrectableEcc);
            }
            if self.sequence.read_reg(0x4002_d030) & 1 != 0
                || self.sequence.read_reg(0x400b_0030) & 4 != 0
            {
                self.last_error = Some(self.capture_error());
                return Err(ReadIssue::CorrectedEcc);
            }
        }
        Ok(bytes)
    }
    /// Complete fixed slot classification input; no caller address.
    pub fn read_slot(&mut self, slot: Slot) -> Result<[u8; SLOT_BYTES as usize], ReadIssue> {
        let mut bytes = [0; SLOT_BYTES as usize];
        for chunk in 0..(SLOT_BYTES as usize / 256) {
            let header = Header {
                io_epoch: 1,
                command_sequence: chunk as u64 + 1,
                slot,
                kind: Kind::Read,
                offset: (chunk * 256) as u32,
                len: 256,
            };
            let read = self.read_chunk(header)?;
            bytes[chunk * 256..chunk * 256 + 256].copy_from_slice(&read);
        }
        Ok(bytes)
    }
    /// Capture the documented read-only factory, FRI and controller fields.
    /// This never touches MAIN data or changes controller state.
    pub fn observe(&mut self) -> Result<FlashObservation, Error> {
        // TRM15.3.2.42: an unassigned controller is inaccessible, including STATCMD.
        // Hold the semaphore with this unique controller until command cleanup/reset.
        self.sequence.acquire_once()?;
        Ok(FlashObservation {
            factory_sramflash: self.sequence.read_reg(0x6011_1074),
            fri_read_control: self.sequence.read_reg(0x4002_9000),
            fri_interface_control: self.sequence.read_reg(0x4002_900c),
            command_status: self.sequence.read_reg(0x4004_33d0),
            command_we_protect_a: self.sequence.read_reg(0x4004_31d0),
            command_we_protect_b: self.sequence.read_reg(0x4004_31d4),
            protect_nonmain: self.sequence.read_reg(0x4004_3210),
        })
    }
}

/// One bounded command at a time. The production implementation denies all
/// operations before touching its private register backend.
pub trait Controller {
    /// Non-MMIO admission before the owner records a physical issue. A
    /// durable debit made here is never refunded after a later issue fault.
    fn preflight(&mut self, _command: &Command) -> Result<(), Error> {
        Ok(())
    }
    /// An error may follow physical execution. The owner retains custody and
    /// fences every error; implementations must never retry inside `issue`.
    fn issue(
        &mut self,
        command: &Command,
        address: Option<u32>,
        deadline: u64,
        now: u64,
    ) -> Result<(), Error>;
    /// `Done` reports the exact issued header; the owner rejects mismatches.
    /// It is a status observation, not by itself idle/drain evidence. A
    /// kind-valid terminal `Ok`, `Read` (including a read issue),
    /// `WearGranted`/`WearDenied`, or `Failed { quiescent: true }` certifies
    /// completion and controller idle/drain for this command.
    /// `Failed { quiescent: false }`, `Busy` and `Fault` certify no such thing.
    fn poll(&mut self, now: u64) -> ControllerState;
    /// Restores and verifies protection; success alone never certifies idle.
    fn restore_protection(&mut self) -> Result<(), Error>;
}
/// A live monotonic tick source supplied by the reviewed composition. Its rate
/// and IRQ progress must be qualified with the eventual controller deadline.
pub trait Clock {
    fn now(&mut self) -> u64;
}
// The fixed inline read buffer must remain owned without a post-freeze heap path.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerState {
    Busy,
    Done(Header, Status, [u8; 256]),
    Fault,
}
impl<I: RegisterIo> Controller for FlashController<I> {
    fn issue(
        &mut self,
        command: &Command,
        mapped: Option<u32>,
        deadline: u64,
        now: u64,
    ) -> Result<(), Error> {
        if !self.admission.writes_qualified() {
            return Err(Error::Unqualified);
        }
        if deadline
            .checked_sub(now)
            .is_none_or(|duration| duration == 0 || duration > self.max_command_duration_us)
        {
            return Err(Error::Unqualified);
        }
        if mapped != address(command.header)? {
            return Err(Error::InvalidRange);
        }
        self.terminal = None;
        self.sequence.begin(command, deadline, now)
    }
    fn poll(&mut self, now: u64) -> ControllerState {
        if !self.admission.writes_qualified() {
            return ControllerState::Fault;
        }
        if let Some(terminal) = self.terminal {
            return terminal;
        }
        // The fixture report must separately establish that completed
        // commands return both banks to READ and drain outstanding reads.
        let state = match self.sequence.step(now, true) {
            registers::Step::Busy => ControllerState::Busy,
            registers::Step::Done(header, true) => {
                ControllerState::Done(header, Status::Ok, [0; 256])
            }
            registers::Step::Done(header, false) => {
                ControllerState::Done(header, Status::Failed { quiescent: true }, [0; 256])
            }
            registers::Step::Fault => ControllerState::Fault,
        };
        if matches!(
            state,
            ControllerState::Fault | ControllerState::Done(_, Status::Failed { .. }, _)
        ) {
            self.last_error = Some(self.capture_error());
        }
        if matches!(state, ControllerState::Done(..)) {
            self.terminal = Some(state);
        }
        state
    }
    fn restore_protection(&mut self) -> Result<(), Error> {
        if !self.admission.writes_qualified() {
            return Err(Error::Unqualified);
        }
        if !self.sequence.idle()
            || self.sequence.read_reg(0x4004_31d0) != u32::MAX
            || self.sequence.read_reg(0x4004_31d4) & 0xfff != 0xfff
            || self.sequence.read_reg(0x4004_3210) & 3 != 3
            || self.sequence.read_reg(0x4004_3104) != 0
        {
            return Err(Error::Uncertain);
        }
        Ok(())
    }
}

/// Fixed command, retained completion and independent capacity-one quiesce slot.
/// `deadline` is an absolute tick from the same live clock as `poll(now)`; this
/// module never invents a hardware duration or converts a loop count to time.
pub struct FlashOwner<C: Controller> {
    controller: C,
    epoch: u64,
    last_sequence: u64,
    active: Option<(Command, u64)>,
    retained: Option<Completion>,
    last_command: Option<Command>,
    quiesce: Option<QuiesceRequest>,
    quiesce_settled: bool,
    late_settled: bool,
    uncertain: bool,
}
impl<C: Controller> FlashOwner<C> {
    pub fn new(controller: C, epoch: u64) -> Self {
        Self {
            controller,
            epoch,
            last_sequence: 0,
            active: None,
            retained: None,
            last_command: None,
            quiesce: None,
            quiesce_settled: false,
            late_settled: false,
            uncertain: false,
        }
    }
    pub fn submit(
        &mut self,
        command: Command,
        deadline: u64,
        now: u64,
        qualification: Qualification,
        active_slot: Slot,
    ) -> Result<(), Error> {
        if let Some(completion) = &self.retained {
            if completion.header == command.header && self.last_command.as_ref() == Some(&command) {
                return if self.uncertain {
                    Err(Error::Uncertain)
                } else {
                    Ok(())
                };
            }
            return if completion.header == command.header {
                Err(Error::Conflict)
            } else {
                Err(Error::Busy)
            };
        }
        if let Some((previous, _)) = &self.active {
            return if previous == &command {
                if self.uncertain {
                    Err(Error::Uncertain)
                } else {
                    Ok(())
                }
            } else if previous.header == command.header {
                Err(Error::Conflict)
            } else {
                Err(Error::Busy)
            };
        }
        if self.uncertain || self.quiesce.is_some() {
            return Err(Error::Uncertain);
        }
        let h = command.header;
        if h.io_epoch != self.epoch || h.command_sequence <= self.last_sequence {
            return Err(Error::Stale);
        }
        if h.command_sequence == u64::MAX {
            return Err(Error::Exhausted);
        }
        if now >= deadline {
            return Err(Error::Uncertain);
        }
        let mapped = address(h)?;
        if !qualification.writes_qualified() {
            return Err(Error::Unqualified);
        }
        if h.slot == active_slot && !matches!(h.kind, Kind::Read) {
            return Err(Error::SelectedSlot);
        }
        if let Err(error) = self.controller.preflight(&command) {
            if error == Error::Uncertain {
                self.last_sequence = h.command_sequence;
                self.last_command = Some(command);
                self.uncertain = true;
                self.retained = Some(Completion {
                    header: h,
                    status: Status::Failed { quiescent: false },
                    data: [0; 256],
                });
            }
            return Err(error);
        }
        self.last_sequence = h.command_sequence;
        self.last_command = Some(command.clone());
        self.active = Some((command.clone(), deadline));
        // `issue` can execute and still return an error. Custody must exist
        // before that call, and an error cannot release or debit it twice.
        if let Err(error) = self.controller.issue(&command, mapped, deadline, now) {
            self.uncertain = true;
            self.retained = Some(Completion {
                header: h,
                status: Status::Failed { quiescent: false },
                data: [0; 256],
            });
            return Err(error);
        }
        Ok(())
    }
    pub fn poll(&mut self, clock: &mut impl Clock) {
        self.poll_at(clock.now());
    }
    fn poll_at(&mut self, now: u64) {
        let Some((command, deadline)) = self.active.as_ref() else {
            return;
        };
        let h = command.header;
        let state = self.controller.poll(now);
        if state == ControllerState::Fault
            || (now >= *deadline && !matches!(state, ControllerState::Done(..)))
        {
            self.uncertain = true;
            if self.retained.is_none() {
                self.retained = Some(Completion {
                    header: h,
                    status: Status::Failed { quiescent: false },
                    data: [0; 256],
                });
            }
            return;
        }
        match state {
            ControllerState::Busy => {}
            ControllerState::Done(done_header, status, data) => {
                let valid = done_header == h
                    && match (h.kind, status) {
                        (Kind::Read, Status::Read { len, .. }) => len == h.len,
                        (Kind::ConsumeErasePermit { .. }, Status::WearGranted { permit }) => {
                            permit != 0
                        }
                        (Kind::ConsumeErasePermit { .. }, Status::WearDenied) => true,
                        (Kind::Erase { .. } | Kind::Program, Status::Ok) => true,
                        (_, Status::Failed { quiescent }) => quiescent,
                        _ => false,
                    };
                let protection_restored = self.controller.restore_protection().is_ok();
                if !valid || !protection_restored {
                    self.uncertain = true;
                    self.retained = Some(Completion {
                        header: h,
                        status: Status::Failed { quiescent: false },
                        data: [0; 256],
                    });
                    return;
                }
                if self.uncertain || now >= *deadline {
                    // A late qualified completion proves idle/drain, but the
                    // original operation already failed its deadline. Only a
                    // matching quiesce can clear that uncertainty.
                    self.active = None;
                    self.late_settled = true;
                    self.retained = Some(Completion {
                        header: h,
                        status: Status::Failed { quiescent: true },
                        data: [0; 256],
                    });
                    self.uncertain = true;
                    if self.quiesce.map(|q| q.outstanding) == Some(h) {
                        self.quiesce_settled = true;
                        self.uncertain = false;
                    }
                    return;
                }
                self.retained = Some(Completion {
                    header: h,
                    status,
                    data,
                });
                self.active = None;
                if self.quiesce.map(|q| q.outstanding) == Some(h) {
                    self.quiesce_settled = true;
                }
            }
            ControllerState::Fault => {}
        }
    }
    pub fn request_quiesce(&mut self, request: QuiesceRequest) -> Result<(), Error> {
        let key = self
            .active
            .as_ref()
            .map(|a| a.0.header)
            .or_else(|| self.retained.as_ref().map(|r| r.header));
        if key != Some(request.outstanding) {
            return Err(Error::Conflict);
        }
        if let Some(previous) = self.quiesce {
            return if previous == request {
                Ok(())
            } else {
                Err(Error::Busy)
            };
        }
        self.quiesce = Some(request);
        self.quiesce_settled = self.active.is_none() && (!self.uncertain || self.late_settled);
        if self.quiesce_settled {
            self.uncertain = false;
        }
        Ok(())
    }
    pub fn quiesce_settled(&self) -> bool {
        self.quiesce_settled
    }
    pub fn completion(&self) -> Option<&Completion> {
        self.retained.as_ref()
    }
    pub fn acknowledge(&mut self, header: Header) -> Result<Completion, Error> {
        if self.active.is_some()
            || self.uncertain
            || self.quiesce.is_some() && !self.quiesce_settled
        {
            return Err(Error::Uncertain);
        }
        if self.retained.as_ref().map(|r| r.header) != Some(header) {
            return Err(Error::NoCompletion);
        }
        self.quiesce = None;
        self.quiesce_settled = false;
        self.late_settled = false;
        self.last_command = None;
        self.retained.take().ok_or(Error::NoCompletion)
    }
    /// Logical lifecycle reset never reacquires a handle or clears uncertainty.
    pub fn logical_reset(&mut self) {}
    pub fn fenced(&self) -> bool {
        self.uncertain
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::cell::Cell;
    use std::rc::Rc;
    struct TestClock(u64);
    impl Clock for TestClock {
        fn now(&mut self) -> u64 {
            self.0
        }
    }
    struct Model {
        issues: Rc<Cell<u32>>,
        restores: Rc<Cell<u32>>,
        state: ControllerState,
        restore_ok: bool,
    }
    impl Controller for Model {
        fn issue(&mut self, _: &Command, _: Option<u32>, _: u64, _: u64) -> Result<(), Error> {
            self.issues.set(self.issues.get() + 1);
            Ok(())
        }
        fn poll(&mut self, _: u64) -> ControllerState {
            self.state
        }
        fn restore_protection(&mut self) -> Result<(), Error> {
            self.restores.set(self.restores.get() + 1);
            if self.restore_ok {
                Ok(())
            } else {
                Err(Error::Uncertain)
            }
        }
    }
    fn model(state: ControllerState) -> (Model, Rc<Cell<u32>>, Rc<Cell<u32>>) {
        let issues = Rc::new(Cell::new(0));
        let restores = Rc::new(Cell::new(0));
        (
            Model {
                issues: issues.clone(),
                restores: restores.clone(),
                state,
                restore_ok: true,
            },
            issues,
            restores,
        )
    }
    fn cmd(kind: Kind, slot: Slot, offset: u32, len: u16, sequence: u64) -> Command {
        Command {
            header: Header {
                io_epoch: 7,
                command_sequence: sequence,
                slot,
                kind,
                offset,
                len,
            },
            data: [0xa5; 256],
        }
    }
    fn full() -> Qualification {
        Qualification::test_with_missing(0)
    }
    #[test]
    fn bounded_owner_storage() {
        assert!(core::mem::size_of::<FlashOwner<Model>>() <= 2048);
    }
    #[test]
    fn every_valid_and_invalid_interval() {
        for slot in [Slot::A, Slot::B] {
            let base = slot.base();
            for offset in 0..SLOT_BYTES {
                for len in [1u16, 16, 255, 256] {
                    let h = cmd(Kind::Read, slot, offset, len, 1).header;
                    let expected = offset + u32::from(len) <= SLOT_BYTES;
                    assert_eq!(address(h).ok(), expected.then_some(Some(base + offset)));
                }
            }
            for offset in [0, 256, 512, 768] {
                assert_eq!(
                    address(cmd(Kind::Program, slot, offset, 256, 1).header),
                    Ok(Some(base + offset))
                );
            }
            for offset in [1, 16, 640, 769, 1024, 2048, u32::MAX] {
                assert!(address(cmd(Kind::Program, slot, offset, 256, 1).header).is_err());
            }
            assert_eq!(
                address(cmd(Kind::Erase { permit: 1 }, slot, 0, 0, 1).header),
                Ok(Some(base))
            );
            assert_eq!(
                address(
                    cmd(
                        Kind::ConsumeErasePermit {
                            lease: 1,
                            issuance: 1
                        },
                        slot,
                        0,
                        0,
                        1
                    )
                    .header
                ),
                Ok(None)
            );
            for (kind, off, len) in [
                (Kind::Read, 0, 0),
                (Kind::Read, 2048, 1),
                (Kind::Read, u32::MAX, 256),
                (Kind::Program, 0, 16),
                (Kind::Erase { permit: 0 }, 0, 0),
                (Kind::Erase { permit: 1 }, 1, 0),
                (Kind::Erase { permit: 1 }, 0, 1),
                (
                    Kind::ConsumeErasePermit {
                        lease: 0,
                        issuance: 1,
                    },
                    0,
                    0,
                ),
                (
                    Kind::ConsumeErasePermit {
                        lease: 1,
                        issuance: 0,
                    },
                    0,
                    0,
                ),
                (
                    Kind::ConsumeErasePermit {
                        lease: 1,
                        issuance: 1,
                    },
                    1,
                    0,
                ),
            ] {
                assert!(address(cmd(kind, slot, off, len, 1).header).is_err());
            }
        }
        assert_eq!(Slot::A.end(), 0x78800);
        assert_eq!(Slot::B.end(), 0x7c800);
        assert!(Slot::A.end() < Slot::B.base());
        assert!(Slot::B.end() < MAIN_END);
    }
    #[test]
    fn each_absent_predicate_denies_before_issue() {
        let (m, issues, _) = model(ControllerState::Busy);
        let mut owner = FlashOwner::new(m, 7);
        let c = cmd(Kind::Program, Slot::B, 0, 256, 1);
        for bit in 0..9 {
            assert_eq!(
                owner.submit(
                    c.clone(),
                    100,
                    1,
                    Qualification::test_with_missing(1 << bit),
                    Slot::A
                ),
                Err(Error::Unqualified)
            );
        }
        assert_eq!(
            owner.submit(c.clone(), 100, 1, full(), Slot::B),
            Err(Error::SelectedSlot)
        );
        assert_eq!(
            owner.submit(c.clone(), 1, 1, full(), Slot::A),
            Err(Error::Uncertain)
        );
        let mut stale = c.clone();
        stale.header.io_epoch = 6;
        assert_eq!(
            owner.submit(stale, 100, 1, full(), Slot::A),
            Err(Error::Stale)
        );
        assert_eq!(issues.get(), 0);
    }
    #[test]
    fn authority_preflight_denial_and_uncertainty_never_issue_media() {
        struct Gate {
            result: Error,
            calls: Rc<Cell<u32>>,
        }
        impl Controller for Gate {
            fn preflight(&mut self, _: &Command) -> Result<(), Error> {
                Err(self.result)
            }
            fn issue(&mut self, _: &Command, _: Option<u32>, _: u64, _: u64) -> Result<(), Error> {
                self.calls.set(self.calls.get() + 1);
                Ok(())
            }
            fn poll(&mut self, _: u64) -> ControllerState {
                ControllerState::Fault
            }
            fn restore_protection(&mut self) -> Result<(), Error> {
                Err(Error::Uncertain)
            }
        }
        let calls = Rc::new(Cell::new(0));
        let command = cmd(Kind::Erase { permit: 1 }, Slot::B, 0, 0, 1);
        let mut denied = FlashOwner::new(
            Gate {
                result: Error::NoAuthority,
                calls: calls.clone(),
            },
            7,
        );
        assert_eq!(
            denied.submit(command.clone(), 10, 1, full(), Slot::A),
            Err(Error::NoAuthority)
        );
        assert!(denied.completion().is_none());
        let mut uncertain = FlashOwner::new(
            Gate {
                result: Error::Uncertain,
                calls: calls.clone(),
            },
            7,
        );
        assert_eq!(
            uncertain.submit(command.clone(), 10, 1, full(), Slot::A),
            Err(Error::Uncertain)
        );
        assert!(uncertain.fenced());
        assert_eq!(uncertain.completion().unwrap().header, command.header);
        assert_eq!(
            uncertain.submit(command, 20, 2, full(), Slot::A),
            Err(Error::Uncertain)
        );
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn duplicate_late_quiesce_and_reset_retain_custody() {
        let (m, issues, restores) = model(ControllerState::Busy);
        let mut owner = FlashOwner::new(m, 7);
        let c = cmd(Kind::Program, Slot::B, 0, 256, 1);
        let h = c.header;
        owner.submit(c.clone(), 10, 1, full(), Slot::A).unwrap();
        assert_eq!(owner.submit(c.clone(), 10, 2, full(), Slot::A), Ok(()));
        let mut changed = c.clone();
        changed.data[0] = 0;
        assert_eq!(
            owner.submit(changed, 10, 2, full(), Slot::A),
            Err(Error::Conflict)
        );
        owner.poll(&mut TestClock(10));
        assert!(owner.fenced());
        assert_eq!(issues.get(), 1);
        assert_eq!(
            owner.completion().unwrap().status,
            Status::Failed { quiescent: false }
        );
        owner.logical_reset();
        assert!(owner.fenced());
        assert_eq!(owner.acknowledge(h), Err(Error::Uncertain));
        assert_eq!(
            owner.request_quiesce(QuiesceRequest {
                outstanding: Header {
                    command_sequence: 2,
                    ..h
                }
            }),
            Err(Error::Conflict)
        );
        owner
            .request_quiesce(QuiesceRequest { outstanding: h })
            .unwrap();
        assert!(!owner.quiesce_settled());
        owner.controller.state = ControllerState::Done(h, Status::Ok, [0; 256]);
        owner.poll(&mut TestClock(11));
        assert!(owner.quiesce_settled());
        assert!(!owner.fenced());
        assert_eq!(
            owner.completion().unwrap().status,
            Status::Failed { quiescent: true }
        );
        assert_eq!(restores.get(), 1);
        assert_eq!(owner.acknowledge(h).unwrap().header, h);
        assert_eq!(owner.submit(c, 100, 12, full(), Slot::A), Err(Error::Stale));
        assert_eq!(issues.get(), 1);
    }
    #[test]
    fn cleanup_failure_fences_without_reissue() {
        let (mut m, issues, restores) = model(ControllerState::Busy);
        m.restore_ok = false;
        let mut owner = FlashOwner::new(m, 7);
        let c = cmd(Kind::Erase { permit: 4 }, Slot::B, 0, 0, 1);
        owner.submit(c.clone(), 100, 1, full(), Slot::A).unwrap();
        owner.controller.state = ControllerState::Done(c.header, Status::Ok, [0; 256]);
        owner.poll(&mut TestClock(2));
        assert!(owner.fenced());
        assert_eq!(
            owner.completion().unwrap().status,
            Status::Failed { quiescent: false }
        );
        assert_eq!(
            owner.submit(
                cmd(Kind::Program, Slot::B, 0, 256, 2),
                100,
                3,
                full(),
                Slot::A
            ),
            Err(Error::Busy)
        );
        assert_eq!(issues.get(), 1);
        assert_eq!(restores.get(), 1);
    }
}

#[cfg(test)]
mod more_tests {
    extern crate std;
    use super::*;
    use core::cell::Cell;
    use std::rc::Rc;
    #[derive(Clone)]
    struct CountingIo(Rc<Cell<u32>>);
    impl RegisterIo for CountingIo {
        fn read(&self, _: usize) -> u32 {
            self.0.set(self.0.get() + 1);
            0
        }
        fn write(&self, _: usize, _: u32) {
            self.0.set(self.0.get() + 1);
        }
        fn delay_cycles(&self, _: u32) {
            self.0.set(self.0.get() + 1);
        }
    }
    #[test]
    fn production_controller_never_touches_mmio() {
        let calls = Rc::new(Cell::new(0));
        let mut controller = FlashController::new(CountingIo(calls.clone()));
        for kind in [
            Kind::Read,
            Kind::Erase { permit: 1 },
            Kind::Program,
            Kind::ConsumeErasePermit {
                lease: 1,
                issuance: 1,
            },
        ] {
            let c = Command {
                header: Header {
                    io_epoch: 1,
                    command_sequence: 1,
                    slot: Slot::B,
                    kind,
                    offset: 0,
                    len: match kind {
                        Kind::Read => 1,
                        Kind::Program => 256,
                        _ => 0,
                    },
                },
                data: [0; 256],
            };
            assert_eq!(
                controller.issue(&c, address(c.header).unwrap(), 10, 0),
                Err(Error::Unqualified)
            );
            assert_eq!(controller.poll(0), ControllerState::Fault);
            assert_eq!(controller.restore_protection(), Err(Error::Unqualified));
        }
        assert_eq!(calls.get(), 0);
        assert!(!Qualification::unqualified().writes_qualified());
    }
}

#[cfg(test)]
mod issue_boundary_regressions {
    use super::*;
    use core::cell::Cell;

    struct Adversarial<'a> {
        issues: &'a Cell<u32>,
        restores: &'a Cell<u32>,
        error_after_issue: bool,
        restore_ok: bool,
        state: ControllerState,
    }
    impl Controller for Adversarial<'_> {
        fn issue(&mut self, _: &Command, _: Option<u32>, _: u64, _: u64) -> Result<(), Error> {
            self.issues.set(self.issues.get() + 1);
            if self.error_after_issue {
                Err(Error::Uncertain)
            } else {
                Ok(())
            }
        }
        fn poll(&mut self, _: u64) -> ControllerState {
            self.state
        }
        fn restore_protection(&mut self) -> Result<(), Error> {
            self.restores.set(self.restores.get() + 1);
            if self.restore_ok {
                Ok(())
            } else {
                Err(Error::Uncertain)
            }
        }
    }
    fn erase(sequence: u64) -> Command {
        Command {
            header: Header {
                io_epoch: 7,
                command_sequence: sequence,
                slot: Slot::B,
                kind: Kind::Erase { permit: 9 },
                offset: 0,
                len: 0,
            },
            data: [0; 256],
        }
    }
    fn owner<'a>(issues: &'a Cell<u32>, restores: &'a Cell<u32>) -> FlashOwner<Adversarial<'a>> {
        FlashOwner::new(
            Adversarial {
                issues,
                restores,
                error_after_issue: false,
                restore_ok: true,
                state: ControllerState::Busy,
            },
            7,
        )
    }
    fn submit(
        owner: &mut FlashOwner<Adversarial<'_>>,
        command: Command,
        now: u64,
    ) -> Result<(), Error> {
        owner.submit(
            command,
            10,
            now,
            Qualification::test_with_missing(0),
            Slot::A,
        )
    }

    #[test]
    fn error_after_execute_retains_exact_custody_and_requires_matching_quiesce() {
        let issues = Cell::new(0);
        let restores = Cell::new(0);
        let mut owner = owner(&issues, &restores);
        owner.controller.error_after_issue = true;
        let c = erase(1);
        let h = c.header;
        assert_eq!(submit(&mut owner, c.clone(), 1), Err(Error::Uncertain));
        assert_eq!(issues.get(), 1);
        assert!(owner.fenced());
        assert_eq!(
            owner.completion().unwrap().status,
            Status::Failed { quiescent: false }
        );
        owner.logical_reset();
        assert_eq!(submit(&mut owner, c.clone(), 2), Err(Error::Uncertain));
        let mut changed = c.clone();
        changed.data[0] = 1;
        assert_eq!(submit(&mut owner, changed, 2), Err(Error::Conflict));
        assert_eq!(submit(&mut owner, erase(2), 2), Err(Error::Busy));
        assert_eq!(issues.get(), 1);
        assert_eq!(
            owner.request_quiesce(QuiesceRequest {
                outstanding: erase(2).header
            }),
            Err(Error::Conflict)
        );
        owner
            .request_quiesce(QuiesceRequest { outstanding: h })
            .unwrap();
        assert!(!owner.quiesce_settled());
        owner.controller.state = ControllerState::Done(erase(2).header, Status::Ok, [0; 256]);
        owner.poll_at(3);
        assert!(owner.fenced());
        assert!(!owner.quiesce_settled());
        owner.controller.state = ControllerState::Done(h, Status::Ok, [0; 256]);
        owner.poll_at(4);
        assert!(owner.quiesce_settled());
        assert!(!owner.fenced());
        assert_eq!(
            owner.completion().unwrap().status,
            Status::Failed { quiescent: true }
        );
        assert_eq!(owner.acknowledge(h).unwrap().header, h);
        assert_eq!(submit(&mut owner, c, 4), Err(Error::Stale));
        assert_eq!(issues.get(), 1);
        assert_eq!(restores.get(), 2);
    }

    #[test]
    fn late_nonquiescent_done_and_cleanup_success_cannot_clear_fence() {
        let issues = Cell::new(0);
        let restores = Cell::new(0);
        let mut owner = owner(&issues, &restores);
        let c = erase(1);
        let h = c.header;
        submit(&mut owner, c.clone(), 1).unwrap();
        owner.poll_at(10);
        owner
            .request_quiesce(QuiesceRequest { outstanding: h })
            .unwrap();
        owner.controller.state =
            ControllerState::Done(h, Status::Failed { quiescent: false }, [0; 256]);
        owner.poll_at(11);
        assert_eq!(
            owner.completion().unwrap().status,
            Status::Failed { quiescent: false }
        );
        assert!(owner.fenced());
        assert!(!owner.quiesce_settled());
        assert_eq!(owner.acknowledge(h), Err(Error::Uncertain));
        assert_eq!(submit(&mut owner, c, 12), Err(Error::Uncertain));
        assert_eq!(issues.get(), 1);
        assert_eq!(restores.get(), 1);
        owner.controller.state =
            ControllerState::Done(h, Status::Failed { quiescent: true }, [0; 256]);
        owner.poll_at(12);
        assert!(owner.quiesce_settled());
        assert_eq!(
            owner.acknowledge(h).unwrap().status,
            Status::Failed { quiescent: true }
        );
    }

    #[test]
    fn cleanup_failure_keeps_fence_until_qualified_status_and_cleanup() {
        let issues = Cell::new(0);
        let restores = Cell::new(0);
        let mut owner = owner(&issues, &restores);
        let c = erase(1);
        let h = c.header;
        submit(&mut owner, c, 1).unwrap();
        owner.controller.restore_ok = false;
        owner.controller.state = ControllerState::Done(h, Status::Ok, [0; 256]);
        owner.poll_at(2);
        owner
            .request_quiesce(QuiesceRequest { outstanding: h })
            .unwrap();
        assert!(owner.fenced());
        assert!(!owner.quiesce_settled());
        owner.controller.restore_ok = true;
        owner.controller.state =
            ControllerState::Done(h, Status::Failed { quiescent: false }, [0; 256]);
        owner.poll_at(3);
        assert!(owner.fenced());
        assert!(!owner.quiesce_settled());
        owner.controller.state = ControllerState::Done(h, Status::Ok, [0; 256]);
        owner.poll_at(4);
        assert_eq!(
            owner.acknowledge(h).unwrap().status,
            Status::Failed { quiescent: true }
        );
        assert_eq!(issues.get(), 1);
        assert_eq!(restores.get(), 3);
    }

    #[test]
    fn healthy_completion_and_late_completion_wait_for_proof() {
        let issues = Cell::new(0);
        let restores = Cell::new(0);
        let mut owner = owner(&issues, &restores);
        let c = erase(1);
        let h = c.header;
        submit(&mut owner, c, 1).unwrap();
        owner.controller.state = ControllerState::Done(h, Status::Ok, [0; 256]);
        owner.poll_at(2);
        assert!(!owner.fenced());
        assert_eq!(owner.acknowledge(h).unwrap().status, Status::Ok);
        let next = erase(2);
        let h2 = next.header;
        submit(&mut owner, next, 3).unwrap();
        owner.controller.state = ControllerState::Busy;
        owner.poll_at(10);
        assert!(owner.fenced());
        owner.controller.state = ControllerState::Done(h2, Status::Ok, [0; 256]);
        owner.poll_at(11);
        assert!(owner.fenced());
        assert_eq!(owner.acknowledge(h2), Err(Error::Uncertain));
        owner
            .request_quiesce(QuiesceRequest { outstanding: h2 })
            .unwrap();
        assert!(owner.quiesce_settled());
        assert_eq!(
            owner.acknowledge(h2).unwrap().status,
            Status::Failed { quiescent: true }
        );
        assert_eq!(issues.get(), 2);
        assert_eq!(restores.get(), 2);
    }
}

/// Explicit one-boot experiment; consumes this controller, leaving production admission closed.
impl<I: RegisterIo> FlashController<I> {
    pub fn into_diagnostic(self) -> registers::Diagnostic<I> { registers::Diagnostic::new(self.sequence) }
}
pub use registers::{Diagnostic, DiagnosticResult, DIAGNOSTIC_PATTERN};
