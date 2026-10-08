#![no_std]
//! Generated AM13 LED platform with CAN XCP and verified calibration flash storage.
mod allocator;
mod calibration;
mod xcp;
mod mainboard;
use mainboard_messages::MainboardMsg;
use mainboard_characterization::{Owner,ReplySlot as BoardReplySlot};
use am13_rs::{Peripherals, clock::Clocks, io::Mmio, timer::SysTick, can};
use blox_ctx_xcp::SessionBridge;
use bloxide_core::messaging::ActorRef;
use bloxide_embassy::EmbassyRuntime;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration, Instant, Timer};
use led_context::{
    FaultStage, LedController, ReplyReader, ReplySlot, ScheduleConsumer, ScheduleSlot,
};
use led_messages::LedMsg;
use static_cell::StaticCell;
use xcp_core::VirtualMap;
use xcp_messages::XcpSessionMsg;

static TAKEN: AtomicBool = AtomicBool::new(false);
static LED_READY: AtomicBool = AtomicBool::new(false);
static LED_ON_STATE: AtomicBool = AtomicBool::new(false);
fn led_on() -> bool {LED_ON_STATE.load(Ordering::Acquire)}
const DIAGNOSTIC_BAUD: u32 = 19_200;

pub struct Resources {
    pub mainboard_owner: Owner,
    pub mainboard_tick: (),
    pub led_controller: LedController,
    pub led_service: LedService,
    pub transport: CanService,
    pub session_bridge: &'static SessionBridge,
    pub session_map: VirtualMap<'static>,
}
pub struct LedService {
    consumer: ScheduleConsumer,
    led: esc_som_board::Led<Mmio>,
    systick: SysTick<Mmio>,
    clocks: Clocks,
}
pub struct CanService {
    can: can::Can<Mmio>,
    reply: ReplyReader,
    bridge: &'static SessionBridge,
    mainboard_reply: &'static BoardReplySlot,
}
#[derive(Debug)]
pub enum StartupError {
    AlreadyTaken,
    Board(esc_som_board::peripherals::StartupError),
    Can(am13_rs::can::Error),
    Map,
}
fn clock_us() -> u64 {
    Instant::now().as_micros()
}

pub fn initialize() -> Result<Resources, StartupError> {
    if TAKEN.swap(true, Ordering::AcqRel) {
        return Err(StartupError::AlreadyTaken);
    }
    let device = Peripherals::take().ok_or(StartupError::AlreadyTaken)?;
    let esc_som_board::peripherals::Resources {
        led,
        mcan0,
        analog, pwm, drivers,
        systick,
        clocks,
        mut flash,
        ..
    } = esc_som_board::peripherals::initialize(device, DIAGNOSTIC_BAUD).map_err(StartupError::Board)?;
    let can = mcan0.configure(500_000).map_err(StartupError::Can)?;
    static BOARD: StaticCell<mainboard::Board> = StaticCell::new();
    static BOARD_REPLY: BoardReplySlot = BoardReplySlot::new();
    let mut mainboard_owner=Owner::new(BOARD.init(mainboard::Board{analog,pwm,drivers}),&BOARD_REPLY,led_on);
    static SLOT: StaticCell<ScheduleSlot> = StaticCell::new();
    let (output, consumer) = SLOT.init(ScheduleSlot::new()).split();
    static REPLY: StaticCell<ReplySlot> = StaticCell::new();
    let (writer, reader) = REPLY.init(ReplySlot::new()).split();
    static BRIDGE: StaticCell<SessionBridge> = StaticCell::new();
    let bridge = BRIDGE.init(SessionBridge::new());
    let map = VirtualMap::new(&mainboard_characterization::REGIONS).map_err(|_| StartupError::Map)?;
    let initial = calibration::initialize_with_defaults(&mut flash, mainboard_characterization::DEFAULT_CAL);
    assert!(mainboard_owner.control.restore_calibration(calibration::load_adc()));
    mainboard_owner.attach_calibration_stage(calibration::stage_adc);
    calibration::install(flash);
    let mut controller = LedController::new_with_config(output, clock_us, initial);
    controller.attach_calibration_store(calibration::store);
    controller.attach_reply(writer);
    Ok(Resources {
        mainboard_owner,
        mainboard_tick: (),
        led_controller: controller,
        led_service: LedService {
            consumer,
            led,
            systick,
            clocks,
        },
        transport: CanService {
            can,
            reply: reader,
            bridge,
            mainboard_reply: &BOARD_REPLY,
        },
        session_bridge: bridge,
        session_map: map,
    })
}
pub fn startup_failed(_: StartupError) -> ! {
    halt()
}
pub fn freeze() {
    allocator::freeze();
}
pub fn halt() -> ! {
    cortex_m::interrupt::disable();
    unsafe {esc_som_board::emergency_safe_off();}
    loop {
        core::hint::spin_loop();
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    halt()
}
#[cortex_m_rt::exception]
unsafe fn HardFault(_: &cortex_m_rt::ExceptionFrame) -> ! {
    halt()
}
#[cortex_m_rt::exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    halt()
}

pub fn start_led_owned(
    spawner: embassy_executor::Spawner,
    resource: LedService,
    owner: ActorRef<LedMsg, EmbassyRuntime>,
) -> Result<(), StartupError> {
    spawner.must_spawn(led_task(resource, owner));
    Ok(())
}
pub fn start_xcp_can(
    spawner: embassy_executor::Spawner,
    resource: CanService,
    mainboard: ActorRef<MainboardMsg, EmbassyRuntime>,
    owner: ActorRef<LedMsg, EmbassyRuntime>,
    session: ActorRef<XcpSessionMsg, EmbassyRuntime>,
) -> Result<(), StartupError> {
    spawner.must_spawn(xcp_can_task(resource, owner, session, mainboard));
    Ok(())
}
#[embassy_executor::task]
async fn led_task(mut r: LedService, owner: ActorRef<LedMsg, EmbassyRuntime>) {
    if !allocator::frozen() {
        halt();
    }
    r.consumer.start();
    r.systick.start(r.clocks);
    unsafe { cortex_m::interrupt::enable() };
    let mut active = None;
    let mut level = false;
    let mut faulted = false;
    let mut safe_off_confirmed = false;
    let mut notice: Option<LedMsg> = None;
    loop {
        let admission = r.consumer.status_reader().status().admission;
        if matches!(
            admission,
            led_context::Admission::Quiescing | led_context::Admission::Quiesced
        ) {
            // The supervised Owner requested Stop. Its output slot remains the
            // authority for queued command disposition; the service owns GPIO.
            active = None;
            if !safe_off_confirmed {
                safe_off_confirmed = r.led.set_on(false).is_ok();
                if safe_off_confirmed {LED_ON_STATE.store(false,Ordering::Release);}
            }
            if r.consumer.begin_quiesce(safe_off_confirmed)
                || admission == led_context::Admission::Quiesced
            {
                return;
            }
            Timer::after(Duration::from_millis(1)).await;
            continue;
        }
        if let Some(message) = notice {
            if owner.try_send(0, message).is_ok() {
                notice = None;
            }
        }
        if !faulted && let Some(schedule) = r.consumer.try_take() {
            active = Some(schedule);
            let on = led_context::phase_at(schedule.config, schedule.phase_epoch_us, clock_us()).0;
            if r.led.set_on(on).is_err() {
                r.consumer.report_fault(schedule, FaultStage::Installation);
                let safe = r.led.set_on(false).is_ok();
                if safe {LED_ON_STATE.store(false,Ordering::Release);}
                r.consumer.report_safe_off(safe);
                safe_off_confirmed = safe;
                faulted = true;
                active = None;
                notice = Some(LedMsg::OutputFaulted(schedule.sequence));
            } else {
                LED_ON_STATE.store(on, Ordering::Release);
                level = on;
                r.consumer.report_completed(schedule);
                notice = Some(LedMsg::OutputCompleted(schedule.sequence));
                if schedule.sequence == 0 {
                    LED_READY.store(true, Ordering::Release);
                }
            }
        }
        let mut delay_us = 1000;
        if let Some(schedule) = active {
            let now = clock_us();
            let (on, next_edge) =
                led_context::phase_at(schedule.config, schedule.phase_epoch_us, now);
            if !faulted && on != level {
                if r.led.set_on(on).is_err() {
                    r.consumer.report_fault(schedule, FaultStage::Edge);
                    notice = Some(LedMsg::OutputFaulted(schedule.sequence));
                    let safe = r.led.set_on(false).is_ok();
                if safe {LED_ON_STATE.store(false,Ordering::Release);}
                    r.consumer.report_safe_off(safe);
                    safe_off_confirmed = safe;
                    faulted = true;
                    active = None;
                } else {
                    LED_ON_STATE.store(on, Ordering::Release);
                level = on;
                }
            }
            if let Some(edge) = next_edge {
                delay_us = edge.saturating_sub(now).clamp(100, 1000);
            }
        }
        if faulted {
            // A full completion FIFO leaves Ready intact until the owner drains it.
            r.consumer.begin_quiesce(safe_off_confirmed);
            if r.consumer.accept_resume() {
                // Only an explicitly requested and acknowledged new epoch gets here.
                faulted = false;
                safe_off_confirmed = false;
                level = false;
            }
        }
        Timer::after(Duration::from_micros(delay_us)).await;
    }
}
#[embassy_executor::task]
async fn xcp_can_task(
    resource: CanService,
    owner: ActorRef<LedMsg, EmbassyRuntime>,
    session: ActorRef<XcpSessionMsg, EmbassyRuntime>,
    mainboard: ActorRef<MainboardMsg, EmbassyRuntime>,
) {
    if !allocator::frozen() {
        halt();
    }
    while !LED_READY.load(Ordering::Acquire) {
        Timer::after(Duration::from_millis(1)).await;
    }
    xcp::run(
        resource.can,
        resource.reply,
        owner,
        session,
        resource.bridge,
        mainboard,
        resource.mainboard_reply,
    )
    .await;
    halt();
}

pub fn start_mainboard_ticks(spawner:embassy_executor::Spawner,_resource:(),owner:ActorRef<MainboardMsg,EmbassyRuntime>)->Result<(),StartupError>{spawner.must_spawn(mainboard_tick_task(owner));Ok(())}
#[embassy_executor::task]
async fn mainboard_tick_task(owner:ActorRef<MainboardMsg,EmbassyRuntime>){
 while !LED_READY.load(Ordering::Acquire){Timer::after(Duration::from_millis(1)).await;}
 loop {let _=owner.try_send(0,MainboardMsg::Tick(mainboard_messages::Tick{now_us:clock_us()}));Timer::after(Duration::from_millis(1)).await;}
}
