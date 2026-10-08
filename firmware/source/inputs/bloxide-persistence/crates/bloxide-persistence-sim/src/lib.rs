#![forbid(unsafe_code)]

use bloxide_persistence::{
    BackendCommand, BackendConfig, BackendTiming, CommandHeader, CommandKind, Completion,
    CompletionStatus, Geometry, MAX_PAYLOAD_BYTES, Slot, WearLease,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub provisioning: u64,
    pub request: CommandHeader,
    pub permit: u64,
    pub consumed: bool,
}

/// Host model. Authority and media survive replacement of the MCU service.
/// This in-process model does not claim crash persistence of the host itself.
#[derive(Clone)]
pub struct Simulator {
    geometry: Geometry,
    slots: [Vec<u8>; 2],
    programmed: [Vec<bool>; 2],
    wear_remaining: [u16; 2],
    receipts: Vec<Receipt>,
    receipt_limit: usize,
    provisioning: u64,
    lease: u64,
    qualified: bool,
    erase_commands: u32,
    program_commands: u32,
}

impl Simulator {
    #[must_use]
    pub fn new(geometry: Geometry, wear_per_slot: u16, provisioning: u64) -> Self {
        let erase = geometry.erase_bytes() as usize;
        let granules = erase / usize::from(geometry.granule_bytes());
        Self {
            geometry,
            slots: [vec![0xFF; erase], vec![0xFF; erase]],
            programmed: [vec![false; granules], vec![false; granules]],
            wear_remaining: [wear_per_slot; 2],
            receipts: Vec::with_capacity(usize::from(wear_per_slot) * 2),
            receipt_limit: usize::from(wear_per_slot) * 2,
            provisioning,
            lease: 0,
            qualified: true,
            erase_commands: 0,
            program_commands: 0,
        }
    }

    /// Qualified MCU boot handshake. Allocated by the persistent external authority;
    /// the caller supplies no boot identity. Old receipts remain spent/reserved.
    pub fn open_session(&mut self) -> Option<BackendConfig> {
        if !self.qualified {
            return None;
        }
        self.lease = self.lease.checked_add(1)?;
        Some(BackendConfig {
            lease: WearLease(self.lease),
            timing: BackendTiming {
                read: 10,
                permit: 10,
                erase: 100,
                program: 10,
                quiesce: 100,
            },
        })
    }

    pub fn lose_authority(&mut self) {
        self.qualified = false;
    }

    const fn index(slot: Slot) -> usize {
        match slot {
            Slot::A => 0,
            Slot::B => 1,
        }
    }

    #[must_use]
    pub fn slot(&self, slot: Slot) -> &[u8] {
        &self.slots[Self::index(slot)]
    }
    /// Fault/prestate construction only; service workloads use execute/cut methods.
    pub fn slot_mut(&mut self, slot: Slot) -> &mut [u8] {
        &mut self.slots[Self::index(slot)]
    }
    #[must_use]
    pub const fn erase_commands(&self) -> u32 {
        self.erase_commands
    }
    #[must_use]
    pub const fn program_commands(&self) -> u32 {
        self.program_commands
    }
    #[must_use]
    pub fn wear_remaining(&self, slot: Slot) -> u16 {
        self.wear_remaining[Self::index(slot)]
    }

    fn consume_erase(&mut self, command: &BackendCommand) -> bool {
        let CommandKind::Erase { permit } = command.header.kind else {
            return false;
        };
        if !self.qualified || command.header.offset != 0 || command.header.len != 0 {
            return false;
        }
        let Some(r) = self.receipts.iter_mut().find(|r| r.permit == permit) else {
            return false;
        };
        if r.consumed
            || r.provisioning != self.provisioning
            || r.request.slot != command.header.slot
            || r.request.io_epoch != command.header.io_epoch
            || r.request.command_sequence.checked_add(1) != Some(command.header.command_sequence)
            || !matches!(r.request.kind, CommandKind::ConsumeErasePermit { lease, .. } if lease.0 == self.lease)
        {
            return false;
        }
        // Durable consumption precedes any physical admission, even a zero-byte cut.
        r.consumed = true;
        self.erase_commands += 1;
        true
    }

    #[must_use]
    pub fn execute(&mut self, command: BackendCommand) -> Completion {
        let mut completion = Completion {
            header: command.header,
            status: CompletionStatus::Ok,
            data: [0; MAX_PAYLOAD_BYTES],
        };
        let slot_index = Self::index(command.header.slot);
        match command.header.kind {
            CommandKind::Read => {
                let start = command.header.offset as usize;
                let len = usize::from(command.header.len);
                let end = start.saturating_add(len);
                if len == 0 || len > MAX_PAYLOAD_BYTES || end > self.slots[slot_index].len() {
                    completion.status = CompletionStatus::Failed { quiescent: true };
                } else {
                    completion.data[..len].copy_from_slice(&self.slots[slot_index][start..end]);
                    completion.status = CompletionStatus::Read {
                        len: command.header.len,
                        issue: None,
                    };
                }
            }
            CommandKind::ConsumeErasePermit { lease, .. } => {
                if !self.qualified || lease.0 == 0 || lease.0 != self.lease {
                    completion.status = CompletionStatus::WearDenied;
                } else if let Some(r) = self.receipts.iter().find(|r| r.request == command.header) {
                    completion.status = CompletionStatus::WearGranted { permit: r.permit };
                } else if self.wear_remaining[slot_index] == 0
                    || self.receipts.len() == self.receipt_limit
                {
                    completion.status = CompletionStatus::WearDenied;
                } else {
                    self.wear_remaining[slot_index] -= 1;
                    let permit = self.receipts.len() as u64 + 1;
                    self.receipts.push(Receipt {
                        provisioning: self.provisioning,
                        request: command.header,
                        permit,
                        consumed: false,
                    });
                    completion.status = CompletionStatus::WearGranted { permit };
                }
            }
            CommandKind::Erase { .. } => {
                if self.consume_erase(&command) {
                    self.slots[slot_index].fill(0xFF);
                    self.programmed[slot_index].fill(false);
                } else {
                    completion.status = CompletionStatus::Failed { quiescent: true };
                }
            }
            CommandKind::Program => {
                if !self.execute_program_prefix(&command, usize::from(command.header.len)) {
                    completion.status = CompletionStatus::Failed { quiescent: true };
                }
            }
        }
        completion
    }

    /// Admit one program, including zero-byte interruption. A second program of the
    /// same granule is rejected even when the first changed no bits.
    pub fn execute_program_prefix(&mut self, command: &BackendCommand, prefix: usize) -> bool {
        let index = Self::index(command.header.slot);
        let start = command.header.offset as usize;
        let len = usize::from(command.header.len);
        let end = start.saturating_add(len);
        let granule = start / usize::from(self.geometry.granule_bytes());
        if command.header.kind != CommandKind::Program
            || len != usize::from(self.geometry.granule_bytes())
            || end > self.slots[index].len()
            || !start.is_multiple_of(len)
            || self.programmed[index][granule]
            || self.slots[index][start..end]
                .iter()
                .zip(&command.data[..len])
                .any(|(&a, &b)| a & b != b)
        {
            return false;
        }
        self.programmed[index][granule] = true;
        self.program_commands += 1;
        for offset in 0..len.min(prefix) {
            self.slots[index][start + offset] &= command.data[offset];
        }
        true
    }

    pub fn execute_erase_prefix(&mut self, command: &BackendCommand, prefix: usize) -> bool {
        if !self.consume_erase(command) {
            return false;
        }
        let index = Self::index(command.header.slot);
        let count = prefix.min(self.slots[index].len());
        self.slots[index][..count].fill(0xFF);
        // Only a completed erase permits subsequent programs without another erase.
        if count == self.slots[index].len() {
            self.programmed[index].fill(false);
        }
        true
    }

    /// Raw monotone fault-model operation for codec algebra; not service admission.
    pub fn execute_erase_subset(&mut self, slot: Slot, mask: &[u8]) {
        let index = Self::index(slot);
        for (byte, &raise) in self.slots[index].iter_mut().zip(mask) {
            *byte |= raise;
        }
    }
}
