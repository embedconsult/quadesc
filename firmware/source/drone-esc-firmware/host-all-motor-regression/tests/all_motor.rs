// Exercise the actual firmware adapter and actual HAL with modeled registers.
#[path="../../mcu-xcp-platform/src/mainboard.rs"] mod mainboard;
#[path="../../../inputs/esc-som-board/tests/support/mod.rs"] mod support;
use mainboard_characterization::*;
use esc_som_board::{peripherals,pins};
use am13_rs::Peripherals;
use support::Model;
use xcp_core::{ApplyRequest,ProviderOperation,ProviderRequest};
use xcp_messages::{ProviderCompletion,WriteResult};
fn board()->(mainboard::Board<Model>,Model){
 let m=Model::default();let r=peripherals::initialize(unsafe{Peripherals::from_io(m.clone())},38400).unwrap();
 for p in pins::NFAULT {for off in [0x1280,0x1380] {let a=p.base()+off;let old=m.value(a);m.0.borrow_mut().values.insert(a,old|(1<<p.bit()));}}
 (mainboard::Board{analog:r.analog,pwm:r.pwm,drivers:r.drivers},m)
}
fn off(m:&Model){for module in 0..4{let b=0x40010000+module*0x1000;assert_eq!(m.value(b+0x10)&3,2);for unit in 0..3{assert_eq!(m.value(b+0x130+unit*0x200),0x11);}}}
fn write(c:&mut Controller<mainboard::Board<Model>>,i:usize,v:u32,s:u64){
 let operation=ProviderOperation{service_epoch:1,session_generation:1,sequence:s};
 let r=c.request(ProviderRequest::Apply(ApplyRequest{operation,descriptor_id:FIELDS[i].id,encoded_value:v.to_le_bytes(),length:FIELDS[i].width,expires_at_us:1_000_000}),10000+s*1000);
 assert!(matches!(r,ProviderCompletion::Write{result:WriteResult::Applied{..},..}),"{r:?}");c.request(ProviderRequest::ReleaseOutcome{operation},10000+s*1000);
}
#[test]
fn all_twelve_actual_routes_quantize_and_each_module_configured_once(){
 let expected=[(0,1),(0,4),(0,5),(1,0),(1,2),(1,4),(1,5),(2,0),(2,2),(2,4),(3,0),(3,2),(3,4)];
 assert_eq!(pins::PWM_ROUTES.map(|(_,_,m,o)|(m,o)),expected);
 assert_eq!(route_mask(13),0x1fbf);assert_eq!(route_mask(13).count_ones(),12);assert_eq!(route_mask(14),0);
 for (hz,duty) in [(20000,2000),(17321,3333),(20000,0),(20000,10000)] {
  let (mut b,m)=board();let begin=m.0.borrow().writes.len();let e=b.configure(13,hz,duty).unwrap();
  if hz==20000{assert_eq!(e.frequency,20000);assert_eq!(e.period,1600);assert_eq!(e.divider,1);}
  if duty==2000{assert_eq!(e.high,320);assert_eq!(e.high_ns,10000);}
  assert_eq!(e.high,(e.period*duty as u32+5000)/10000);
  for module in 0..4 {let base=0x40010000+module*0x1000;assert_eq!(m.0.borrow().writes[begin..].iter().filter(|(a,_)|*a==base+0x14).count(),1);}
  off(&m);b.start(13).unwrap();
  for (i,(_,_,module,output)) in pins::PWM_ROUTES.iter().enumerate(){let base=0x40010000+*module as usize*0x1000;let unit=*output as usize/2;let side=*output as usize%2;
   let force=(m.value(base+0x130+unit*0x200)>>(side*4))&7;
   if i==6{assert_eq!(force,1);continue;}
   assert_eq!(m.value(base+0x100+unit*0x200+side*8),e.high);
   assert_eq!(m.value(base+0x120+unit*0x200+side*8),if side==0{0x12}else{0x102});
   assert_eq!(force,if duty==0{1}else if duty==10000{2}else{0});assert_eq!(m.value(base+0x10)&3,0);
  }
  b.stop();off(&m);
 }
}
#[test]
fn individual_routes_remain_compatible_and_all_to_single_clears_other_widths(){
 for route in 0..13 {let(mut b,m)=board();b.configure(13,20000,2000).unwrap();b.start(13).unwrap();b.configure(route,20000,2500).unwrap();b.start(route).unwrap();
  for (i,(_,_,module,output)) in pins::PWM_ROUTES.iter().enumerate(){let base=0x40010000+*module as usize*0x1000;let force=(m.value(base+0x130+(*output as usize/2)*0x200)>>((*output as usize%2)*4))&7;assert_eq!(force,if i==route as usize{0}else{1});}
 }
}
#[test]
fn partial_configure_and_start_failures_stop_every_module(){
 let(mut b,m)=board();b.configure(13,20000,2000).unwrap();b.start(13).unwrap();m.force(0x400b0424,0b01100000);assert!(b.configure(13,20000,2000).is_err());off(&m);
 let(mut b,m)=board();b.configure(0,20000,2000).unwrap();assert!(b.start(13).is_err());off(&m);
}
#[test]
fn controller_all_mode_stop_disconnect_fault_invalid_pulse_and_failure_safeoff(){
 for reason in 0..5 {let(b,m)=board();let mut c=Controller::new(b);c.synchronize(1);
  write(&mut c,DRV_ENABLE_MASK,3,1);write(&mut c,DRV_ACTION,3,2);write(&mut c,PWM_CHANNEL,13,3);write(&mut c,PWM_ACTION,1,4);
  assert_eq!(c.value(PWM_APPLIED_ROUTE_MASK),0x1fbf);write(&mut c,PWM_ACTION,2,5);assert_eq!(c.value(PWM_STATUS),2);
  match reason {0=>write(&mut c,PWM_ACTION,3,6),1=>c.synchronize(2),2=>{let p=pins::NFAULT[2];m.force(p.base()+0x1380,0);c.poll(17000);},3=>write(&mut c,PWM_ACTION,4,6),_=>{m.force(0x400b0424,0);write(&mut c,PWM_ACTION,1,6);}}
  off(&m);assert_eq!(c.value(PWM_APPLIED_ROUTE_MASK),0);if reason!=0{assert_eq!(c.value(DRV_APPLIED_ENABLE_MASK),0);}
 }
}
