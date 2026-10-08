use bloxide_core::{lifecycle::LifecycleCommand, MachineState, StateMachine};
use xcp_core::{Region, VirtualMap};
use blox_ctx_xcp::SessionBridge;
use xcp_session_blox::{XcpSessionCtx, XcpSessionEvent, XcpSessionSpec, XcpSessionState};

static REGIONS: [Region; 1] = [Region::read_only(1, 0, 4)];
static BRIDGE: SessionBridge = SessionBridge::new();

#[test]
fn start_enters_disconnected_without_reinitializing_owned_session_state() {
    let map = VirtualMap::new(&REGIONS).unwrap();
    let ctx = XcpSessionCtx::new(bloxide_core::next_actor_id!(), map, &BRIDGE);
    let mut machine = StateMachine::<XcpSessionSpec>::new(ctx);
    machine.dispatch(XcpSessionEvent::Lifecycle(LifecycleCommand::Start));
    assert_eq!(
        machine.current_state(),
        MachineState::State(XcpSessionState::Disconnected)
    );
}
