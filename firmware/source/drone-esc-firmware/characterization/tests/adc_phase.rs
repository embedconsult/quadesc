use mainboard_characterization::*;
use xcp_core::{ApplyRequest,ProviderOperation,ProviderRequest};
use xcp_messages::{ProviderCompletion,WriteResult};
#[derive(Default)]
struct Model{stopped:bool,fault:u16,fail:bool,samples:u32,normal:u32}
impl Hardware for Model {
 fn stop(&mut self){self.stopped=true;}
 fn configure(&mut self,_:u16,h:u32,_:u16)->Result<Effective,HardwareError>{Ok(Effective{frequency:h,period:640,high:320,divider:1,high_ns:10000})}
 fn start(&mut self,_:u16)->Result<(),HardwareError>{self.stopped=false;Ok(())}
 fn pulse(&mut self,_:u16,_:u32)->Result<Effective,HardwareError>{Err(HardwareError::Invalid)}
 fn pulse_done(&mut self,_:u16)->bool{false}
 fn enables(&mut self,_:u16)->Result<(),HardwareError>{Ok(())}
 fn transfer(&mut self,_:u16,_:u16)->Result<u16,HardwareError>{Ok(0)}
 fn adc(&mut self,_:usize)->Result<u16,HardwareError>{self.normal+=1;Ok(2048)}
 fn faults(&self)->u16{self.fault}
 fn diagnostic_sample(&mut self,_:u32,_:u32,_:u32,_:u32)->Result<DiagnosticSample,HardwareError>{self.samples+=1;if self.fail{Err(HardwareError::Transport)}else{Ok(DiagnosticSample{raw:2000+self.samples as u16,..Default::default()})}}
}
fn write(c:&mut Controller<Model>,idx:usize,v:u32,seq:u64)->bool{
 let op=ProviderOperation{service_epoch:1,session_generation:1,sequence:seq};
 let r=c.request(ProviderRequest::Apply(ApplyRequest{operation:op,descriptor_id:FIELDS[idx].id,encoded_value:v.to_le_bytes(),length:FIELDS[idx].width,expires_at_us:1_000_000}),10_000);
 c.request(ProviderRequest::ReleaseOutcome{operation:op},10_000);
 matches!(r,ProviderCompletion::Write{result:WriteResult::Applied{..},..})
}
fn new()->Controller<Model>{let mut c=Controller::new(Model::default());c.synchronize(1);c}
#[test]
fn bounded_capture_statistics_raw_cursor_and_normal_scan_continue(){
 let mut c=new();assert!(write(&mut c,ADC_DIAG_SAMPLES,32,1));assert!(write(&mut c,ADC_DIAG_ACTION,1,2));
 assert!(!c.can_save());assert!(!write(&mut c,ADC_DIAG_CHANNEL,2,3));assert!(!write(&mut c,PWM_ACTION,1,4));
 for t in 10..42 {c.poll(t*1000);}
 assert_eq!(c.value(ADC_DIAG_STATUS),2);assert!(c.can_save());assert_eq!(c.value(ADC_DIAG_COUNT),32);assert_eq!(c.value(ADC_DIAG_MIN),2001);assert_eq!(c.value(ADC_DIAG_MAX),2032);assert_eq!(c.value(ADC_DIAG_SUM),64528);
 assert_eq!(c.hardware.normal,32);assert_eq!(c.value(ADC_SCAN_SEQUENCE),1);assert_eq!(c.value(THERMAL_SAMPLE_M3),0x10800);
 assert!(write(&mut c,ADC_DIAG_CURSOR,31,5));assert_eq!(c.value(ADC_DIAG_RAW),2032);
 assert!(!write(&mut c,ADC_DIAG_CURSOR,32,6));
 c.poll(43_000);assert_eq!(c.hardware.samples,32);
}
#[test]
fn all_stop_paths_cancel_capture_and_errors_stop_outputs(){
 for path in 0..7 {
  let mut c=new();assert!(write(&mut c,ADC_DIAG_ACTION,1,1));
  match path {0=>c.safe_off(),1=>c.synchronize(2),2=>{assert!(write(&mut c,ADC_DIAG_ACTION,2,2));},3=>{assert!(write(&mut c,PWM_ACTION,3,2));},4=>{assert!(write(&mut c,DRV_ACTION,4,2));},5=>{assert!(write(&mut c,REBOOT_ACTION,1,2));},_=>{c.hardware.fail=true;c.poll(11_000);}}
  assert_ne!(c.value(ADC_DIAG_STATUS),1);assert!(c.hardware.stopped);
  let n=c.hardware.samples;c.poll(12_000);assert_eq!(n,c.hardware.samples);
 }
}
#[test]
fn nonrenewing_duration_and_communication_lease(){
 let mut c=new();assert!(write(&mut c,ADC_DIAG_ACTION,1,1));c.poll(1_510_000);assert_eq!(c.value(ADC_DIAG_STATUS),4);
 let mut c=new();assert!(write(&mut c,ADC_DIAG_ACTION,1,1));
 // Malformed requests still cannot extend absolute deadline.
 for t in 1..=16 {c.request(ProviderRequest::ReleaseOutcome{operation:ProviderOperation{service_epoch:1,session_generation:1,sequence:88}},t*1_000_000);c.poll(t*1_000_000);}
 assert_eq!(c.value(ADC_DIAG_STATUS),4);
}
#[test]
fn hardware_mode_requires_running_all_routes_correct_timing(){
 let mut c=new();assert!(write(&mut c,ADC_DIAG_MODE,1,1));assert!(!write(&mut c,ADC_DIAG_ACTION,1,2));
 assert!(write(&mut c,ADC_DIAG_MODE,0,3));assert!(write(&mut c,ADC_DIAG_WINDOW_CYCLES,127,4));assert!(!write(&mut c,ADC_DIAG_ACTION,1,5));
}
