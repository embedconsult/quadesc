use mainboard_characterization::*;
use xcp_core::{ApplyRequest,ProviderOperation,ProviderRequest};
use xcp_messages::{ProviderCompletion,WriteResult};
#[derive(Default)]
struct Rig { normal:Vec<usize>,sync:Vec<u32>,fault:u16,fail:bool,stop:bool }
impl Hardware for Rig {
 fn stop(&mut self){self.stop=true;}
 fn configure(&mut self,_:u16,h:u32,d:u16)->Result<Effective,HardwareError>{Ok(Effective{frequency:h,period:32_000_000/h,high:d as u32*640/10000,divider:1,high_ns:10000})}
 fn start(&mut self,_:u16)->Result<(),HardwareError>{Ok(())}
 fn start_quadrature(&mut self)->Result<[u32;4],HardwareError>{self.stop=false;Ok([319,639,159,479])}
 fn pulse(&mut self,_:u16,_:u32)->Result<Effective,HardwareError>{Err(HardwareError::Invalid)}
 fn pulse_done(&mut self,_:u16)->bool{false}
 fn enables(&mut self,_:u16)->Result<(),HardwareError>{Ok(())}
 fn transfer(&mut self,_:u16,_:u16)->Result<u16,HardwareError>{Ok(0)}
 fn adc(&mut self,c:usize)->Result<u16,HardwareError>{self.normal.push(c);Ok(2048)}
 fn faults(&self)->u16{self.fault}
 fn diagnostic_sample(&mut self,c:u32,w:u32,m:u32,o:u32)->Result<DiagnosticSample,HardwareError>{
  assert_eq!((w,m,o),(128,1,8));self.sync.push(c);
  if self.fail {Err(HardwareError::Transport)}else{Ok(DiagnosticSample{raw:1900+c as u16,flags:1,..Default::default()})}
 }
}
struct Bench{c:Controller<Rig>,now:u64,seq:u64}
impl Bench{
 fn write(&mut self,i:usize,v:u32)->bool{
  self.seq+=1;let operation=ProviderOperation{service_epoch:1,session_generation:1,sequence:self.seq};
  let r=self.c.request(ProviderRequest::Apply(ApplyRequest{operation,descriptor_id:FIELDS[i].id,encoded_value:v.to_le_bytes(),length:FIELDS[i].width,expires_at_us:self.now+1_000_000}),self.now);
  self.c.request(ProviderRequest::ReleaseOutcome{operation},self.now);
  matches!(r,ProviderCompletion::Write{result:WriteResult::Applied{..},..})
 }
 fn new()->Self {let mut b=Self{c:Controller::new(Rig::default()),now:10000,seq:0};b.c.synchronize(1);
  for (i,v) in [(DRV_ENABLE_MASK,3),(DRV_ACTION,3),(PWM_CHANNEL,13),(PWM_FREQUENCY_HZ,50000),(PWM_DUTY_PERCENT,5000),(PWM_ACTION,1)] {assert!(b.write(i,v));}
  b.now+=3000;b
 }
 fn start(&mut self){assert!(self.write(BENCH_ACTION,1));assert_eq!(self.c.value(BENCH_STATUS),1);}
 fn tick(&mut self){self.now+=1000;self.c.poll(self.now);}
}
#[test]fn four_therms_are_exclusively_hardware_each_scan_and_continuous_past_15s(){
 let mut b=Bench::new();b.start();
 for n in 0..16000 {b.tick();if n%500==0 {b.write(DRV_REGISTER,0);}}
 assert_eq!(b.c.value(BENCH_STATUS),1);assert_eq!(b.c.value(BENCH_CHANNEL_MASK),0x33000);
 for m in 0..4 {assert_eq!(b.c.value(THERMAL_SAMPLE_M1+m)&65535,1900+m as u32);assert!(b.c.value(BENCH_M1_COUNT+m*3)>500);}
 assert!(b.c.hardware.normal.iter().all(|c|![12,13,16,17].contains(c)));
 assert_eq!(b.c.hardware.sync.len(),(0..4).map(|m|b.c.value(BENCH_M1_COUNT+m*3) as usize).sum::<usize>());
}
#[test]fn active_edits_rejected(){let mut b=Bench::new();b.start();
 for (i,v) in [(BENCH_ACTION,1),(BENCH_OFFSET_TICKS,16),(ADC_DIAG_ACTION,1),(PWM_ACTION,1),(PWM_ACTION,2),(PWM_DUTY_PERCENT,3000),(DRV_ACTION,2)]{assert!(!b.write(i,v));}
 assert!(!b.c.can_save());assert!(b.write(PWM_ACTION,3));assert_eq!(b.c.value(BENCH_EFFECTIVE_MODE),0);assert_eq!(b.c.value(DRV_APPLIED_ENABLE_MASK),0);
}
#[test]fn stop_disconnect_fault_capture_failure_stale_and_late_request(){
 for path in 0..7{let mut b=Bench::new();b.start();
  match path {0=>{b.write(BENCH_ACTION,2);},1=>b.c.synchronize(2),2=>{b.c.hardware.fault=8;b.tick();},3=>{b.c.hardware.fail=true;for _ in 0..15{b.tick();}},4=>{b.now+=100000;b.c.poll(b.now);},5=>{b.now+=1500000;b.write(DRV_REGISTER,0);},_=>{b.write(DRV_ACTION,4);}}
  assert_ne!(b.c.value(BENCH_STATUS),1);assert!(b.c.hardware.stop);assert_eq!(b.c.value(BENCH_EFFECTIVE_MODE),0);assert_eq!(b.c.value(DRV_APPLIED_ENABLE_MASK),0);
 }
}
#[test]fn absolute_deadline_cannot_renew_with_traffic(){let mut b=Bench::new();b.start();
 for n in 0..330000 {b.tick();if n%500==0 {b.write(DRV_REGISTER,0);}}
 assert_eq!(b.c.value(BENCH_STATUS),4);assert_eq!(b.c.value(BENCH_ELAPSED_MS),330000);assert!(b.c.hardware.stop);
}
#[test]fn rejects_incompatible_pwm(){for (i,v) in [(PWM_CHANNEL,1),(PWM_FREQUENCY_HZ,20000),(PWM_DUTY_PERCENT,4000)]{
 let mut b=Bench::new();b.write(i,v);b.write(PWM_ACTION,1);assert!(!b.write(BENCH_ACTION,1));}}

#[test]fn missing_first_hardware_sample_is_not_reported_fresh(){let mut b=Bench::new();b.start();b.tick();
 for m in 0..4 {assert_eq!(b.c.value(BENCH_AGE_M1+m),u32::MAX);}
 for _ in 0..30 {b.tick();}
 for m in 0..4 {assert!(b.c.value(BENCH_AGE_M1+m)<100);}
}
