#![forbid(unsafe_code)]

use bloxide_calibration::{
    ApplyField, ApplyRequest, BoundedU16, CommitGate, CommitRecord, EncodedValue, GateError,
    Milliseconds, OperationKey, Owner, OwnerOutcome, RejectReason,
};
use core::cell::Cell;

type Period = BoundedU16<Milliseconds, 100, 10_000>;

fn initial() -> Period {
    Period::try_new(1000).unwrap()
}

fn request(sequence: u64, period: u16, deadline: u64) -> ApplyRequest {
    ApplyRequest {
        field: ApplyField {
            key: OperationKey {
                service_epoch: 7,
                session_generation: 3,
                sequence,
            },
            field_id: 1,
            encoded_value: EncodedValue::try_from_slice(&period.to_le_bytes()).unwrap(),
            expires_at_us: deadline,
        },
        expected_revision: Some(0),
    }
}

struct Producer {
    reserved: bool,
    ready: Option<CommitRecord<Period>>,
    phase_epoch_us: u64,
    reserves: u32,
    cancellations: u32,
    publications: u32,
    error: Option<GateError>,
}

impl Producer {
    fn new() -> Self {
        Self {
            reserved: false,
            ready: None,
            phase_epoch_us: 17,
            reserves: 0,
            cancellations: 0,
            publications: 0,
            error: None,
        }
    }

    fn try_reserve(&mut self) -> Result<Permit<'_>, GateError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.reserved || self.ready.is_some() {
            return Err(GateError::Busy);
        }
        self.reserved = true;
        self.reserves += 1;
        Ok(Permit {
            producer: self,
            armed: true,
        })
    }
}

// Move-only exclusive borrow, no unsafe/static allocation or Copy/Clone permit.
struct Permit<'a> {
    producer: &'a mut Producer,
    armed: bool,
}

impl Permit<'_> {
    fn publish(mut self, record: CommitRecord<Period>) {
        assert!(self.producer.reserved);
        assert!(self.producer.ready.is_none());
        self.producer.phase_epoch_us = record.applied_at_us;
        self.producer.ready = Some(record);
        self.producer.publications += 1;
        self.producer.reserved = false;
        self.armed = false;
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.producer.reserved = false;
            self.producer.cancellations += 1;
        }
    }
}

struct Gate<'a> {
    producer: Option<&'a mut Producer>,
    clock: &'a Cell<u64>,
    reserve_at: Option<u64>,
}

impl<'a> CommitGate<Period> for Gate<'a> {
    type Permit = Permit<'a>;

    fn try_reserve(&mut self, _: &Period) -> Result<Self::Permit, GateError> {
        let permit = self.producer.take().unwrap().try_reserve()?;
        if let Some(time) = self.reserve_at {
            self.clock.set(time);
        }
        Ok(permit)
    }

    fn publish(&mut self, permit: Self::Permit, record: CommitRecord<Period>) {
        permit.publish(record);
    }
}

fn clock_case(
    candidate_at: u64,
    reserve_at: Option<u64>,
    period: u16,
) -> (Owner<Period>, Producer, OwnerOutcome<Period>) {
    let clock = Cell::new(90);
    let mut owner = Owner::new(initial(), 7, 3);
    let mut producer = Producer::new();
    let req = request(1, period, 100);
    let outcome = owner.apply(
        req,
        || clock.get(),
        |_, id, encoded| {
            assert_eq!(id, 1);
            clock.set(candidate_at);
            Period::try_decode(encoded).map_err(|_| RejectReason::Bounds)
        },
        &mut Gate {
            producer: Some(&mut producer),
            clock: &clock,
            reserve_at,
        },
    );
    assert_eq!(outcome.key(), req.field.key);
    assert_eq!(owner.retained(), Some(outcome));
    // Resolving or replaying after expiry preserves the exact terminal outcome
    // without invoking clock/candidate or trying to reuse the output slot.
    assert_eq!(owner.resolve_or_cancel(req.field.key), outcome);
    assert_eq!(
        owner.apply(
            req,
            || panic!("retained result must bypass clock"),
            |_, _, _| panic!("retained result must bypass candidate"),
            &mut Gate {
                producer: Some(&mut producer),
                clock: &clock,
                reserve_at: None
            }
        ),
        outcome
    );
    (owner, producer, outcome)
}

fn assert_expired(owner: &Owner<Period>, producer: &Producer, outcome: OwnerOutcome<Period>) {
    assert_eq!(
        outcome,
        OwnerOutcome::Rejected {
            key: request(1, 2000, 100).field.key,
            reason: RejectReason::Expired,
            active_values: initial(),
            active_revision: owner.active_revision(),
        }
    );
    assert_eq!(owner.active(), initial());
    assert_eq!(owner.active_revision().get(), 0);
    assert_eq!(producer.phase_epoch_us, 17);
    assert!(producer.ready.is_none());
    assert!(!producer.reserved);
    assert_eq!(producer.publications, 0);
}

#[test]
fn validation_reaching_or_crossing_deadline_cancels_changed_reservation() {
    for time in [100, 101] {
        let (owner, producer, outcome) = clock_case(time, None, 2000);
        assert_expired(&owner, &producer, outcome);
        assert_eq!(producer.reserves, 1);
        assert_eq!(producer.cancellations, 1);
    }
}

#[test]
fn final_commit_deadline_must_reject_and_cancel_reserved_slot() {
    for time in [100, 101] {
        let (owner, producer, outcome) = clock_case(95, Some(time), 2000);
        assert_expired(&owner, &producer, outcome);
        assert_eq!(producer.reserves, 1);
        assert_eq!(producer.cancellations, 1);
    }
}

#[test]
fn same_value_reaching_or_crossing_deadline_rejects_without_reservation() {
    for time in [100, 101] {
        let (owner, producer, outcome) = clock_case(time, None, 1000);
        assert_expired(&owner, &producer, outcome);
        assert_eq!(producer.reserves, 0);
        assert_eq!(producer.cancellations, 0);
    }
}

#[test]
fn just_before_expiry_uses_final_sample_for_record_outcome_and_phase() {
    let (owner, producer, outcome) = clock_case(95, Some(99), 2000);
    let record = producer.ready.unwrap();
    assert_eq!(
        outcome,
        OwnerOutcome::Applied {
            key: record.key,
            changed: true,
            active_values: record.active_values,
            active_revision: record.active_revision,
            applied_at_us: 99,
        }
    );
    assert_eq!(record.applied_at_us, 99);
    assert_eq!(producer.phase_epoch_us, 99);
    assert_eq!(record.active_values, owner.active());
    assert_eq!(owner.active().get(), 2000);
    assert_eq!(record.active_revision.get(), 1);
    assert_eq!(owner.active_revision(), record.active_revision);
    assert_eq!(producer.publications, 1);
    assert_eq!(producer.cancellations, 0);
    assert!(!producer.reserved); // consuming the permit did not cancel Ready
}

#[test]
fn same_value_uses_fresh_time_and_preserves_existing_phase_and_output() {
    let (mut owner, mut producer, _) = clock_case(95, Some(99), 2000);
    let before = producer.ready;
    owner.release(request(1, 2000, 100).field.key).unwrap();
    let clock = Cell::new(100);
    let mut req = request(2, 2000, 110);
    req.expected_revision = Some(1);
    let outcome = owner.apply(
        req,
        || clock.get(),
        |current, _, _| {
            clock.set(109);
            Ok(current)
        },
        &mut Gate {
            producer: Some(&mut producer),
            clock: &clock,
            reserve_at: Some(110),
        },
    );
    assert_eq!(
        outcome,
        OwnerOutcome::Applied {
            key: req.field.key,
            changed: false,
            active_values: owner.active(),
            active_revision: owner.active_revision(),
            applied_at_us: 109,
        }
    );
    assert_eq!(owner.active_revision().get(), 1);
    assert_eq!(producer.ready, before);
    assert_eq!(producer.phase_epoch_us, 99);
    assert_eq!(producer.reserves, 1);
    assert_eq!(producer.publications, 1);
    assert_eq!(producer.cancellations, 0);
    assert_eq!(owner.retained(), Some(outcome));
}

#[test]
fn all_admission_failures_keep_their_reason_and_state() {
    for (error, reason) in [
        (GateError::Busy, RejectReason::OutputBusy),
        (GateError::NotReady, RejectReason::OutputUnavailable),
        (GateError::Quiescing, RejectReason::OutputUnavailable),
        (GateError::Faulted, RejectReason::OutputUnavailable),
    ] {
        let mut owner = Owner::new(initial(), 7, 3);
        let mut producer = Producer::new();
        producer.error = Some(error);
        let clock = Cell::new(90);
        let req = request(1, 2000, 100);
        let outcome = owner.apply(
            req,
            || clock.get(),
            |_, _, encoded| {
                clock.set(101);
                Period::try_decode(encoded).map_err(|_| RejectReason::Bounds)
            },
            &mut Gate {
                producer: Some(&mut producer),
                clock: &clock,
                reserve_at: None,
            },
        );
        assert_eq!(
            outcome,
            OwnerOutcome::Rejected {
                key: req.field.key,
                reason,
                active_values: initial(),
                active_revision: owner.active_revision()
            }
        );
        assert_eq!(owner.active(), initial());
        assert_eq!(owner.active_revision().get(), 0);
        assert_eq!(producer.phase_epoch_us, 17);
        assert!(producer.ready.is_none());
        assert!(!producer.reserved);
        assert_eq!(
            (
                producer.reserves,
                producer.publications,
                producer.cancellations
            ),
            (0, 0, 0)
        );
        assert_eq!(owner.retained(), Some(outcome));
    }
}

#[test]
fn initially_expired_and_revision_mismatch_skip_candidate_and_admission() {
    for (time, revision, reason) in [
        (100, 0, RejectReason::Expired),
        (101, 0, RejectReason::Expired),
        (90, 1, RejectReason::RevisionMismatch),
    ] {
        let mut owner = Owner::new(initial(), 7, 3);
        let mut producer = Producer::new();
        let mut req = request(1, 2000, 100);
        req.expected_revision = Some(revision);
        let outcome = owner.apply(
            req,
            || time,
            |_, _, _| panic!("must skip candidate"),
            &mut Gate {
                producer: Some(&mut producer),
                clock: &Cell::new(time),
                reserve_at: None,
            },
        );
        assert!(
            matches!(outcome, OwnerOutcome::Rejected { reason: actual, .. } if actual == reason)
        );
        assert_eq!(owner.active(), initial());
        assert_eq!(owner.active_revision().get(), 0);
        assert_eq!(producer.reserves, 0);
    }
}
