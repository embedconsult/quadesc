use crate::{Classification, OperationKey, Record, Schema, Slot, Snapshot, SnapshotError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefaultsReason(u16);

impl DefaultsReason {
    pub const EMPTY_A: Self = Self(1 << 0);
    pub const EMPTY_B: Self = Self(1 << 1);
    pub const UNCOMMITTED_A: Self = Self(1 << 2);
    pub const UNCOMMITTED_B: Self = Self(1 << 3);
    pub const CORRUPT_A: Self = Self(1 << 4);
    pub const CORRUPT_B: Self = Self(1 << 5);
    pub const UNREADABLE_A: Self = Self(1 << 6);
    pub const UNREADABLE_B: Self = Self(1 << 7);
    pub const AMBIGUOUS: Self = Self(1 << 8);
    pub const UNSUPPORTED: Self = Self(1 << 9);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }

    const fn insert(&mut self, flag: Self) {
        self.0 |= flag.0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedRecord {
    pub slot: Slot,
    pub record: Record,
    pub duplicate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritePolicy {
    AutomaticBlank,
    RequiresRecoveryAuthorization,
    Locked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDisposition {
    Selected(SelectedRecord),
    Defaults {
        snapshot: Snapshot,
        reasons: DefaultsReason,
    },
    NoConfiguration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recovery {
    pub a: Classification,
    pub b: Classification,
    pub disposition: RecoveryDisposition,
    pub write_policy: WritePolicy,
}

impl Recovery {
    #[must_use]
    pub const fn selected(self) -> Option<SelectedRecord> {
        match self.disposition {
            RecoveryDisposition::Selected(selected) => Some(selected),
            RecoveryDisposition::Defaults { .. } | RecoveryDisposition::NoConfiguration => None,
        }
    }

    #[must_use]
    pub const fn target_slot(self) -> Slot {
        match self.selected() {
            Some(selected) => selected.slot.other(),
            None => Slot::A,
        }
    }

    #[must_use]
    pub const fn target_classification(self) -> Classification {
        match self.target_slot() {
            Slot::A => self.a,
            Slot::B => self.b,
        }
    }

    pub fn plan(
        self,
        snapshot: Snapshot,
        recovery_authorized: bool,
    ) -> Result<SavePlan, RecoveryError> {
        if let Some(selected) = self.selected()
            && selected.record.is_same_durable_snapshot(&snapshot)
        {
            return Ok(SavePlan::DurableExisting(selected));
        }
        match self.write_policy {
            WritePolicy::Locked => return Err(RecoveryError::WritesLocked),
            WritePolicy::RequiresRecoveryAuthorization if !recovery_authorized => {
                return Err(RecoveryError::RecoveryAuthorizationRequired);
            }
            WritePolicy::AutomaticBlank | WritePolicy::RequiresRecoveryAuthorization => {}
        }
        let sequence = match self.selected() {
            Some(selected) => selected
                .record
                .sequence
                .checked_add(1)
                .ok_or(RecoveryError::SequenceExhausted)?,
            None => 1,
        };
        Ok(SavePlan::Write {
            target: self.target_slot(),
            record: Record::new(sequence, snapshot),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SavePlan {
    DurableExisting(SelectedRecord),
    Write { target: Slot, record: Record },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryError {
    InvalidDefaults,
    WritesLocked,
    RecoveryAuthorizationRequired,
    SequenceExhausted,
}

fn reason_for(classification: Classification, slot: Slot, reasons: &mut DefaultsReason) {
    let flag = match (classification, slot) {
        (Classification::Empty, Slot::A) => DefaultsReason::EMPTY_A,
        (Classification::Empty, Slot::B) => DefaultsReason::EMPTY_B,
        (Classification::Uncommitted, Slot::A) => DefaultsReason::UNCOMMITTED_A,
        (Classification::Uncommitted, Slot::B) => DefaultsReason::UNCOMMITTED_B,
        (Classification::Corrupt(_), Slot::A) => DefaultsReason::CORRUPT_A,
        (Classification::Corrupt(_), Slot::B) => DefaultsReason::CORRUPT_B,
        (Classification::Unreadable(_), Slot::A) => DefaultsReason::UNREADABLE_A,
        (Classification::Unreadable(_), Slot::B) => DefaultsReason::UNREADABLE_B,
        (Classification::Unsupported(_) | Classification::Valid(_), _) => return,
    };
    reasons.insert(flag);
}

pub fn recover<S: Schema>(
    a: Classification,
    b: Classification,
    schema: &S,
) -> Result<Recovery, RecoveryError> {
    let default_snapshot = Snapshot::defaults(
        schema,
        0,
        OperationKey {
            service_epoch: 0,
            session_generation: 0,
            sequence: 0,
        },
    )
    .map_err(|_: SnapshotError| RecoveryError::InvalidDefaults)?;

    if matches!(a, Classification::Unsupported(_)) || matches!(b, Classification::Unsupported(_)) {
        return Ok(Recovery {
            a,
            b,
            disposition: RecoveryDisposition::Defaults {
                snapshot: default_snapshot,
                reasons: DefaultsReason::UNSUPPORTED,
            },
            write_policy: WritePolicy::Locked,
        });
    }

    let selected = match (a, b) {
        (Classification::Valid(a_record), Classification::Valid(b_record)) => {
            if a_record.sequence > b_record.sequence {
                Some(SelectedRecord {
                    slot: Slot::A,
                    record: a_record,
                    duplicate: false,
                })
            } else if b_record.sequence > a_record.sequence {
                Some(SelectedRecord {
                    slot: Slot::B,
                    record: b_record,
                    duplicate: false,
                })
            } else if a_record.persistent_identity_eq(&b_record) {
                Some(SelectedRecord {
                    slot: Slot::A,
                    record: a_record,
                    duplicate: true,
                })
            } else {
                return Ok(Recovery {
                    a,
                    b,
                    disposition: RecoveryDisposition::Defaults {
                        snapshot: default_snapshot,
                        reasons: DefaultsReason::AMBIGUOUS,
                    },
                    write_policy: WritePolicy::Locked,
                });
            }
        }
        (Classification::Valid(record), _) => Some(SelectedRecord {
            slot: Slot::A,
            record,
            duplicate: false,
        }),
        (_, Classification::Valid(record)) => Some(SelectedRecord {
            slot: Slot::B,
            record,
            duplicate: false,
        }),
        _ => None,
    };

    if let Some(selected) = selected {
        return Ok(Recovery {
            a,
            b,
            disposition: RecoveryDisposition::Selected(selected),
            write_policy: WritePolicy::RequiresRecoveryAuthorization,
        });
    }

    let mut reasons = DefaultsReason::empty();
    reason_for(a, Slot::A, &mut reasons);
    reason_for(b, Slot::B, &mut reasons);
    let write_policy = if matches!((a, b), (Classification::Empty, Classification::Empty)) {
        WritePolicy::AutomaticBlank
    } else {
        WritePolicy::RequiresRecoveryAuthorization
    };
    Ok(Recovery {
        a,
        b,
        disposition: RecoveryDisposition::Defaults {
            snapshot: default_snapshot,
            reasons,
        },
        write_policy,
    })
}
