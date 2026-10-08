use am13_rs::timer::*;
#[test]
fn coalescing_cancellation_and_saturation_preserve_existing_deadlines() {
    let mut q = Deadlines::<u8, 2>::new();
    q.schedule(1, 100).unwrap();
    q.schedule(1, 300).unwrap();
    q.schedule(2, 200).unwrap();
    assert_eq!(q.schedule(3, 1), Err(CapacityError));
    assert_eq!(q.expire(99), [None, None]);
    assert_eq!(q.expire(100), [Some(1), None]);
    // A dropped/cancelled timer can cause an early wake; the live future re-registers.
    q.schedule(1, 300).unwrap();
    q.schedule(1, 250).unwrap();
    assert_eq!(q.expire(200), [None, Some(2)]);
    assert_eq!(q.expire(250), [Some(1), None]);
    assert_eq!(q.expire(u64::MAX), [None, None]);
}
#[test]
fn zero_capacity_and_u32_carry_and_u64_endpoint() {
    assert_eq!(Deadlines::<u8, 0>::new().schedule(1, 0), Err(CapacityError));
    let mut q = Deadlines::<u8, 2>::new();
    let edge = u32::MAX as u64 + 1;
    q.schedule(1, edge).unwrap();
    q.schedule(2, u64::MAX).unwrap();
    assert_eq!(q.expire(edge - 1), [None, None]);
    assert_eq!(q.expire(edge), [Some(1), None]);
    assert_eq!(q.expire(u64::MAX), [None, Some(2)]);
}
#[test]
fn exported_units_are_explicit_checked_and_rounded_up() {
    assert_eq!(ticks_to_micros(10_000), Some(1_000_000));
    assert_eq!(ticks_to_micros(u64::MAX), None);
    for (us, ticks) in [(0, 0), (1, 1), (99, 1), (100, 1), (101, 2)] {
        assert_eq!(micros_to_ticks_ceil(us), ticks);
    }
    assert_eq!(micros_to_ticks_ceil(u64::MAX), u64::MAX / 100 + 1);
}
