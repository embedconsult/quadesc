//! Classical CAN CTO framing (0x700 requests, 0x701 replies) for the generated XcpSession and LedOwner.
//! The bridge and fixed reply slot carry one request and result at a time.
use am13_rs::{io::Mmio, can::{Can, Frame}};
use blox_ctx_xcp::SessionBridge;
use bloxide_core::messaging::ActorRef;
use bloxide_embassy::EmbassyRuntime;
use embassy_time::{Duration, Instant, Timer};
use led_context::ReplyReader;
use led_messages::{LedMsg, LedResult, XcpProviderReply};
use xcp_core::ProviderRequest;
use xcp_messages::{ProviderCompletion, ServiceRequest, XcpSessionMsg};
use xcp_core::Packet;
pub const REQUEST_ID: u16 = 0x700;
pub const RESPONSE_ID: u16 = 0x701;

use mainboard_messages::{MainboardMsg,Provider};
use mainboard_characterization::{ReplySlot,RebootAfterAck};
struct Transport {
    mainboard: ActorRef<MainboardMsg,EmbassyRuntime>,
    board_reply: &'static ReplySlot,
    board_write: bool,
    reboot: RebootAfterAck,
    bridge: &'static SessionBridge,
    in_flight: Option<ProviderRequest>,
    completion: Option<ProviderCompletion>,
}
impl Transport {
    fn new(bridge: &'static SessionBridge,mainboard:ActorRef<MainboardMsg,EmbassyRuntime>,board_reply:&'static ReplySlot) -> Self {
        Self {
            bridge, mainboard, board_reply, board_write:false,reboot:RebootAfterAck::default(),
            in_flight: None,
            completion: None,
        }
    }
    fn settled(&self) -> bool {
        self.bridge.loss_acknowledged()
            && self.bridge.settled()
            && self.bridge.effect().is_none()
            && self.in_flight.is_none()
            && self.completion.is_none()
    }
}

fn now() -> u64 {
    Instant::now().as_micros()
}
fn send<M: Copy>(to: &ActorRef<M, EmbassyRuntime>, msg: M) -> Result<(), ()>
where
    M: Send + 'static,
{
    to.try_send(0, msg).map_err(|_| ())
}
async fn send_bounded<M: Copy + Send + 'static>(
    to: &ActorRef<M, EmbassyRuntime>,
    msg: M,
) -> Result<(), ()> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if send(to, msg).is_ok() {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(());
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}
fn matches_completion(request: ProviderRequest, completion: ProviderCompletion) -> bool {
    match (request, completion) {
        (ProviderRequest::StoreCalibration { correlation: a }, ProviderCompletion::CalibrationStored { correlation: b, .. }) => a == b,
        (
            ProviderRequest::Synchronize {
                session_generation: a,
            },
            ProviderCompletion::Synchronized {
                session_generation: b,
                ..
            },
        ) => a == b,
        (ProviderRequest::Read(r), ProviderCompletion::Read { correlation, .. }) => {
            r.correlation == correlation
        }
        (ProviderRequest::Apply(r), ProviderCompletion::Write { correlation, .. }) => {
            r.operation.correlation() == correlation
        }
        (
            ProviderRequest::ResolveOrCancel { operation },
            ProviderCompletion::Write { correlation, .. },
        ) => operation.correlation() == correlation,
        (
            ProviderRequest::ReleaseOutcome { operation },
            ProviderCompletion::Released { correlation },
        ) => operation.correlation() == correlation,
        (
            ProviderRequest::Quiesce { correlation: a },
            ProviderCompletion::Quiesced { correlation: b, .. },
        ) => a == b,
        _ => false,
    }
}
async fn forward_effect(
    transport: &mut Transport,
    reply: &mut ReplyReader,
    owner: &ActorRef<LedMsg, EmbassyRuntime>,
    session: &ActorRef<XcpSessionMsg, EmbassyRuntime>,
) -> Result<bool, ()> {
    if transport.in_flight.is_none() && transport.completion.is_none() {
        let Some(effect) = transport.bridge.effect() else {
            return Ok(false);
        };
        let local=match effect {
            ProviderRequest::Read(r)=>r.descriptor_id>=3,
            ProviderRequest::Apply(w)=>{transport.board_write=w.descriptor_id>=3;transport.board_write},
            ProviderRequest::ResolveOrCancel{..}|ProviderRequest::ReleaseOutcome{..}=>transport.board_write,
            _=>false,
        };
        let preflight=matches!(effect,ProviderRequest::Synchronize{..}|ProviderRequest::Quiesce{..}|ProviderRequest::StoreCalibration{..});
        if local || preflight {
            send_bounded(&transport.mainboard,MainboardMsg::Provider(Provider{request:effect,now_us:now()})).await?;
            let deadline=Instant::now()+Duration::from_secs(1);
            let completed=loop {if let Some(c)=transport.board_reply.take(){break c;} if Instant::now()>=deadline{return Err(());} Timer::after(Duration::from_millis(1)).await;};
            if !matches_completion(effect,completed){return Err(());}
            let refused=mainboard_characterization::completion_requires_fence(completed);
            if local || refused {
                transport.reboot.observe(effect,completed);
                if !transport.bridge.accept_effect(effect){return Err(());}
                send_bounded(session,XcpSessionMsg::ProviderCompletion(completed)).await?;
                return Ok(true);
            }
        }
        send_bounded(owner, LedMsg::XcpProvider(effect)).await?;
        transport.in_flight = Some(effect);
        if !transport.bridge.accept_effect(effect) {
            return Err(());
        }
    }
    if transport.completion.is_none() {
        let effect = transport.in_flight.ok_or(())?;
        let until = Instant::now() + Duration::from_secs(5);
        let completion = loop {
            match reply.take() {
                Ok(Some(LedResult::XcpProvider(XcpProviderReply::Completion(c))))
                    if matches_completion(effect, c) =>
                {
                    break c;
                }
                Ok(None) if Instant::now() < until => Timer::after(Duration::from_millis(1)).await,
                _ => return Err(()),
            }
        };
        transport.completion = Some(completion);
    }
    let completion = transport.completion.ok_or(())?;
    send_bounded(session, XcpSessionMsg::ProviderCompletion(completion)).await?;
    transport.completion = None;
    transport.in_flight = None;
    Ok(true)
}
async fn exchange(
    can: &mut Can<Mmio>,
    transport: &mut Transport,
    reply: &mut ReplyReader,
    owner: &ActorRef<LedMsg, EmbassyRuntime>,
    session: &ActorRef<XcpSessionMsg, EmbassyRuntime>,
) -> Result<(), ()> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if forward_effect(transport, reply, owner, session).await? {
            continue;
        }
        if let Some(packet) = transport.bridge.response() {
            // Can::send waits for fresh IR.TC AND TXBTO; queueing alone is not an ACK.
            let sent=can.send(Frame::new(RESPONSE_ID, packet.as_slice()).map_err(|_| ())?).is_ok();
            if transport.reboot.response_sent(packet.as_slice(),sent) {
                // The matching action already stopped PWM and lowered both enables.
                cortex_m::peripheral::SCB::sys_reset();
            }
            if !sent {return Err(());}

            if !transport.bridge.accept_response(packet) {
                return Err(());
            }
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(());
        }
        let _ = send(
            session,
            XcpSessionMsg::Service(ServiceRequest { now_us: now() }),
        );
        Timer::after(Duration::from_millis(1)).await;
    }
}
async fn lost(
    transport: &mut Transport,
    reply: &mut ReplyReader,
    owner: &ActorRef<LedMsg, EmbassyRuntime>,
    session: &ActorRef<XcpSessionMsg, EmbassyRuntime>,
) {
    if send_bounded(session, XcpSessionMsg::TransportLost)
        .await
        .is_err()
    {
        return;
    }
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if transport.settled() {
            let _ = send_bounded(session, XcpSessionMsg::TransportSettled).await;
            return;
        }
        if (transport.bridge.effect().is_some()
            || transport.in_flight.is_some()
            || transport.completion.is_some())
            && forward_effect(transport, reply, owner, session)
                .await
                .is_err()
        {
            return;
        }
        let _ = send(
            session,
            XcpSessionMsg::Service(ServiceRequest { now_us: now() }),
        );
        Timer::after(Duration::from_millis(1)).await;
    }
}
pub async fn run(
    mut can: Can<Mmio>,
    mut reply: ReplyReader,
    owner: ActorRef<LedMsg, EmbassyRuntime>,
    session: ActorRef<XcpSessionMsg, EmbassyRuntime>,
    bridge: &'static SessionBridge,
    mainboard:ActorRef<MainboardMsg,EmbassyRuntime>,
    board_reply:&'static ReplySlot,
) {
    let mut transport = Transport::new(bridge,mainboard,board_reply);
    loop {
        match can.receive() {
            Ok(Some(frame)) if frame.id == REQUEST_ID && frame.len > 0 => {
                let Ok(packet) = Packet::try_from_slice(&frame.data[..frame.len as usize]) else { continue; };
                if send_bounded(&session, XcpSessionMsg::Packet(xcp_messages::PacketRequest {
                    packet, now_us: now(),
                })).await.is_err()
                    || exchange(&mut can, &mut transport, &mut reply, &owner, &session).await.is_err()
                { break; }
            }
            Ok(_) | Err(am13_rs::can::Error::UnsupportedFrame) => {}
            Err(_) => break,
        }
        if bridge.effect().is_some() || !bridge.settled()
            || transport.in_flight.is_some() || transport.completion.is_some()
        {
            if forward_effect(&mut transport, &mut reply, &owner, &session).await.is_err() { break; }
            let _ = send(&session, XcpSessionMsg::Service(ServiceRequest { now_us: now() }));
        }
        Timer::after(Duration::from_millis(1)).await;
    }
    let _=send_bounded(&transport.mainboard,MainboardMsg::TransportLost).await;
    lost(&mut transport, &mut reply, &owner, &session).await;
}
