#[path="../../mcu-xcp-platform/src/mainboard.rs"] mod mainboard;
#[path="../../../inputs/esc-som-board/tests/support/mod.rs"] mod support;
use mainboard_characterization::*;
use esc_som_board::{peripherals,pins};
use am13_rs::{Peripherals,io::RegisterIo};
use support::Model;
#[derive(Clone,Default)]struct Io{m:Model,stalled:bool}
impl RegisterIo for Io {
 fn read(&self,a:usize)->u32{
  let v=self.m.read(a);
  if !self.stalled && (0..4).any(|m|a==0x40010028+m*0x1000) && self.m.value(0x400b048c)&1!=0 {
   self.m.0.borrow_mut().values.insert(a,(v+80)%640);
  }
  v
 }
 fn write(&self,a:usize,v:u32){self.m.write(a,v);
  if a==0x40003024 {self.m.write(0x4000301c,0);}
  if a==0x4000302c {self.m.write(0x40003028,0);}
  if a==0x40010074 {self.m.write(0x40010070,0);}
  if a==0x40010060 && v==1 {
   assert_eq!(self.m.value(0x40003330),0x8060005f);
   self.m.write(0x4000301c,0x100);self.m.write(0x4000b000,1999);self.m.write(0x40010070,1);
  }
 }
 fn delay_cycles(&self,_:u32){}
}
fn board(stalled:bool)->(mainboard::Board<Io>,Io){
 let io=Io{stalled,..Default::default()};let r=peripherals::initialize(unsafe{Peripherals::from_io(io.clone())},38400).unwrap();
 for p in pins::NFAULT {for off in [0x1280,0x1380] {let a=p.base()+off;let old=io.m.value(a);io.m.0.borrow_mut().values.insert(a,old|(1<<p.bit()));}}
 io.m.force(0x40010000,4);
 (mainboard::Board{analog:r.analog,pwm:r.pwm,drivers:r.drivers},io)
}
#[test]fn real_adapter_gate_preloads_and_each_therm_trigger_restoration(){
 let(mut b,io)=board(false);b.configure(13,50000,5000).unwrap();
 let begin=io.m.0.borrow().writes.len();assert_eq!(b.start_quadrature().unwrap(),[319,639,159,479]);
 let trace=io.m.0.borrow().writes[begin..].to_vec();
 let gate=trace.iter().position(|x|*x==(0x400b048c,0)).unwrap();
 let release=trace.iter().rposition(|x|*x==(0x400b048c,1)).unwrap();assert!(release>gate);
 for (module,pre) in [319,639,159,479].iter().enumerate(){
  let address=0x40010028+module*0x1000;
  assert!(trace[gate+1..release].contains(&(address,*pre)));
  for unit in 0..3 {
   let aq=0x40010120+module*0x1000+unit*0x200;
   assert!(trace[..gate].contains(&(aq,0x11)));
   assert!(trace[..gate].contains(&(aq+8,0x101)));
   assert!(trace[gate..release].contains(&(aq,0x12)));
   assert!(trace[gate..release].contains(&(aq+8,0x102)));
  }
 }
 // Behavioral first-cycle check using the verified preload/AQ commands:
 // latches are clear before release, then ZERO sets and compare320 clears.
 let mut counter=[319,639,159,479];let mut level=[false;4];let mut rises=[Vec::new(),Vec::new(),Vec::new(),Vec::new()];
 for tick in 1..=1281 {
  for m in 0..4 {counter[m]=(counter[m]+1)%640;if counter[m]==0 {assert!(!level[m]);level[m]=true;rises[m].push(tick);}if counter[m]==320 {level[m]=false;}}
 }
 assert_eq!(rises[1],vec![1,641,1281]);assert_eq!(rises[3],vec![161,801]);assert_eq!(rises[0],vec![321,961]);assert_eq!(rises[2],vec![481,1121]);
 for (m,selector) in [16,13,22,23].into_iter().enumerate(){
  let n=io.m.0.borrow().writes.len();let r=b.diagnostic_sample(m as u32,128,1,8).unwrap();assert_eq!(r.raw,1999);
  assert_eq!(io.m.value(0x4000304c),selector<<15);assert_eq!(io.m.value(0x40003330),0x800000bf);assert_eq!(io.m.value(0x40010060),0);
  assert!(!io.m.0.borrow().writes[n..].iter().any(|(a,v)|*a==0x40003330 && v&(1<<30)!=0));
 }
 // No CAN clock selector or divider writes in synchronized start/capture.
 assert!(!io.m.0.borrow().writes[begin..].iter().any(|(a,_)|[0x400b0448,0x400b0450].contains(a)));
 b.stop();for m in 0..4{assert_eq!(io.m.value(0x40010010+m*0x1000)&3,2);}
}
#[test]fn stalled_warmup_and_readback_failures_fail_closed(){
 for failure in 0..3{
  let(mut b,io)=board(failure==0);b.configure(13,50000,5000).unwrap();
  if failure==1{io.m.force(0x400b048c,1);}if failure==2{io.m.force(0x40011014,640);}
  assert!(b.start_quadrature().is_err());
  for m in 0..4{assert_eq!(io.m.value(0x40010010+m*0x1000)&3,2);for u in 0..3{assert_eq!(io.m.value(0x40010130+m*0x1000+u*0x200),0x11);}}
 }
}

#[test]
fn disable_attempts_both_independent_gpios_even_when_first_confirmation_fails() {
 for failed in [pins::INL_ENABLE,pins::DRV_ENABLE] {
  let (mut b,io)=board(false);b.enables(3).unwrap();
  io.m.force(failed.pad(),0);let n=io.m.0.borrow().writes.len();
  assert!(b.enables(0).is_err());
  let writes=io.m.0.borrow().writes[n..].to_vec();
  for p in [pins::INL_ENABLE,pins::DRV_ENABLE] {
   assert!(writes.contains(&(p.base()+0x12a0,1<<p.bit())));
   assert_eq!(io.m.value(p.base()+0x1280)&(1<<p.bit()),0);
  }
 }
}
