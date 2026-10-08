//! Private NVMNW command sequencer. Admission is owned by `FlashController`.
//!
//! Offsets/bit values: TI SDK 26.01.00.03 hw_nvmnw.h, hw_gsc.h, hw_memmap.h
//! (hashes in spec/flash-design/primary-evidence.md and RESULT.md). The SDK's
//! bank-local protection calculation predicts bit 10/11 for candidate A/B;
//! TRM 13.4 describes a conflicting unit. These encodings are conditional.
//! NVMNW command base 0x40042000 is distinct from FLASH FRI 0x40028000.
//! Status bits alone do not prove banks returned to READ or drained bus reads.
use super::{Command, Error, Header, Kind, Slot, address};
use crate::io::RegisterIo;

const NVMNW: usize = 0x4004_2000;
const GSC: usize = 0x4004_6000;
const SEM_REQ: usize = GSC + 0x1800;
const SEM_CLR: usize = GSC + 0x1804;
const SEM_STAT: usize = GSC + 0x1808;
const EXEC: usize = NVMNW + 0x1100;
const TYPE: usize = NVMNW + 0x1104;
const CTL: usize = NVMNW + 0x1108;
const ADDR: usize = NVMNW + 0x1120;
const BYTE_EN: usize = NVMNW + 0x1124;
const DATA_INDEX: usize = NVMNW + 0x112c;
const DATA0: usize = NVMNW + 0x1130;
const PROTECT_A: usize = NVMNW + 0x11d0;
const PROTECT_B: usize = NVMNW + 0x11d4;
const PROTECT_NM: usize = NVMNW + 0x1210;
const STAT: usize = NVMNW + 0x13d0;
const DONE: u32 = 1;
const PASS: u32 = 2;
const IN_PROGRESS: u32 = 4;
const FAILURES: u32 = 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x1000;
const SEM_OWNED: u32 = 0xc000_0000;
const ERASE_SECTOR: u32 = 0x42;
const PROGRAM_WORD: u32 = 1;
const CLEAR_STATUS: u32 = 5;
const FULL_WORD_ECC: u32 = 0x3ffff;

// A/B are bank1 sectors 112/120. SDK hw_nvmnw.h CMDWEPROTB says
// eight-sector groups; dl_flashctl.c uses (sector_in_bank/8)-4.
const fn group_mask(slot: Slot) -> u32 {
    match slot {
        Slot::A => 1 << 10,
        Slot::B => 1 << 11,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Observe {
    Busy,
    StaleDone,
    Failed(u32),
    Passed,
}
pub(super) enum Step {
    Busy,
    Done(Header, bool),
    Fault,
}
fn classify(status: u32, progress_seen: bool) -> Observe {
    if status & FAILURES != 0 {
        return Observe::Failed(status);
    }
    if status & IN_PROGRESS != 0 {
        return Observe::Busy;
    }
    // An old DONE may survive command launch. Requiring an observed active
    // transition can fail closed for a very fast operation; it cannot invent
    // a fresh terminal from the old status.
    if !progress_seen {
        return Observe::StaleDone;
    }
    if status & DONE == 0 {
        return Observe::Busy;
    }
    if status & PASS == 0 {
        return Observe::Failed(status);
    }
    Observe::Passed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Idle,
    Clearing,
    Command,
    Fault,
}

// No `pub` constructor: only tests can create this model. A later qualified
// controller needs a fresh review of mode, read/ECC, timing, SRAM and wear gates.
pub(super) struct Sequence<I: RegisterIo> {
    io: I,
    phase: Phase,
    // Active header, then the last consumed identity after cleanup. Keeping
    // this fixed slot prevents a retired command from being admitted again.
    header: Option<Header>,
    deadline: u64,
    issued: bool,
    progress_seen: bool,
    sem_owned: bool,
    next_word: u8,
    payload: [u8; 256],
    diagnostics: u32,
    terminal_observed: bool,
}
impl<I: RegisterIo> Sequence<I> {
    pub(super) fn read_reg(&self, address: usize) -> u32 {
        self.io.read(address)
    }
    pub(super) fn new(io: I) -> Self {
        Self {
            io,
            phase: Phase::Idle,
            header: None,
            deadline: 0,
            issued: false,
            progress_seen: false,
            sem_owned: false,
            next_word: 0,
            payload: [0; 256],
            diagnostics: 0,
            terminal_observed: false,
        }
    }
    fn capture(&mut self) -> u32 {
        let raw = self.io.read(STAT);
        self.diagnostics |= raw;
        raw
    }
    pub(super) fn acquire_once(&mut self) -> Result<(), Error> {
        if self.sem_owned { return Ok(()); }
        self.io.write(SEM_REQ, 1);
        if self.io.read(SEM_STAT) & SEM_OWNED != SEM_OWNED {
            return Err(Error::Busy);
        }
        self.sem_owned = true;
        Ok(())
    }
    fn protect(&mut self, slot: Slot, allow_target: bool) -> Result<(), Error> {
        // This register recipe is not deployed: bank scoping/group size and
        // static protection need physical confirmation. All other groups and
        // NONMAIN remain protected under the conditional SDK interpretation.
        let b = if allow_target {
            0xfff & !group_mask(slot)
        } else {
            0xfff
        };
        self.io.write(PROTECT_A, u32::MAX);
        self.io.write(PROTECT_B, b);
        self.io.write(PROTECT_NM, 3);
        if self.io.read(PROTECT_A) != u32::MAX
            || self.io.read(PROTECT_B) & 0xfff != b
            || self.io.read(PROTECT_NM) & 3 != 3
        {
            return Err(Error::Uncertain);
        }
        Ok(())
    }
    fn launch(&mut self, kind: u32, addr: u32, data: Option<&[u8; 16]>) {
        // Initialize every used field. No address-translation/ECC override.
        self.io.write(TYPE, kind);
        self.io.write(CTL, 0);
        self.io.write(ADDR, addr);
        self.io
            .write(BYTE_EN, if data.is_some() { FULL_WORD_ECC } else { 0 });
        self.io.write(DATA_INDEX, (addr >> 4) & 3);
        for i in 0..4 {
            let word = data.map_or(0, |bytes| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            self.io.write(DATA0 + i * 4, word);
        }
        barrier();
        self.io.write(EXEC, 1);
        barrier();
        self.issued = true;
        self.progress_seen = false;
        self.terminal_observed = false;
    }
    pub(super) fn begin(
        &mut self,
        command: &Command,
        deadline: u64,
        now: u64,
    ) -> Result<(), Error> {
        if self.phase != Phase::Idle || now >= deadline {
            return Err(Error::Uncertain);
        }
        let h = command.header;
        // This standalone model has one epoch for its lifetime. A new epoch
        // needs a new qualified controller lifecycle; no register observation
        // can authorize it. Compare before any MMIO, including status capture.
        if let Some(consumed) = self.header {
            if h.io_epoch != consumed.io_epoch || h.command_sequence <= consumed.command_sequence {
                return Err(Error::Stale);
            }
        } else if h.command_sequence == 0 {
            return Err(Error::Stale);
        }
        if h.command_sequence == u64::MAX {
            return Err(Error::Exhausted);
        }
        let addr = address(h)?;
        if !matches!(h.kind, Kind::Erase { .. } | Kind::Program) {
            return Err(Error::InvalidKind);
        }
        let Some(physical) = addr else {
            return Err(Error::InvalidKind);
        };
        if let Err(error) = self.acquire_once() {
            self.header = Some(h);
            self.phase = Phase::Fault;
            return Err(error);
        }
        let pre = self.capture();
        if pre & IN_PROGRESS != 0 {
            return Err(Error::Busy);
        }
        self.header = Some(h);
        self.deadline = deadline;
        self.payload = command.data;
        self.next_word = 0;
        // Clear status has its own completion. It resets dynamic protections,
        // so target protection is configured only *after* it completes.
        self.launch(CLEAR_STATUS, 0, None);
        self.phase = Phase::Clearing;
        // Retain exact command address but do not launch it until a clear-status
        // terminal is observed and protection has been verified.
        let _ = physical;
        Ok(())
    }
    pub(super) fn poll(&mut self, now: u64) -> Observe {
        if self.phase == Phase::Idle {
            return Observe::StaleDone;
        }
        let raw = self.capture();
        if raw & IN_PROGRESS != 0 {
            self.progress_seen = true;
        }
        let observed = classify(raw, self.progress_seen && self.issued);
        if self.phase == Phase::Fault {
            // Late exact status observation can support a subsequent model
            // cleanup, but never upgrades the original timed-out result.
            if matches!(observed, Observe::Passed) {
                self.terminal_observed = true;
            }
            return Observe::Failed(raw);
        }
        if matches!(observed, Observe::Passed) {
            self.terminal_observed = true;
        }
        if now >= self.deadline || matches!(observed, Observe::Failed(_)) {
            self.phase = Phase::Fault;
            return Observe::Failed(raw);
        }
        observed
    }
    // The caller must have a reviewed, live all-banks-READ/drained predicate.
    // A terminal STATCMD word alone cannot provide it.
    pub(super) fn advance(&mut self, now: u64, banks_read: bool) -> Result<bool, Error> {
        if !banks_read || now >= self.deadline || !self.terminal_observed {
            return Err(Error::Uncertain);
        }
        let h = self.header.ok_or(Error::Uncertain)?;
        let base = address(h)?.ok_or(Error::InvalidKind)?;
        match self.phase {
            Phase::Clearing => {
                if let Err(error) = self.protect(h.slot, true) {
                    self.phase = Phase::Fault;
                    return Err(error);
                }
                self.phase = Phase::Command;
            }
            Phase::Command if matches!(h.kind, Kind::Program) => {
                self.next_word += 1;
                if self.next_word == 16 {
                    return Ok(true);
                }
            }
            Phase::Command if matches!(h.kind, Kind::Erase { .. }) => return Ok(true),
            _ => return Err(Error::Uncertain),
        }
        if matches!(h.kind, Kind::Erase { .. }) {
            self.launch(ERASE_SECTOR, base, None);
        } else {
            let start = usize::from(self.next_word) * 16;
            let data: [u8; 16] = self.payload[start..start + 16]
                .try_into()
                .map_err(|_| Error::InvalidRange)?;
            self.launch(
                PROGRAM_WORD,
                base + u32::from(self.next_word) * 16,
                Some(&data),
            );
        }
        Ok(false)
    }
    pub(super) fn cleanup(&mut self, banks_read: bool) -> Result<(), Error> {
        if !banks_read || !self.terminal_observed {
            return Err(Error::Uncertain);
        }
        let h = self.header.ok_or(Error::Uncertain)?;
        if self.phase == Phase::Clearing {
            return Err(Error::Uncertain);
        }
        if self.phase == Phase::Command && matches!(h.kind, Kind::Program) && self.next_word != 16 {
            return Err(Error::Uncertain);
        }
        self.io.write(TYPE, 0); // NOOP is a register value, not quiescence proof.
        self.protect(h.slot, false)?;
        if self.sem_owned {
            self.io.write(SEM_CLR, 1);
            if self.io.read(SEM_STAT) & 0x8000_0000 != 0 {
                return Err(Error::Uncertain);
            }
            self.sem_owned = false;
        }
        self.phase = Phase::Idle;
        // Retire, do not forget. The same slot is the sequence watermark for
        // the next admission, even when this cleanup followed a timeout.
        Ok(())
    }
    pub(super) fn late_cleanup(&mut self, header: Header, banks_read: bool) -> Result<(), Error> {
        if self.header != Some(header) || self.phase != Phase::Fault {
            return Err(Error::Conflict);
        }
        self.cleanup(banks_read)
    }
    pub(super) fn header(&self) -> Option<Header> {
        self.header
    }
    pub(super) fn idle(&self) -> bool {
        self.phase == Phase::Idle
    }
    pub(super) fn diagnostics(&self) -> u32 {
        self.diagnostics
    }
    /// One observed transition; never loop, retry an execute, or begin an
    /// additional word after the original absolute deadline.
    pub(super) fn step(&mut self, now: u64, banks_read: bool) -> Step {
        match self.poll(now) {
            Observe::Busy | Observe::StaleDone => Step::Busy,
            Observe::Passed => match self.advance(now, banks_read) {
                Ok(false) => Step::Busy,
                Ok(true) => {
                    let Some(header) = self.header else {
                        return Step::Fault;
                    };
                    if self.cleanup(banks_read).is_ok() {
                        Step::Done(header, true)
                    } else {
                        Step::Fault
                    }
                }
                Err(_) => Step::Fault,
            },
            Observe::Failed(_) => {
                let Some(header) = self.header else {
                    return Step::Fault;
                };
                if self.phase == Phase::Fault && self.late_cleanup(header, banks_read).is_ok() {
                    Step::Done(header, false)
                } else {
                    Step::Fault
                }
            }
        }
    }
}

#[inline(always)]
fn barrier() {
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("dsb sy", "isb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "arm"))]
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::{cell::RefCell, rc::Rc, vec::Vec};
    #[derive(Default)]
    struct State {
        regs: std::collections::BTreeMap<usize, u32>,
        writes: Vec<(usize, u32)>,
        bad_protect_read: bool,
    }
    #[derive(Clone, Default)]
    struct Io(Rc<RefCell<State>>);
    impl Io {
        fn set(&self, address: usize, value: u32) {
            self.0.borrow_mut().regs.insert(address, value);
        }
        fn writes(&self, address: usize) -> Vec<u32> {
            self.0
                .borrow()
                .writes
                .iter()
                .filter(|x| x.0 == address)
                .map(|x| x.1)
                .collect()
        }
    }
    impl RegisterIo for Io {
        fn read(&self, address: usize) -> u32 {
            let s = self.0.borrow();
            if address == PROTECT_B && s.bad_protect_read {
                return 0;
            }
            *s.regs.get(&address).unwrap_or(&0)
        }
        fn write(&self, address: usize, value: u32) {
            let mut s = self.0.borrow_mut();
            s.writes.push((address, value));
            s.regs.insert(address, value);
            if address == SEM_CLR {
                s.regs.insert(SEM_STAT, 0);
            }
        }
        fn delay_cycles(&self, _: u32) {}
    }
    fn command(slot: Slot, kind: Kind) -> Command {
        Command {
            header: Header {
                io_epoch: 3,
                command_sequence: 9,
                slot,
                kind,
                offset: 0,
                len: if matches!(kind, Kind::Program) {
                    256
                } else {
                    0
                },
            },
            data: core::array::from_fn(|i| i as u8),
        }
    }
    fn setup(kind: Kind) -> (Io, Sequence<Io>, Command) {
        let io = Io::default();
        io.set(SEM_STAT, SEM_OWNED);
        io.set(PROTECT_A, u32::MAX);
        io.set(PROTECT_B, 0xfff);
        io.set(PROTECT_NM, 3);
        let s = Sequence::new(io.clone());
        (io, s, command(Slot::A, kind))
    }
    #[test]
    fn production_controller_uses_exact_sequence_and_retires_header() {
        use super::super::{AdmissionEvidence, Controller, ControllerState, FlashController};
        let io = Io::default();
        io.set(0x6011_1074, 0x2200);
        io.set(0x4002_9000, 0x100);
        io.set(PROTECT_NM, 3);
        io.set(SEM_STAT, SEM_OWNED);
        let mut controller = FlashController::new(io.clone());
        let evidence = AdmissionEvidence {
            snapshot: controller.observe_safety().unwrap(),
            expected_static_bank1_a: 0,
            expected_static_bank1_b: 0,
            reports: [[1; 32]; 6],
            command_deadline_us: 100,
        };
        controller.activate(&evidence).unwrap();
        let command = command(Slot::A, Kind::Erase { permit: 1 });
        let mapped = address(command.header).unwrap();
        controller.issue(&command, mapped, 100, 1).unwrap();
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS]);
        io.set(STAT, IN_PROGRESS);
        assert_eq!(controller.poll(2), ControllerState::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(controller.poll(3), ControllerState::Busy);
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS, ERASE_SECTOR]);
        io.set(STAT, IN_PROGRESS);
        assert_eq!(controller.poll(4), ControllerState::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(
            controller.poll(5),
            ControllerState::Done(command.header, super::super::Status::Ok, [0; 256])
        );
        controller.restore_protection().unwrap();
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS, ERASE_SECTOR, 0]);
        let before = io.0.borrow().writes.clone();
        assert_eq!(
            controller.issue(&command, mapped, 100, 6),
            Err(Error::Stale)
        );
        assert_eq!(io.0.borrow().writes, before);
    }
    #[test]
    fn production_timeout_records_error_and_late_cleanup_cannot_succeed() {
        use super::super::{
            AdmissionEvidence, Controller, ControllerState, FlashController, Status,
        };
        let io = Io::default();
        io.set(0x6011_1074, 0x2200);
        io.set(0x4002_9000, 0x100);
        io.set(PROTECT_NM, 3);
        io.set(SEM_STAT, SEM_OWNED);
        let mut controller = FlashController::new(io.clone());
        let evidence = AdmissionEvidence {
            snapshot: controller.observe_safety().unwrap(),
            expected_static_bank1_a: 0,
            expected_static_bank1_b: 0,
            reports: [[1; 32]; 6],
            command_deadline_us: 10,
        };
        controller.activate(&evidence).unwrap();
        let command = command(Slot::A, Kind::Program);
        let mapped = address(command.header).unwrap();
        controller.issue(&command, mapped, 10, 1).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(controller.poll(10), ControllerState::Fault);
        assert!(controller.last_error().is_some());
        assert_eq!(controller.restore_protection(), Err(Error::Uncertain));
        assert_eq!(io.writes(EXEC), [1]);
        io.set(STAT, DONE | PASS);
        assert_eq!(
            controller.poll(11),
            ControllerState::Done(command.header, Status::Failed { quiescent: true }, [0; 256])
        );
        assert_eq!(io.writes(EXEC), [1]);
        assert_eq!(
            controller.issue(&command, mapped, 20, 12),
            Err(Error::Stale)
        );
    }
    #[test]
    fn stale_done_busy_and_failure_are_not_completion() {
        assert_eq!(classify(DONE | PASS, false), Observe::StaleDone);
        assert_eq!(classify(DONE | PASS | IN_PROGRESS, true), Observe::Busy);
        assert_eq!(
            classify(IN_PROGRESS | 0x10, true),
            Observe::Failed(IN_PROGRESS | 0x10)
        );
        for bit in [0x10, 0x20, 0x40, 0x80, 0x100, 0x1000] {
            assert_eq!(
                classify(DONE | PASS | bit, true),
                Observe::Failed(DONE | PASS | bit)
            );
        }
        assert_eq!(classify(DONE, true), Observe::Failed(DONE));
        assert_eq!(classify(DONE | PASS, true), Observe::Passed);
    }
    #[test]
    fn one_semaphore_attempt_and_clear_before_unprotect() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        io.set(SEM_STAT, 0);
        assert_eq!(seq.begin(&c, 20, 1), Err(Error::Busy));
        assert_eq!(io.writes(SEM_REQ), [1]);
        assert!(io.writes(EXEC).is_empty());
        assert_eq!(seq.begin(&c, 20, 2), Err(Error::Uncertain));
        io.set(SEM_STAT, SEM_OWNED);
        let mut seq = Sequence::new(io.clone());
        seq.begin(&c, 20, 2).unwrap();
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS]);
        assert!(io.writes(PROTECT_B).is_empty());
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(3), Observe::StaleDone);
        io.set(STAT, IN_PROGRESS | DONE | PASS);
        assert_eq!(seq.poll(3), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(3), Observe::Passed);
        assert_eq!(seq.advance(3, false), Err(Error::Uncertain));
        assert_eq!(seq.advance(3, true), Ok(false));
        assert_eq!(io.writes(PROTECT_B), [0xfff & !(1 << 10)]);
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS, ERASE_SECTOR]);
        assert_eq!(io.writes(ADDR).last(), Some(&Slot::A.base()));
    }
    #[test]
    fn sixteen_full_ecc_words_and_cleanup() {
        let (io, mut seq, c) = setup(Kind::Program);
        seq.begin(&c, 100, 1).unwrap();
        for n in 0..17 {
            io.set(STAT, IN_PROGRESS);
            assert_eq!(seq.poll(2 + n), Observe::Busy);
            io.set(STAT, DONE | PASS);
            assert_eq!(seq.poll(2 + n), Observe::Passed);
            let done = seq.advance(2 + n, true).unwrap();
            assert_eq!(done, n == 16);
        }
        let types = io.writes(TYPE);
        assert_eq!(types.len(), 17);
        assert_eq!(types[0], CLEAR_STATUS);
        assert!(types[1..].iter().all(|x| *x == PROGRAM_WORD));
        assert_eq!(io.writes(BYTE_EN)[1..], [FULL_WORD_ECC; 16]);
        let addrs = io.writes(ADDR);
        for n in 0..16 {
            assert_eq!(addrs[n + 1], Slot::A.base() + n as u32 * 16);
        }
        assert_eq!(io.writes(DATA0)[1], 0x03020100);
        assert_eq!(io.writes(DATA_INDEX)[1], 0);
        assert!(io.writes(CTL).iter().all(|value| *value == 0));
        seq.cleanup(true).unwrap();
        assert_eq!(io.writes(TYPE).last(), Some(&0));
        assert_eq!(io.writes(PROTECT_B).last(), Some(&0xfff));
        assert_eq!(io.writes(SEM_CLR), [1]);
    }
    #[test]
    fn wrong_protection_and_uncertain_issue_retain_ownership() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        seq.begin(&c, 10, 1).unwrap();
        io.set(STAT, IN_PROGRESS | DONE | PASS);
        assert_eq!(seq.poll(2), Observe::Busy);
        assert_eq!(seq.advance(2, true), Err(Error::Uncertain));
        assert_eq!(seq.poll(10), Observe::Failed(IN_PROGRESS | DONE | PASS));
        assert_eq!(seq.begin(&c, 20, 11), Err(Error::Uncertain));
        assert_eq!(io.writes(EXEC), [1]);
        assert_eq!(seq.cleanup(true), Err(Error::Uncertain));
        assert!(io.writes(SEM_CLR).is_empty());
        let mut wrong = c.header;
        wrong.command_sequence += 1;
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(11), Observe::Failed(DONE | PASS));
        assert_eq!(seq.late_cleanup(wrong, true), Err(Error::Conflict));
        assert!(io.writes(SEM_CLR).is_empty());
        seq.late_cleanup(c.header, true).unwrap();
        assert_eq!(io.writes(SEM_CLR), [1]);
    }
    #[test]
    fn b_mask_and_read_refusal() {
        assert_eq!(group_mask(Slot::A), 1 << 10);
        assert_eq!(group_mask(Slot::B), 1 << 11);
        let (io, mut seq, _) = setup(Kind::Program);
        let mut read = command(Slot::B, Kind::Read);
        read.header.len = 1;
        assert_eq!(seq.begin(&read, 10, 1), Err(Error::InvalidKind));
        assert!(io.writes(EXEC).is_empty());
    }
    #[test]
    fn protection_readback_failure_never_issues_media_command() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        seq.begin(&c, 20, 1).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(2), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(3), Observe::Passed);
        io.0.borrow_mut().bad_protect_read = true;
        assert_eq!(seq.advance(3, true), Err(Error::Uncertain));
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS]);
        assert_eq!(io.writes(EXEC), [1]);
        assert!(io.writes(SEM_CLR).is_empty());
        assert_eq!(seq.advance(4, true), Err(Error::Uncertain));
        assert_eq!(io.writes(EXEC), [1]);
    }
    #[test]
    fn absolute_deadline_and_old_diagnostic_capture() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        assert_eq!(seq.begin(&c, 5, 5), Err(Error::Uncertain));
        assert!(io.writes(SEM_REQ).is_empty());
        io.set(STAT, DONE | 0x10);
        seq.begin(&c, 5, 1).unwrap();
        assert_eq!(seq.diagnostics & (DONE | 0x10), DONE | 0x10);
        assert_eq!(io.writes(TYPE), [CLEAR_STATUS]);
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(2), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(5), Observe::Failed(DONE | PASS));
        assert_eq!(seq.late_cleanup(c.header, false), Err(Error::Uncertain));
        seq.late_cleanup(c.header, true).unwrap();
        assert_eq!(io.writes(SEM_CLR), [1]);
    }

    #[test]
    fn timed_out_header_cannot_reexecute_after_late_cleanup() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        seq.begin(&c, 6, 1).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(2), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(3), Observe::Passed);
        seq.advance(3, true).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(6), Observe::Failed(IN_PROGRESS));
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(7), Observe::Failed(DONE | PASS));
        seq.late_cleanup(c.header, true).unwrap();
        assert_eq!(io.writes(EXEC).len(), 2);
        let writes_after_cleanup = io.0.borrow().writes.clone();
        io.set(SEM_STAT, SEM_OWNED);
        assert_eq!(seq.begin(&c, 20, 8), Err(Error::Stale));
        assert_eq!(io.0.borrow().writes, writes_after_cleanup);
        assert_eq!(seq.poll(8), Observe::StaleDone);
    }

    #[test]
    fn retired_header_fences_stale_identity_across_next_operation() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        seq.begin(&c, 20, 1).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(2), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(3), Observe::Passed);
        seq.advance(3, true).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(4), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(5), Observe::Passed);
        assert_eq!(seq.advance(5, true), Ok(true));
        seq.cleanup(true).unwrap();

        let writes = io.0.borrow().writes.clone();
        let mut changed = c.clone();
        changed.header.slot = Slot::B;
        let mut older = c.clone();
        older.header.command_sequence -= 1;
        let mut new_epoch = c.clone();
        new_epoch.header.io_epoch += 1;
        let mut exhausted = c.clone();
        exhausted.header.command_sequence = u64::MAX;
        for stale in [&c, &changed, &older, &new_epoch] {
            assert_eq!(seq.begin(stale, 30, 6), Err(Error::Stale));
        }
        assert_eq!(seq.begin(&exhausted, 30, 6), Err(Error::Exhausted));
        assert_eq!(io.0.borrow().writes, writes);

        let mut next = c.clone();
        next.header.command_sequence += 1;
        io.set(SEM_STAT, SEM_OWNED);
        assert_eq!(seq.begin(&next, 30, 6), Ok(()));
        assert_eq!(io.writes(EXEC).len(), 3);
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(7), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(8), Observe::Passed);
        seq.advance(8, true).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(9), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(10), Observe::Passed);
        assert_eq!(seq.advance(10, true), Ok(true));
        seq.cleanup(true).unwrap();
        let writes = io.0.borrow().writes.clone();
        assert_eq!(seq.begin(&c, 40, 11), Err(Error::Stale));
        assert_eq!(seq.begin(&next, 40, 11), Err(Error::Stale));
        assert_eq!(io.0.borrow().writes, writes);
    }

    #[test]
    fn failed_late_cleanup_keeps_timeout_and_header_fenced() {
        let (io, mut seq, c) = setup(Kind::Erase { permit: 1 });
        seq.begin(&c, 6, 1).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(2), Observe::Busy);
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(3), Observe::Passed);
        seq.advance(3, true).unwrap();
        io.set(STAT, IN_PROGRESS);
        assert_eq!(seq.poll(6), Observe::Failed(IN_PROGRESS));
        io.set(STAT, DONE | PASS);
        assert_eq!(seq.poll(7), Observe::Failed(DONE | PASS));
        io.0.borrow_mut().bad_protect_read = true;
        assert_eq!(seq.late_cleanup(c.header, true), Err(Error::Uncertain));
        assert_eq!(seq.poll(8), Observe::Failed(DONE | PASS));
        let mut next = c.clone();
        next.header.command_sequence += 1;
        let writes = io.0.borrow().writes.clone();
        assert_eq!(seq.begin(&c, 20, 8), Err(Error::Uncertain));
        assert_eq!(seq.begin(&next, 20, 8), Err(Error::Uncertain));
        assert_eq!(io.0.borrow().writes, writes);
        io.0.borrow_mut().bad_protect_read = false;
        seq.late_cleanup(c.header, true).unwrap();
        assert_eq!(seq.begin(&c, 20, 8), Err(Error::Stale));
    }
}


pub const DIAGNOSTIC_PATTERN: [u8; 32] = *b"AM13-FLASH-R1-01AM13-FLASH-R1-02";
#[derive(Clone, Copy)]
pub struct DiagnosticResult {
    pub attempted: bool, pub phase: u8, pub error: u8,
    pub status: [u32; 3], pub elapsed_us: [u64; 3], pub readback: [u8; 32],
    pub protect_b: u32, pub pre_status: u32, pub verify_ok: bool,
}
pub struct Diagnostic<I: RegisterIo> {
    seq: Sequence<I>, payload:[u8;32], pub result: DiagnosticResult, pending: bool,
    clearing: bool, started: u64, deadline: u64,
}
impl<I: RegisterIo> Diagnostic<I> {
    pub(super) fn new(seq: Sequence<I>) -> Self {
        Self { seq, payload:DIAGNOSTIC_PATTERN, pending: false, clearing: false, started: 0, deadline: 0,
            result: DiagnosticResult { attempted:false, phase:0, error:0, status:[0;3], elapsed_us:[0;3], readback:[0;32], protect_b:0, pre_status:0, verify_ok:false } }
    }
    /// Select one fixed-length bench record before consuming this boot's allowance.
    pub fn set_bench_record(&mut self, bytes:[u8;32])->Result<(),Error> {
        if self.result.attempted {return Err(Error::Busy);}
        self.payload=bytes;Ok(())
    }
    pub fn rearm_verified(&mut self) -> Result<(),Error> {
        if self.result.phase != 4 || !self.result.verify_ok || self.result.error != 0 || self.pending { return Err(Error::Busy); }
        self.result.attempted=false; self.result.phase=0; self.result.verify_ok=false;
        Ok(())
    }
    fn fail(&mut self, error:u8) -> u8 {
        self.result.error=error; self.pending=false;
        // Restore volatile MAIN protections only when controller is no longer busy.
        if self.seq.sem_owned && self.seq.io.read(STAT)&IN_PROGRESS==0 { self.protect(); }
        2
    }
    fn protect(&mut self) {
        self.seq.io.write(PROTECT_A,u32::MAX); self.seq.io.write(PROTECT_B,0xffff);
        if self.seq.sem_owned { self.seq.io.write(SEM_CLR,1); self.seq.sem_owned=false; }
    }
    /// op 0 consumes admission; 1..3 launch fixed erase/program; 4 reads after success;
    /// 5 polls. Return 0 busy, 1 complete, 2 failed. No caller addresses; fixed32-byte pattern or explicitly selected bench record.
    pub fn step(&mut self, op:u8, now:u64) -> u8 {
        if op==0 {
            if self.result.attempted { return self.fail(1); }
            self.result.attempted=true;
            // Admission reads controller registers too: claim before the first one.
            if self.seq.acquire_once().is_err() { return self.fail(5); }
            let factory=self.seq.io.read(0x60111074);
            let fri=self.seq.io.read(0x40029000);
            self.result.pre_status=self.seq.io.read(STAT);
            if factory&0xfff!=512 || (factory>>12)&3!=2 || (fri>>8)&15<1
                || self.seq.io.read(0x4002900c)&4!=0 || self.seq.io.read(0x400b2048)&0x1000!=0
                || self.result.pre_status&IN_PROGRESS!=0 || self.seq.io.read(PROTECT_NM)&3!=3 { return self.fail(3); }
            return 1;
        }
        if self.result.error!=0 || !self.result.attempted { return 2; }
        if (1..=3).contains(&op) {
            if self.pending || op!=self.result.phase+1 { return self.fail(4); }
            self.result.phase=op;
            if self.seq.acquire_once().is_err() { return self.fail(5); }
            self.started=now; self.deadline=now.saturating_add(500_000);
            self.pending=true; self.clearing=true;
            self.seq.launch(CLEAR_STATUS,0,None);
            return 0;
        }
        if op==5 && self.pending {
            let raw=self.seq.io.read(STAT);
            let i=(self.result.phase-1) as usize;
            self.result.status[i]=raw; self.result.elapsed_us[i]=now.saturating_sub(self.started);
            if now>=self.deadline { return self.fail(6); }
            if raw&IN_PROGRESS!=0 { return 0; }
            if self.clearing {
                // SDK clear-status is a command-register write; terminal can be all-zero.
                if raw&FAILURES!=0 { return self.fail(7); }
                self.clearing=false;
                self.seq.io.write(PROTECT_A,u32::MAX);
                // Characterization: clear both candidate A mappings (SDK10/TRM14).
                // Every issued media address remains hard-coded to sector A.
                self.seq.io.write(PROTECT_B,0xffff & !(1<<10) & !(1<<14));
                self.result.protect_b=self.seq.io.read(PROTECT_B);
                if self.seq.io.read(PROTECT_NM)&3!=3 { return self.fail(8); }
                if self.result.phase==1 { self.seq.launch(ERASE_SECTOR,0x78000,None); }
                else {
                    let off=(self.result.phase as usize-2)*16;
                    let word: [u8;16]=self.payload[off..off+16].try_into().unwrap();
                    self.seq.launch(PROGRAM_WORD,0x78000+off as u32,Some(&word));
                }
                return 0;
            }
            if raw&FAILURES!=0 || raw&(DONE|PASS)!=(DONE|PASS) { return self.fail(9); }
            self.pending=false; self.protect();
            if self.result.phase==1 {
                for addr in (0x78000..0x78800).step_by(4) {
                    if self.seq.io.read(addr)!=u32::MAX { return self.fail(12); }
                }
            }
            return 1;
        }
        if op==4 && self.result.phase==3 && !self.pending {
            // Exact aligned reads only after all three successful commands.
            for i in 0..8 {
                self.result.readback[i*4..i*4+4].copy_from_slice(&self.seq.io.read(0x78000+i*4).to_le_bytes());
            }
            self.result.verify_ok=self.result.readback==self.payload;
            if !self.result.verify_ok { return self.fail(10); }
            self.result.phase=4; return 1;
        }
        self.fail(11)
    }
}

#[cfg(test)]
mod diagnostic_tests {
    extern crate std;
    use super::*;
    use std::{rc::Rc, cell::RefCell, collections::BTreeMap, vec::Vec};
    #[derive(Clone,Default)]
    struct Io { regs:Rc<RefCell<BTreeMap<usize,u32>>>, writes:Rc<RefCell<Vec<(usize,u32)>>> }
    impl RegisterIo for Io {
        fn read(&self,a:usize)->u32 {
            if (NVMNW..NVMNW+0x2000).contains(&a) { assert_eq!(self.regs.borrow().get(&SEM_STAT).copied().unwrap_or(0), SEM_OWNED, "unowned NVMNW read {a:#x}"); }
            if a==0x60111074 { return 512|(2<<12); }
            if a==0x40029000 { return 2<<8; }
            if a==PROTECT_NM { return 3; }

            *self.regs.borrow().get(&a).unwrap_or(&if (0x78000..0x78800).contains(&a) {u32::MAX} else {0})
        }
        fn write(&self,a:usize,v:u32) {
            if (NVMNW..NVMNW+0x2000).contains(&a) { assert_eq!(self.regs.borrow().get(&SEM_STAT).copied().unwrap_or(0), SEM_OWNED, "unowned NVMNW write {a:#x}"); }
            if a==SEM_REQ { self.regs.borrow_mut().insert(SEM_STAT,SEM_OWNED); }
            if a==SEM_CLR { self.regs.borrow_mut().insert(SEM_STAT,0); }
            self.writes.borrow_mut().push((a,v)); self.regs.borrow_mut().insert(a,v);
            if a==EXEC {
                let kind=self.read(TYPE);
                self.regs.borrow_mut().insert(STAT,if kind==CLEAR_STATUS {0} else {3});
                if kind==PROGRAM_WORD {
                    let addr=self.read(ADDR) as usize;
                    for i in 0..4 { let v=self.read(DATA0+i*4);self.regs.borrow_mut().insert(addr+i*4,v); }
                }
            }
        }
        fn delay_cycles(&self,_:u32){}
    }
    #[test]
    fn startup_observation_claims_controller_before_status() {
        let io=Io::default();
        let mut controller=super::super::FlashController::new(io.clone());
        assert!(controller.observe().unwrap().basic_readiness());
        assert_eq!(io.read(SEM_STAT),SEM_OWNED);
        assert_eq!(io.writes.borrow().as_slice(), &[(SEM_REQ,1)]);
        let mut diagnostic=controller.into_diagnostic();
        assert_eq!(diagnostic.step(0,0),1);
        // Retained ownership is reused, not initialized again at admission.
        assert_eq!(io.writes.borrow().as_slice(), &[(SEM_REQ,1)]);
    }
    #[test]
    fn diagnostic_exact_media_bound_and_readback() {
        let io=Io::default(); let mut d=Diagnostic::new(Sequence::new(io.clone()));
        assert_eq!(d.step(0,0),1);
        for phase in 1..=3 { assert_eq!(d.step(phase,10),0);assert_eq!(d.step(5,11),0);assert_eq!(d.step(5,12),1); }
        assert_eq!(d.step(4,20),1);assert_eq!(d.result.readback,DIAGNOSTIC_PATTERN);
        let writes=io.writes.borrow();
        let addresses:Vec<u32>=writes.iter().filter(|(a,_)| *a==ADDR).map(|(_,v)|*v).collect();
        assert_eq!(addresses,std::vec![0,0x78000,0,0x78000,0,0x78010]);
        assert!(!writes.iter().any(|(a,_)|*a==PROTECT_NM));
        drop(writes); let count=io.writes.borrow().iter().filter(|(a,_)|*a==EXEC).count();
        assert_eq!(d.step(0,21),2);assert_eq!(d.step(1,22),2);
        assert_eq!(io.writes.borrow().iter().filter(|(a,_)|*a==EXEC).count(),count);
    }
    #[test]
    fn bench_record_uses_same_exact_bounds_and_consumes_allowance() {
        let io=Io::default();let mut d=Diagnostic::new(Sequence::new(io.clone()));
        let record=[0x5a;32];d.set_bench_record(record).unwrap();assert_eq!(d.step(0,0),1);
        assert_eq!(d.set_bench_record([0;32]),Err(Error::Busy));
        for phase in 1..=3 {assert_eq!(d.step(phase,10),0);assert_eq!(d.step(5,11),0);assert_eq!(d.step(5,12),1);}
        assert_eq!(d.step(4,20),1);assert_eq!(d.result.readback,record);
        let addresses:Vec<u32>=io.writes.borrow().iter().filter(|(a,_)|*a==ADDR).map(|(_,v)|*v).collect();
        assert_eq!(addresses,std::vec![0,0x78000,0,0x78000,0,0x78010]);
    }
    #[test]
    fn diagnostic_busy_timeout_has_no_retry_or_readback() {
        let io=Io::default();let mut d=Diagnostic::new(Sequence::new(io.clone()));
        assert_eq!(d.step(0,0),1);assert_eq!(d.step(1,1),0);
        io.regs.borrow_mut().insert(STAT,IN_PROGRESS);
        assert_eq!(d.step(5,2),0);assert_eq!(d.step(5,500001),2);
        let n=io.writes.borrow().len();assert_eq!(d.step(2,500002),2);assert_eq!(io.writes.borrow().len(),n);
        assert_eq!(d.result.error,6);assert!(!d.result.verify_ok);
    }
}
