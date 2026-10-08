use crate::CommandHeader;

/// Compact diagnostics only; never used as an ownership or outcome authority.
#[derive(Clone, Copy, Default)]
pub struct TraceEvent {
    pub io_epoch: u64,
    pub command_sequence: u64,
    pub event: u8,
    pub slot: u8,
}

#[derive(Default)]
pub struct TraceRing {
    entries: [TraceEvent; 32],
    next: usize,
}

impl TraceRing {
    pub fn record(&mut self, header: CommandHeader, event: u8) {
        self.entries[self.next] = TraceEvent {
            io_epoch: header.io_epoch,
            command_sequence: header.command_sequence,
            event,
            slot: if header.slot == crate::Slot::A { 0 } else { 1 },
        };
        self.next = (self.next + 1) % self.entries.len();
    }
    #[must_use]
    pub fn entries(&self) -> &[TraceEvent; 32] {
        &self.entries
    }
}
