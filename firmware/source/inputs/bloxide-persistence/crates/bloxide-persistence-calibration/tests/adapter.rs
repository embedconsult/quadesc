use bloxide_calibration::{
    ApplyField, ApplyRequest, CommitGate, CommitRecord, EncodedValue, GateError, OperationKey,
    Owner, RejectReason,
};
use bloxide_persistence::{OperationKey as PersistenceKey, Outcome, Schema, SchemaError};
use bloxide_persistence_calibration::{
    Broker, BrokerError, CanonicalCalibration, CaptureError, capture_owner,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Led {
    period: u16,
    duty: u16,
}

impl CanonicalCalibration for Led {
    fn encode(self, out: &mut [u8; 256]) -> u16 {
        out[..2].copy_from_slice(&self.period.to_le_bytes());
        out[2..4].copy_from_slice(&self.duty.to_le_bytes());
        4
    }
}

#[derive(Clone, Copy)]
struct LedSchema;

impl Schema for LedSchema {
    fn id(&self) -> u16 {
        1
    }

    fn payload_len(&self) -> u16 {
        4
    }

    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        Led {
            period: 1000,
            duty: 500,
        }
        .encode(out);
        Ok(())
    }

    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError> {
        if bytes.len() != 4 {
            return Err(SchemaError::InvalidValue);
        }
        let period = u16::from_le_bytes(bytes[..2].try_into().unwrap());
        let duty = u16::from_le_bytes(bytes[2..].try_into().unwrap());
        if (100..=10_000).contains(&period) && duty <= 1000 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}

struct Gate;

impl CommitGate<Led> for Gate {
    type Permit = ();

    fn try_reserve(&mut self, _next: &Led) -> Result<Self::Permit, GateError> {
        Ok(())
    }

    fn publish(&mut self, (): Self::Permit, _record: CommitRecord<Led>) {}
}

fn owner_key(sequence: u64) -> OperationKey {
    OperationKey {
        service_epoch: 7,
        session_generation: 3,
        sequence,
    }
}

fn request(sequence: u64, period: u16) -> ApplyRequest {
    ApplyRequest {
        field: ApplyField {
            key: owner_key(sequence),
            field_id: 1,
            encoded_value: EncodedValue::try_from_slice(&period.to_le_bytes()).unwrap(),
            expires_at_us: 100,
        },
        expected_revision: None,
    }
}

fn patch(mut current: Led, field: u32, value: EncodedValue) -> Result<Led, RejectReason> {
    if field != 1 || value.as_slice().len() != 2 {
        return Err(RejectReason::BadEncoding);
    }
    current.period = u16::from_le_bytes(value.as_slice().try_into().unwrap());
    Ok(current)
}

fn to_persistence(key: OperationKey) -> PersistenceKey {
    PersistenceKey {
        service_epoch: key.service_epoch,
        session_generation: key.session_generation,
        sequence: key.sequence,
    }
}

#[test]
fn exact_public_owner_getters_capture_one_immutable_snapshot() {
    let initial = Led {
        period: 1000,
        duty: 500,
    };
    let mut owner = Owner::new(initial, 7, 3);
    let mut broker = Broker::<1>::new(90, 11);
    let save_a = broker.reserve(0).unwrap();
    let captured_a = capture_owner(&owner, &LedSchema, save_a, Some((7, 0))).unwrap();
    assert_eq!(captured_a.bytes(), &[0xE8, 0x03, 0xF4, 0x01]);
    assert_eq!(captured_a.source_epoch(), 7);
    assert_eq!(captured_a.source_revision(), 0);

    let mut gate = Gate;
    let outcome = owner.apply(request(1, 2000), || 10, patch, &mut gate);
    assert!(matches!(
        outcome,
        bloxide_calibration::OwnerOutcome::Applied { .. }
    ));
    owner.release(owner_key(1)).unwrap();
    assert_eq!(owner.active().period, 2000);
    assert_eq!(owner.active_revision().get(), 1);

    // The first owned capture remains byte-for-byte revision zero after the RAM
    // owner advances. A later capture is a separate immutable value.
    assert_eq!(captured_a.bytes(), &[0xE8, 0x03, 0xF4, 0x01]);
    broker
        .record_terminal(
            0,
            Outcome::Cancelled {
                key: to_persistence(save_a),
            },
        )
        .unwrap();
    broker.release(0).unwrap();
    let save_b = broker.reserve(0).unwrap();
    let captured_b = capture_owner(&owner, &LedSchema, save_b, Some((7, 1))).unwrap();
    assert_eq!(captured_b.bytes(), &[0xD0, 0x07, 0xF4, 0x01]);
    assert_eq!(captured_b.source_revision(), 1);
}

#[test]
fn owner_lifecycle_and_expected_labels_gate_capture() {
    let initial = Led {
        period: 1000,
        duty: 500,
    };
    let mut owner = Owner::new(initial, 7, 3);
    let key = OperationKey {
        service_epoch: 90,
        session_generation: 11,
        sequence: 1,
    };
    assert_eq!(
        capture_owner(&owner, &LedSchema, key, Some((8, 0))),
        Err(CaptureError::EpochMismatch)
    );
    assert_eq!(
        capture_owner(&owner, &LedSchema, key, Some((7, 1))),
        Err(CaptureError::RevisionMismatch)
    );
    owner.stop();
    assert_eq!(
        capture_owner(&owner, &LedSchema, key, None),
        Err(CaptureError::OwnerNotRunning)
    );
}

#[test]
fn four_clients_retain_exact_correlated_results_without_eviction() {
    let mut broker = Broker::<4>::new(90, 11);
    let mut keys = [owner_key(0); 4];
    for (client, key) in keys.iter_mut().enumerate() {
        *key = broker.reserve(client).unwrap();
    }
    assert_eq!(broker.reserve(4), Err(BrokerError::ClientOutOfRange));
    assert_eq!(broker.reserve(0), Err(BrokerError::ClientBusy));
    assert_eq!(
        broker.record_terminal(
            0,
            Outcome::Cancelled {
                key: PersistenceKey {
                    service_epoch: 90,
                    session_generation: 11,
                    sequence: 99,
                },
            },
        ),
        Err(BrokerError::KeyMismatch)
    );
    for (client, &key) in keys.iter().enumerate() {
        let outcome = Outcome::Cancelled {
            key: to_persistence(key),
        };
        broker.record_terminal(client, outcome).unwrap();
        assert_eq!(broker.result(client), Some(outcome));
    }
    assert_eq!(broker.advance_epoch(91, 12), Err(BrokerError::ClientBusy));
    for (client, &key) in keys.iter().enumerate() {
        assert_eq!(broker.release(client).unwrap(), key);
    }
    broker.advance_epoch(91, 12).unwrap();
    let next = broker.reserve(0).unwrap();
    assert_eq!(next.service_epoch, 91);
    assert_eq!(next.session_generation, 12);
    assert_eq!(next.sequence, 1);
}
