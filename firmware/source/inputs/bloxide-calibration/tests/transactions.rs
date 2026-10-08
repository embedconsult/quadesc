use bloxide_calibration::{
    ApplyField, ApplyRequest, BoundedI32, BoundedU16, Celsius, CommitGate, CommitRecord,
    EncodedValue, GateError, Milliseconds, OperationKey, Owner, OwnerMode, OwnerOutcome, Permille,
    RejectReason, ResetError,
};

type PeriodMs = BoundedU16<Milliseconds, 100, 10_000>;
type DutyPermille = BoundedU16<Permille, 0, 1000>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LedConfig {
    period: PeriodMs,
    duty: DutyPermille,
}

#[derive(Default)]
struct LedGate {
    admission_error: Option<GateError>,
    publications: u32,
    phase_epoch_us: u64,
    admitted_revision: Option<u32>,
    commanded_on: bool,
    physical_fault: bool,
}

impl CommitGate<LedConfig> for LedGate {
    type Permit = ();

    fn try_reserve(&mut self, _next: &LedConfig) -> Result<Self::Permit, GateError> {
        self.admission_error.map_or(Ok(()), Err)
    }

    fn publish(&mut self, (): Self::Permit, record: CommitRecord<LedConfig>) {
        self.publications += 1;
        self.phase_epoch_us = record.applied_at_us;
        self.admitted_revision = Some(record.active_revision.get());
        self.commanded_on = record.active_values.duty.get() != 0;
        // A physical fault is status owned by the output side. Publication is
        // still complete and cannot return a calibration rejection.
    }
}

fn initial_led() -> LedConfig {
    LedConfig {
        period: PeriodMs::try_new(1000).unwrap(),
        duty: DutyPermille::try_new(500).unwrap(),
    }
}

fn key(sequence: u64) -> OperationKey {
    OperationKey {
        service_epoch: 7,
        session_generation: 3,
        sequence,
    }
}

fn request(key: OperationKey, field_id: u32, bytes: &[u8], expires_at_us: u64) -> ApplyRequest {
    ApplyRequest {
        field: ApplyField {
            key,
            field_id,
            encoded_value: EncodedValue::try_from_slice(bytes).unwrap(),
            expires_at_us,
        },
        expected_revision: None,
    }
}

fn patch_led(
    current: LedConfig,
    field_id: u32,
    encoded: EncodedValue,
) -> Result<LedConfig, RejectReason> {
    match field_id {
        1 => Ok(LedConfig {
            period: PeriodMs::try_decode(encoded).map_err(|_| RejectReason::Bounds)?,
            ..current
        }),
        2 => Ok(LedConfig {
            duty: DutyPermille::try_decode(encoded).map_err(|_| RejectReason::Bounds)?,
            ..current
        }),
        _ => Err(RejectReason::UnknownVariable),
    }
}

#[test]
fn invalid_partial_expired_and_busy_admission_never_mutate() {
    let mut owner = Owner::new(initial_led(), 7, 3);
    let mut gate = LedGate::default();
    let before = owner.active();

    let invalid = owner.apply(
        request(key(1), 1, &[99, 0], 100),
        || 10,
        patch_led,
        &mut gate,
    );
    assert!(matches!(
        invalid,
        OwnerOutcome::Rejected {
            reason: RejectReason::Bounds,
            ..
        }
    ));
    assert_eq!(owner.active(), before);
    assert_eq!(owner.active_revision().get(), 0);
    owner.release(key(1)).unwrap();

    let partial = owner.apply(request(key(2), 1, &[100], 100), || 10, patch_led, &mut gate);
    assert!(matches!(
        partial,
        OwnerOutcome::Rejected {
            reason: RejectReason::Bounds,
            ..
        }
    ));
    assert_eq!(owner.active(), before);
    owner.release(key(2)).unwrap();

    let expired = owner.apply(
        request(key(3), 1, &2000_u16.to_le_bytes(), 10),
        || 10,
        patch_led,
        &mut gate,
    );
    assert!(matches!(
        expired,
        OwnerOutcome::Rejected {
            reason: RejectReason::Expired,
            ..
        }
    ));
    assert_eq!(owner.active(), before);
    owner.release(key(3)).unwrap();

    gate.admission_error = Some(GateError::Busy);
    let busy = owner.apply(
        request(key(4), 1, &2000_u16.to_le_bytes(), 100),
        || 10,
        patch_led,
        &mut gate,
    );
    assert!(matches!(
        busy,
        OwnerOutcome::Rejected {
            reason: RejectReason::OutputBusy,
            ..
        }
    ));
    assert_eq!(owner.active(), before);
    assert_eq!(gate.publications, 0);
}

#[test]
fn changed_and_same_value_semantics_separate_physical_failure() {
    let mut owner = Owner::new(initial_led(), 7, 3);
    let mut gate = LedGate {
        physical_fault: true,
        ..LedGate::default()
    };
    let applied = owner.apply(
        request(key(1), 1, &2000_u16.to_le_bytes(), 100),
        || 20,
        patch_led,
        &mut gate,
    );
    assert!(matches!(
        applied,
        OwnerOutcome::Applied { changed: true, .. }
    ));
    assert_eq!(owner.active().period.get(), 2000);
    assert_eq!(owner.active_revision().get(), 1);
    assert_eq!(gate.publications, 1);
    assert_eq!(gate.phase_epoch_us, 20);
    assert_eq!(gate.admitted_revision, Some(1));
    assert!(gate.commanded_on);
    assert!(gate.physical_fault);
    assert_eq!(owner.retained(), Some(applied));
    owner.release(key(1)).unwrap();

    gate.admission_error = Some(GateError::Faulted);
    let same = owner.apply(
        request(key(2), 1, &2000_u16.to_le_bytes(), 100),
        || 30,
        patch_led,
        &mut gate,
    );
    assert!(matches!(same, OwnerOutcome::Applied { changed: false, .. }));
    assert_eq!(owner.active_revision().get(), 1);
    assert_eq!(gate.publications, 1);
    assert_eq!(gate.phase_epoch_us, 20);
    assert_eq!(gate.admitted_revision, Some(1));
}

#[test]
fn retired_sequence_fences_delayed_a_after_b() {
    let mut owner = Owner::new(initial_led(), 7, 3);
    let mut gate = LedGate::default();
    let request_a = request(key(10), 1, &2000_u16.to_le_bytes(), 100);
    let applied_a = owner.apply(request_a, || 10, patch_led, &mut gate);
    assert!(matches!(applied_a, OwnerOutcome::Applied { .. }));
    assert_eq!(
        owner.apply(request_a, || 11, patch_led, &mut gate),
        applied_a
    );
    owner.release(key(10)).unwrap();

    let applied_b = owner.apply(
        request(key(11), 1, &3000_u16.to_le_bytes(), 100),
        || 12,
        patch_led,
        &mut gate,
    );
    assert!(matches!(applied_b, OwnerOutcome::Applied { .. }));
    assert_eq!(owner.active().period.get(), 3000);

    let blocked_c = owner.apply(
        request(key(12), 1, &4000_u16.to_le_bytes(), 100),
        || 13,
        patch_led,
        &mut gate,
    );
    assert!(matches!(blocked_c, OwnerOutcome::Busy { .. }));
    assert_eq!(owner.active().period.get(), 3000);

    let delayed_a = owner.apply(request_a, || 13, patch_led, &mut gate);
    assert!(matches!(delayed_a, OwnerOutcome::Retired { .. }));
    assert_eq!(owner.active().period.get(), 3000);
    assert_eq!(gate.publications, 2);
}

#[test]
fn cancellation_tombstone_prevents_late_apply() {
    let mut owner = Owner::new(initial_led(), 7, 3);
    let mut gate = LedGate::default();
    let cancelled = owner.resolve_or_cancel(key(20));
    assert!(matches!(cancelled, OwnerOutcome::Cancelled { .. }));
    let late = owner.apply(
        request(key(20), 1, &9000_u16.to_le_bytes(), 100),
        || 10,
        patch_led,
        &mut gate,
    );
    assert_eq!(late, cancelled);
    assert_eq!(owner.active(), initial_led());
    assert_eq!(gate.publications, 0);
}

#[test]
fn coordinated_reset_preserves_committed_outcome_until_reconciled() {
    let mut owner = Owner::new(initial_led(), 7, 3);
    let mut gate = LedGate::default();
    let applied = owner.apply(
        request(key(1), 2, &750_u16.to_le_bytes(), 100),
        || 10,
        patch_led,
        &mut gate,
    );
    let next_epoch = owner.begin_coordinated_reset().unwrap();
    assert_eq!(next_epoch, 8);
    assert_eq!(
        owner.mode(),
        OwnerMode::Reconciling {
            next_service_epoch: 8
        }
    );
    assert_eq!(owner.retained(), Some(applied));
    assert_eq!(owner.active().duty.get(), 750);
    assert_eq!(
        owner.finish_coordinated_reset(4),
        Err(ResetError::OutstandingOutcome)
    );
    assert_eq!(owner.resolve_or_cancel(key(1)), applied);

    owner.release(key(1)).unwrap();
    owner.finish_coordinated_reset(4).unwrap();
    assert_eq!(owner.service_epoch(), 8);
    assert_eq!(owner.session_generation(), 4);
    assert_eq!(owner.active().duty.get(), 750);

    let stale = owner.apply(
        request(key(2), 2, &250_u16.to_le_bytes(), 200),
        || 20,
        patch_led,
        &mut gate,
    );
    assert!(matches!(stale, OwnerOutcome::StaleOperation { .. }));
    assert_eq!(owner.active().duty.get(), 750);
}

type Temperature = BoundedI32<Celsius, -4000, 12_500>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThermalLimits {
    low: Temperature,
    high: Temperature,
}

#[derive(Default)]
struct ImmediateGate;

impl CommitGate<ThermalLimits> for ImmediateGate {
    type Permit = ();

    fn try_reserve(&mut self, _next: &ThermalLimits) -> Result<(), GateError> {
        Ok(())
    }

    fn publish(&mut self, (): (), _record: CommitRecord<ThermalLimits>) {}
}

fn patch_thermal(
    current: ThermalLimits,
    field_id: u32,
    encoded: EncodedValue,
) -> Result<ThermalLimits, RejectReason> {
    let value = Temperature::try_decode(encoded).map_err(|_| RejectReason::Bounds)?;
    let next = match field_id {
        1 => ThermalLimits {
            low: value,
            ..current
        },
        2 => ThermalLimits {
            high: value,
            ..current
        },
        _ => return Err(RejectReason::UnknownVariable),
    };
    if next.low.get() > next.high.get() {
        return Err(RejectReason::CrossField);
    }
    Ok(next)
}

#[test]
fn unrelated_consumer_cross_field_rejection_is_atomic() {
    let initial = ThermalLimits {
        low: Temperature::try_new(1000).unwrap(),
        high: Temperature::try_new(5000).unwrap(),
    };
    let mut owner = Owner::new(initial, 7, 3);
    let mut gate = ImmediateGate;
    let outcome = owner.apply(
        request(key(1), 1, &6000_i32.to_le_bytes(), 100),
        || 10,
        patch_thermal,
        &mut gate,
    );
    assert!(matches!(
        outcome,
        OwnerOutcome::Rejected {
            reason: RejectReason::CrossField,
            ..
        }
    ));
    assert_eq!(owner.active(), initial);
    assert_eq!(owner.active_revision().get(), 0);
}
