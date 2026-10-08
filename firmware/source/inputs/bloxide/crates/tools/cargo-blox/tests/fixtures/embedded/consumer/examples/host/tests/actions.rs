// Copyright 2025 Bloxide, all rights reserved
use bloxide_core::{LifecycleCommand, StateMachine};
use fixture_host::generated::lamp_spec_skeleton::FixtureLampSpec;
use fixture_lamp_blox::{FixtureLampCtx, FixtureLampEvent};
use fixture_messages::{LampMsg, SetLevel};
#[test]
fn imported_concrete_action_moves_producer_and_preserves_it_across_reset() {
    let (producer, consumer) = Box::leak(Box::new(fixture_context::Slot::new())).split();
    let mut machine = StateMachine::<FixtureLampSpec>::new(FixtureLampCtx::new(1, producer));
    machine.dispatch(FixtureLampEvent::Lifecycle(LifecycleCommand::Start));
    machine.dispatch(FixtureLampEvent::Msg(bloxide_core::Envelope(
        0,
        LampMsg::SetLevel(SetLevel { value: 73 }),
    )));
    assert_eq!(consumer.commanded(), 73);
    machine.dispatch(FixtureLampEvent::Lifecycle(LifecycleCommand::Reset));
    assert_eq!(consumer.commanded(), 73);
    machine.dispatch(FixtureLampEvent::Msg(bloxide_core::Envelope(
        0,
        LampMsg::SetLevel(SetLevel { value: 29 }),
    )));
    assert_eq!(consumer.commanded(), 29);
}
