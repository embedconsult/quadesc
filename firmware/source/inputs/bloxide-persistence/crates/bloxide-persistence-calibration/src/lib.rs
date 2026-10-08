#![no_std]
#![forbid(unsafe_code)]

use bloxide_calibration::{OperationKey as CalibrationKey, Owner, OwnerMode};
use bloxide_persistence::{OperationKey, Outcome, Schema, Snapshot, SnapshotError};

pub trait CanonicalCalibration: Copy + Eq {
    fn encode(self, out: &mut [u8; 256]) -> u16;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureError {
    OwnerNotRunning,
    EpochMismatch,
    RevisionMismatch,
    InvalidSnapshot(SnapshotError),
}

pub fn capture_owner<V: CanonicalCalibration, S: Schema>(
    owner: &Owner<V>,
    schema: &S,
    persistence_key: CalibrationKey,
    expected: Option<(u64, u32)>,
) -> Result<Snapshot, CaptureError> {
    if owner.mode() != OwnerMode::Running {
        return Err(CaptureError::OwnerNotRunning);
    }
    let service_epoch = owner.service_epoch();
    let revision = owner.active_revision().get();
    if let Some((expected_epoch, expected_revision)) = expected {
        if service_epoch != expected_epoch {
            return Err(CaptureError::EpochMismatch);
        }
        if revision != expected_revision {
            return Err(CaptureError::RevisionMismatch);
        }
    }
    Snapshot::validate_schema(schema).map_err(CaptureError::InvalidSnapshot)?;
    let mut encoded = [0xFF; 256];
    let len = owner.active().encode(&mut encoded);
    if len == 0 || usize::from(len) > encoded.len() {
        return Err(CaptureError::InvalidSnapshot(
            SnapshotError::LengthOutOfRange,
        ));
    }
    Snapshot::new(
        schema,
        &encoded[..usize::from(len)],
        service_epoch,
        revision,
        OperationKey {
            service_epoch: persistence_key.service_epoch,
            session_generation: persistence_key.session_generation,
            sequence: persistence_key.sequence,
        },
    )
    .map_err(CaptureError::InvalidSnapshot)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerError {
    ClientOutOfRange,
    ClientBusy,
    KeyMismatch,
    SequenceExhausted,
    GenerationExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientSlot {
    key: Option<CalibrationKey>,
    result: Option<Outcome>,
}

impl ClientSlot {
    const EMPTY: Self = Self {
        key: None,
        result: None,
    };
}

pub struct Broker<const CLIENTS: usize> {
    service_epoch: u64,
    session_generation: u32,
    next_sequence: u64,
    retired_through: Option<u64>,
    clients: [ClientSlot; CLIENTS],
}

impl<const CLIENTS: usize> Broker<CLIENTS> {
    pub const fn new(service_epoch: u64, session_generation: u32) -> Self {
        assert!(CLIENTS >= 1 && CLIENTS <= 4);
        Self {
            service_epoch,
            session_generation,
            next_sequence: 1,
            retired_through: None,
            clients: [ClientSlot::EMPTY; CLIENTS],
        }
    }

    pub fn reserve(&mut self, client: usize) -> Result<CalibrationKey, BrokerError> {
        let Some(slot) = self.clients.get_mut(client) else {
            return Err(BrokerError::ClientOutOfRange);
        };
        if slot.key.is_some() || slot.result.is_some() {
            return Err(BrokerError::ClientBusy);
        }
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(BrokerError::SequenceExhausted)?;
        let key = CalibrationKey {
            service_epoch: self.service_epoch,
            session_generation: self.session_generation,
            sequence,
        };
        slot.key = Some(key);
        Ok(key)
    }

    pub fn record_terminal(&mut self, client: usize, result: Outcome) -> Result<(), BrokerError> {
        let Some(slot) = self.clients.get_mut(client) else {
            return Err(BrokerError::ClientOutOfRange);
        };
        if slot.key.is_none() || slot.result.is_some() {
            return Err(BrokerError::ClientBusy);
        }
        let key = slot.key.expect("checked above");
        let result_key = result.key();
        if result_key.service_epoch != key.service_epoch
            || result_key.session_generation != key.session_generation
            || result_key.sequence != key.sequence
        {
            return Err(BrokerError::KeyMismatch);
        }
        slot.result = Some(result);
        Ok(())
    }

    #[must_use]
    pub fn result(&self, client: usize) -> Option<Outcome> {
        self.clients.get(client).and_then(|slot| slot.result)
    }

    pub fn release(&mut self, client: usize) -> Result<CalibrationKey, BrokerError> {
        let Some(slot) = self.clients.get_mut(client) else {
            return Err(BrokerError::ClientOutOfRange);
        };
        let Some(key) = slot.key else {
            return Err(BrokerError::ClientBusy);
        };
        if slot.result.is_none() {
            return Err(BrokerError::ClientBusy);
        }
        self.retired_through = Some(
            self.retired_through
                .map_or(key.sequence, |retired| retired.max(key.sequence)),
        );
        *slot = ClientSlot::EMPTY;
        Ok(key)
    }

    pub fn advance_epoch(
        &mut self,
        service_epoch: u64,
        session_generation: u32,
    ) -> Result<(), BrokerError> {
        if self.clients.iter().any(|slot| slot.key.is_some()) {
            return Err(BrokerError::ClientBusy);
        }
        let Some(expected_epoch) = self.service_epoch.checked_add(1) else {
            return Err(BrokerError::GenerationExhausted);
        };
        let Some(expected_generation) = self.session_generation.checked_add(1) else {
            return Err(BrokerError::GenerationExhausted);
        };
        if service_epoch != expected_epoch || session_generation != expected_generation {
            return Err(BrokerError::GenerationExhausted);
        }
        self.service_epoch = service_epoch;
        self.session_generation = session_generation;
        self.next_sequence = 1;
        self.retired_through = None;
        Ok(())
    }
}
