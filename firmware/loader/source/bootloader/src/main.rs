#![no_std]
#![no_main]
mod hardware;
use am13_can_boot_core::{Session,Flash,APP};
use am13_rs::gpio::Pull;
use esc_som_board::pins;
use core::ptr::{read_volatile,write_volatile};
#[unsafe(no_mangle)]
pub static mut BOOT_DIAG:[u32;8]=[0;8];
fn diag(n:usize,v:u32) {unsafe{write_volatile(core::ptr::addr_of_mut!(BOOT_DIAG).cast::<u32>().add(n),v)}}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo)->! {diag(0,0xdead);loop {cortex_m::asm::wfi()}}
#[cortex_m_rt::entry]
fn main()->! {
    diag(0,1);
    let device=am13_rs::Peripherals::take().unwrap();
    let am13_rs::Peripherals{mut gpio,clocks,mcan0,..}=device;
    gpio.power().unwrap();
    for pin in [pins::DRV_ENABLE,pins::INL_ENABLE].into_iter().chain(pins::MOTOR_OUTPUTS) {gpio.output(pin,false).unwrap();}
    for pin in pins::DRIVER_CS {gpio.output(pin,true).unwrap();}
    gpio.output(pins::STATUS_LED,true).unwrap();
    gpio.input(pins::BSL_INVOKE,Pull::Down).unwrap();
    gpio.alternate(pins::BSL_CAN_TX,10,false,Pull::None).unwrap();
    gpio.alternate(pins::BSL_CAN_RX,10,true,Pull::None).unwrap();
    clocks.sysosc_32mhz().unwrap();diag(0,2);
    let mut can=match mcan0.configure_boot_fd() {Ok(c)=>c,Err(_)=>{diag(0,0xcafe);loop{cortex_m::asm::wfi()}}};
    diag(0,3);
    let mut flash=hardware::Storage::new();let valid=flash.valid().is_some();diag(1,valid as u32);
    let mut session=Session::new();let mut held=false;
    unsafe {
        write_volatile(0xe000e014 as *mut u32,31_999);
        write_volatile(0xe000e018 as *mut u32,0);
        write_volatile(0xe000e010 as *mut u32,5); //1ms polling, no SysTick IRQ
    }
    let mut ms=0;diag(0,4);
    loop {
        if unsafe{read_volatile(0xe000e010 as *const u32)}&(1<<16)!=0 {ms+=1;}
        if let Ok(Some(frame))=can.receive() {
            if frame.id==0x710 {
                if let Some(r)=session.command(&frame.data[..frame.len],&mut flash) {
                    if session.connected {held=true;}
                    diag(2,session.phase as u32);diag(3,flash.last_status);
                    let sent=can.send(0x711,&r.bytes[..r.len]).is_ok();
                    if r.reset && sent {cortex_m::peripheral::SCB::sys_reset()}
                }
            }
        }
        if valid && !held && ms>=3000 {
            // Recheck immediately before jumping; no session ever held this boot.
            if flash.valid().is_some() {
                let mut v=[0;8];flash.read(APP,&mut v).unwrap();
                let sp=u32::from_le_bytes(v[..4].try_into().unwrap());let pc=u32::from_le_bytes(v[4..].try_into().unwrap());
                diag(0,5);can.stop();
                unsafe {
                    write_volatile(0xe000e010 as *mut u32,0);
                    for bank in 0..8 {write_volatile((0xe000e180usize+bank*4) as *mut u32,u32::MAX);write_volatile((0xe000e280usize+bank*4) as *mut u32,u32::MAX);}
                    write_volatile(0xe000ed04 as *mut u32,(1<<25)|(1<<27));
                    write_volatile(0xe000ed08 as *mut u32,APP);
                    core::arch::asm!("dsb","isb","msr control, r2","msr basepri, r2","msr faultmask, r2","msr msplim, r2","msr msp, {sp}","bx {pc}",sp=in(reg)sp,pc=in(reg)pc,in("r2")0u32,options(noreturn));
                }
            }
            held=true;
        }
    }
}
