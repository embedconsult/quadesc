use mainboard_characterization::*;
use xcp_core::{ApplyRequest,ProviderOperation,ProviderRequest,ReadRequest,VirtualMap,Session,ProviderPort,SubmitError,Packet,Dispatch};
use xcp_messages::{ProviderCompletion,WriteResult,ReadResult,RejectionReason,SynchronizeResult,QuiesceResult};
#[derive(Default)]
struct Port;
impl ProviderPort for Port {fn try_submit(&mut self,_:ProviderRequest)->Result<(),SubmitError>{Ok(())}}
#[derive(Default)]
struct Model {started:bool,enabled:u16,pulse:bool,done:bool,starts:u32,stops:u32,reply:u16,fail:bool,fault:u16,enable_fail:bool,frames:Vec<(u16,u16)>}
impl Hardware for Model {
 fn stop(&mut self){self.started=false;self.pulse=false;self.stops+=1;}
 fn configure(&mut self,_:u16,h:u32,d:u16)->Result<Effective,HardwareError>{Ok(Effective{frequency:h,high_ns:1000,period:32000,high:d as u32,divider:1})}
 fn start(&mut self,_:u16)->Result<(),HardwareError>{self.started=true;self.starts+=1;Ok(())}
 fn pulse(&mut self,_:u16,_:u32)->Result<Effective,HardwareError>{self.pulse=true;self.done=false;self.starts+=1;Ok(Effective::default())}
 fn pulse_done(&mut self,_:u16)->bool{self.done}
 fn enables(&mut self,m:u16)->Result<(),HardwareError>{if self.enable_fail {return Err(HardwareError::Transport)}self.enabled=m;Ok(())}
 fn transfer(&mut self,d:u16,f:u16)->Result<u16,HardwareError>{self.frames.push((d,f));if self.fail{Err(HardwareError::Transport)}else{Ok(self.reply)}}
 fn adc(&mut self,c:usize)->Result<u16,HardwareError>{if c==7{Err(HardwareError::Transport)}else{Ok(100+c as u16)}}
 fn faults(&self)->u16{self.fault}
}
fn new()->Controller<Model>{let mut c=Controller::new(Model::default());c.synchronize(1);c}
fn op(seq:u64)->ProviderOperation{ProviderOperation{service_epoch:1,session_generation:1,sequence:seq}}
fn apply(c:&mut Controller<Model>,index:usize,value:u32,seq:u64)->ProviderCompletion {
 c.request(ProviderRequest::Apply(ApplyRequest{operation:op(seq),descriptor_id:FIELDS[index].id,encoded_value:value.to_le_bytes(),length:FIELDS[index].width,expires_at_us:1_000_000}),10000)
}
fn write(c:&mut Controller<Model>,index:usize,value:u32,seq:u64){assert!(matches!(apply(c,index,value,seq),ProviderCompletion::Write{result:WriteResult::Applied{..},..}));c.request(ProviderRequest::ReleaseOutcome{operation:op(seq)},10000);}
#[test]
fn typed_permissions_ranges_and_virtual_address_bounds(){
 let map=VirtualMap::new(&REGIONS).unwrap();assert_eq!(map.regions().len(),COUNT);
 let mut c=new();
 assert!(matches!(apply(&mut c,ADC_VALID_MASK,1,1),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::ReadOnly},..}));
 c.request(ProviderRequest::ReleaseOutcome{operation:op(1)},10000);
 assert!(matches!(apply(&mut c,PWM_CHANNEL,14,2),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::Bounds},..}));
 let r=c.request(ProviderRequest::Read(ReadRequest{correlation:op(3).correlation(),descriptor_id:999,offset:0,length:2}),10000);
 assert!(matches!(r,ProviderCompletion::Read{result:ReadResult::AccessDenied,..}));
 let r=c.request(ProviderRequest::Read(ReadRequest{correlation:op(3).correlation(),descriptor_id:FIELDS[PWM_CHANNEL].id,offset:1,length:2}),10000);
 assert!(matches!(r,ProviderCompletion::Read{result:ReadResult::OutOfRange,..}));
}
#[test]
fn staged_pwm_apply_start_stop_and_idempotent_command(){
 let mut c=new();write(&mut c,PWM_DUTY_PERCENT,10000,1);assert!(!c.hardware.started);
 write(&mut c,PWM_ACTION,1,2);assert!(!c.hardware.started);assert_eq!(c.value(PWM_STATUS),1);
 let a=apply(&mut c,PWM_ACTION,2,3);let b=apply(&mut c,PWM_ACTION,2,3);assert_eq!(a,b);assert_eq!(c.hardware.starts,1);
 c.request(ProviderRequest::ReleaseOutcome{operation:op(3)},10000);
 write(&mut c,PWM_CHANNEL,5,4);assert_eq!(c.value(PWM_ACTIVE_CHANNEL),0);
 write(&mut c,PWM_ACTION,3,5);assert!(!c.hardware.started);assert_eq!(c.value(PWM_ACTION),0);
 assert!(matches!(apply(&mut c,PWM_ACTION,2,3),ProviderCompletion::Write{result:WriteResult::Retired,..}));
}
#[test]
fn pulse_completion_retrigger_cancel_and_timeout(){
 let mut c=new();write(&mut c,PWM_ACTION,4,1);assert_eq!(c.hardware.starts,1);assert_eq!(c.value(PWM_STATUS),3);
 write(&mut c,PWM_ACTION,4,2);assert_eq!(c.hardware.starts,1);assert_eq!(c.value(PWM_COMMAND_SEQUENCE),1);
 c.hardware.done=true;c.poll(20000);assert_eq!(c.value(PWM_STATUS),4);assert_eq!(c.value(PWM_COMPLETED_SEQUENCE),1);
 write(&mut c,PWM_ACTION,4,3);assert_eq!(c.hardware.starts,2);
 write(&mut c,PWM_ACTION,3,4);assert!(!c.hardware.pulse);assert_eq!(c.value(PWM_COMPLETED_SEQUENCE),3);
 write(&mut c,PWM_ACTION,4,5);c.poll(500000);assert_eq!(c.value(PWM_STATUS),9);assert!(!c.hardware.pulse);
}
#[test]
fn spi_masks_absence_framing_readback_and_safe_errors(){
 let mut c=new();write(&mut c,DRV_ENABLE_MASK,1,1);write(&mut c,DRV_ACTION,3,2);
 c.poll(13000);write(&mut c,DRV_DEVICE,3,3);write(&mut c,DRV_REGISTER,2,4);write(&mut c,DRV_WRITE_VALUE,0x400,5);
 // Enable time is10000; command fixture uses10000 so first prove waking is rejected.
 write(&mut c,DRV_ACTION,2,6);assert_eq!(c.value(DRV_STATUS),7);assert!(c.hardware.frames.is_empty());
 // Use actual later timestamp for protocol operation.
 let request=|seq|ProviderRequest::Apply(ApplyRequest{operation:op(seq),descriptor_id:FIELDS[DRV_ACTION].id,encoded_value:2u32.to_le_bytes(),length:2,expires_at_us:1000000});
 c.request(request(7),15000);assert_eq!(c.value(DRV_STATUS),8);c.request(ProviderRequest::ReleaseOutcome{operation:op(7)},15000);
 write(&mut c,DRV_WRITE_VALUE,0x120,8);c.hardware.reply=0x120;c.request(request(9),15000);assert_eq!(c.value(DRV_STATUS),2);
 assert_eq!(c.hardware.frames,[(3,0x1120),(3,0x9000)]);
 c.request(ProviderRequest::ReleaseOutcome{operation:op(9)},15000);c.hardware.reply=0xffff;c.request(request(10),15000);
 assert_eq!(c.value(DRV_STATUS),5);assert_eq!(c.value(DRV_RAW_RESPONSE),65535);assert_eq!(c.hardware.enabled,0);
}
#[test]
fn adc_cache_has_real_scan_boundary_validity_and_age(){
 let mut c=new();assert_eq!(c.value(ADC_AGE_MS),u32::MAX);
 for i in 0..29{c.poll(i*1000);}assert_eq!(c.value(ADC_SCAN_SEQUENCE),0);
 c.poll(29000);assert_eq!(c.value(ADC_SCAN_SEQUENCE),1);assert_eq!(c.value(ADC_VALID_MASK),0x3fffffff&!(1<<7));assert_eq!(c.value(ADC_ERROR_MASK),1<<7);
 assert_eq!(c.value(8+29),129);c.poll(40000);assert_eq!(c.value(ADC_AGE_MS),11);
}
#[test]
fn disconnect_stop_and_save_admission(){
 let mut c=new();write(&mut c,PWM_ACTION,1,1);write(&mut c,PWM_ACTION,2,2);assert!(!c.can_save());
 c.synchronize(2);assert!(!c.hardware.started);assert_eq!(c.hardware.enabled,0);assert!(c.can_save());
}
#[test]
fn engineering_golden_points_and_nonfinite_calibration_rejected(){
 // Positive shunt current lowers the inverting DRV SO voltage; midpoint is zero.
 let (mid,_)=convert_adc(0,2048,DEFAULT_CAL[0]);assert!(mid.abs()<1e-5);
 assert!((convert_adc(0,1024,DEFAULT_CAL[0]).0-41.25).abs()<1e-4);
 assert!((convert_adc(0,3072,DEFAULT_CAL[0]).0+41.25).abs()<1e-4);
 assert!((convert_adc(1,2048,DEFAULT_CAL[1]).0-18.15).abs()<1e-4);
 let ntc=(0..30).find(|&i|ADC_KIND[i]==2).unwrap();
 assert!((convert_adc(ntc,2048,[1.0,0.0]).0-10000.0).abs()<0.01);
 assert!((convert_adc(ntc,1024,[1.0,0.0]).0-3333.3333).abs()<0.01);
 assert!((convert_adc(ntc,2048,[1.1,15.0]).0-11015.0).abs()<0.01);
 for raw in [0,4095] {let (_,f)=convert_adc(ntc,raw,[1.0,0.0]);assert_eq!(f&1,0);assert_ne!(f&2,0);}
 assert_ne!(convert_adc(ntc,4096,[1.0,0.0]).1&128,0);
 let mut c=new();
 for (i,v) in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY,10001.0].into_iter().enumerate(){
  let seq=i as u64+1;
  assert!(matches!(apply(&mut c,ADC_FIELDS[0][1],v.to_bits(),seq),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::Bounds},..}));
  c.request(ProviderRequest::ReleaseOutcome{operation:op(seq)},10000);
 }
 write(&mut c,ADC_FIELDS[0][1],(-0.05f32).to_bits(),10);
 write(&mut c,ADC_FIELDS[0][2],90f32.to_bits(),11);
 for i in 0..30{c.poll(i*1000);}
 assert!((f32::from_bits(c.value(ADC_FIELDS[0][0]))-85.0).abs()<1e-5);
 assert_eq!(c.value(ADC_FIELDS[0][3])&1,0); // CSA not read/verified on Model.
 assert_ne!(c.value(ADC_FIELDS[0][3])&8,0);
 let before=c.calibration();let mut bad=before;bad[29][1]=f32::NAN;
 assert!(!c.restore_calibration(bad));assert_eq!(c.calibration(),before); // atomic RAM restore
 assert!(!c.hardware.started && c.hardware.enabled==0);
}

fn timed_request(index:usize,value:u32,seq:u64)->ProviderRequest {
 ProviderRequest::Apply(ApplyRequest{operation:op(seq),descriptor_id:FIELDS[index].id,encoded_value:value.to_le_bytes(),length:FIELDS[index].width,expires_at_us:100_000_000})
}
fn at(c:&mut Controller<Model>,index:usize,value:u32,seq:u64,now:u64) {
 let r=c.request(timed_request(index,value,seq),now);
 assert!(matches!(r,ProviderCompletion::Write{result:WriteResult::Applied{..},..}),"{r:?}");
 c.request(ProviderRequest::ReleaseOutcome{operation:op(seq)},now);
}
fn diagnostic()->Controller<Model> {
 let mut c=new();c.hardware.fault=15;
 at(&mut c,DRV_ENABLE_MASK,1,1,0);at(&mut c,DRV_ACTION,3,2,0);c
}
#[test]
fn held_fault_diagnostic_wake_read_and_rmw_clear_keeps_fault_observable() {
 let mut c=diagnostic();assert_eq!(c.hardware.enabled,1);assert!(!c.can_save());
 at(&mut c,DRV_ACTION,1,3,1999);assert_eq!(c.value(DRV_STATUS),7);assert!(c.hardware.frames.is_empty());
 c.poll(2000);assert_eq!(c.hardware.enabled,1);assert_eq!(c.value(FAULT_ASSERTED_MASK),15);
 // Exact host sequence: read reg2, preserve every other bit, write CLR_FLT=1.
 at(&mut c,DRV_REGISTER,2,4,2000);c.hardware.reply=0x120;
 at(&mut c,DRV_ACTION,1,5,2000);assert_eq!(c.value(DRV_STATUS),1);
 let original=c.value(DRV_READBACK);at(&mut c,DRV_WRITE_VALUE,original|1,6,2001);
 at(&mut c,DRV_ACTION,2,7,2001);assert_eq!(c.value(DRV_STATUS),2);
 assert_eq!(c.hardware.frames,[(0,0x9000),(0,0x1121),(0,0x9000)]);
 assert_eq!(c.value(FAULT_ASSERTED_MASK),15);assert_eq!(c.hardware.enabled,1);
 assert!(!c.hardware.started && !c.hardware.pulse);
}
#[test]
fn diagnostic_blocks_all_pwm_routes_and_enable_escalation_even_before_wake() {
 let mut c=diagnostic();let mut seq=3;
 for faults in [15,0] {c.hardware.fault=faults;
  for channel in 0..=13 {at(&mut c,PWM_CHANNEL,channel,seq,100);seq+=1;
   for action in [1,2,4] {at(&mut c,PWM_ACTION,action,seq,100);seq+=1;
    assert_eq!(c.value(PWM_STATUS),8);assert_eq!(c.hardware.enabled,1);
   }
  }
  for mask in [2,3] {at(&mut c,DRV_ENABLE_MASK,mask,seq,100);seq+=1;at(&mut c,DRV_ACTION,3,seq,100);seq+=1;
   assert_eq!(c.hardware.enabled,1);assert_eq!(c.value(DRV_STATUS),9);
  }
 }
 assert_eq!(c.hardware.starts,0);
 at(&mut c,PWM_ACTION,3,seq,100);assert_eq!(c.hardware.enabled,1);
}
#[test]
fn diagnostic_timeout_is_nonrenewing_on_poll_or_command_and_requires_explicit_disable() {
 for poll in [false,true] {
  let mut c=diagnostic();at(&mut c,DRV_ACTION,3,3,DRV_DIAGNOSTIC_TIMEOUT_US-1);
  assert_eq!(c.hardware.enabled,1);
  if poll {c.poll(DRV_DIAGNOSTIC_TIMEOUT_US);} else {
   assert!(matches!(c.request(timed_request(DRV_ACTION,3,4),DRV_DIAGNOSTIC_TIMEOUT_US),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::OutputUnavailable},..}));
   c.request(ProviderRequest::ReleaseOutcome{operation:op(4)},DRV_DIAGNOSTIC_TIMEOUT_US);
  }
  assert_eq!(c.hardware.enabled,0);assert_eq!(c.value(DRV_STATUS),9);assert!(c.can_save());
  at(&mut c,DRV_ACTION,3,5,DRV_DIAGNOSTIC_TIMEOUT_US+1);assert_eq!(c.hardware.enabled,0);
  at(&mut c,DRV_ACTION,4,6,DRV_DIAGNOSTIC_TIMEOUT_US+2);
  at(&mut c,DRV_ACTION,3,7,DRV_DIAGNOSTIC_TIMEOUT_US+3);assert_eq!(c.hardware.enabled,1);
  c.synchronize(2);assert_eq!(c.hardware.enabled,0);assert!(!c.hardware.started);
 }
}
#[test]
fn actuation_fault_still_shuts_down_and_faulted_enable_is_refused() {
 for mask in [2,3] {
  let mut c=new();at(&mut c,DRV_ENABLE_MASK,mask,1,0);at(&mut c,DRV_ACTION,3,2,0);
  at(&mut c,PWM_ACTION,1,3,2000);at(&mut c,PWM_ACTION,2,4,2000);assert!(c.hardware.started);
  c.hardware.fault=15;c.poll(2001);assert_eq!(c.hardware.enabled,0);assert!(!c.hardware.started);assert_eq!(c.value(DRV_STATUS),6);
  at(&mut c,DRV_ACTION,3,5,2002);assert_eq!(c.hardware.enabled,0);assert_eq!(c.value(DRV_STATUS),6);
  at(&mut c,DRV_ACTION,1,6,2003);assert!(c.hardware.frames.is_empty());
 }
}
#[test]
fn diagnostic_commands_are_idempotent_and_spi_errors_shut_down() {
 let mut c=diagnostic();c.hardware.reply=0x120;at(&mut c,DRV_REGISTER,2,3,2000);
 at(&mut c,DRV_WRITE_VALUE,0x121,4,2000);
 let r=timed_request(DRV_ACTION,2,5);let a=c.request(r,2000);let b=c.request(r,2001);assert_eq!(a,b);assert_eq!(c.hardware.frames.len(),2);
 c.request(ProviderRequest::ReleaseOutcome{operation:op(5)},2001);
 assert!(matches!(c.request(r,2002),ProviderCompletion::Write{result:WriteResult::Retired,..}));
 c.hardware.fail=true;at(&mut c,DRV_ACTION,1,6,2003);assert_eq!(c.value(DRV_STATUS),6);assert_eq!(c.hardware.enabled,0);
 let mut c=diagnostic();c.hardware.reply=0xffff;at(&mut c,DRV_ACTION,1,3,2000);assert_eq!(c.value(DRV_STATUS),5);assert_eq!(c.hardware.enabled,0);
 let mut c=diagnostic();at(&mut c,DRV_REGISTER,2,3,2000);at(&mut c,DRV_WRITE_VALUE,0x120,4,2000);
 at(&mut c,DRV_ACTION,2,5,2000);assert_eq!(c.value(DRV_STATUS),9);assert_eq!(c.hardware.enabled,0);
}
#[test]
fn reboot_is_inhibited_idempotent_and_requires_transmitted_positive_ack() {
 for sent in [false,true] {
  let mut c=new();at(&mut c,DRV_ENABLE_MASK,3,1,0);at(&mut c,DRV_ACTION,3,2,0);
  at(&mut c,PWM_ACTION,1,3,2000);at(&mut c,PWM_ACTION,2,4,2000);
  let r=timed_request(REBOOT_ACTION,1,5);let result=c.request(r,2001);
  assert!(!c.hardware.started);assert_eq!(c.hardware.enabled,0);assert!(!c.can_save());assert_eq!(c.value(REBOOT_ACTION),0);
  let stops=c.hardware.stops;assert_eq!(c.request(r,2002),result);assert_eq!(c.hardware.stops,stops);
  let mut gate=RebootAfterAck::default();gate.observe(r,result);
  assert_eq!(gate.response_sent(&[0xff],sent),sent);assert!(!gate.response_sent(&[0xff],true));
  c.request(ProviderRequest::ReleaseOutcome{operation:op(5)},2002);
  assert!(matches!(c.request(timed_request(PWM_ACTION,4,6),2003),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::WrongLifecycle},..}));
 }
}
#[test]
fn reboot_zero_malformed_expired_reads_and_failed_disable_never_arm() {
 let mut c=new();let mut gate=RebootAfterAck::default();
 let read=ProviderRequest::Read(ReadRequest{correlation:op(1).correlation(),descriptor_id:FIELDS[REBOOT_ACTION].id,offset:0,length:2});
 gate.observe(read,c.request(read,0));assert!(!gate.response_sent(&[0xff],true));
 for (seq,value,length,expires) in [(1,0u32,2,100),(2,2,2,100),(3,1,1,100),(4,1,2,0)] {
  let r=ProviderRequest::Apply(ApplyRequest{operation:op(seq),descriptor_id:FIELDS[REBOOT_ACTION].id,encoded_value:value.to_le_bytes(),length,expires_at_us:expires});
  gate.observe(r,c.request(r,0));assert!(!gate.response_sent(&[0xff],true));c.request(ProviderRequest::ReleaseOutcome{operation:op(seq)},0);
 }
 c.hardware.enable_fail=true;let r=timed_request(REBOOT_ACTION,1,5);let result=c.request(r,0);
 assert!(matches!(result,ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::OutputUnavailable},..}));
 gate.observe(r,result);assert!(!gate.response_sent(&[0xff],true));
}

#[test]
fn configured_diagnostic_to_drive_requires_wake_and_clear_faults() {
 let mut c=diagnostic();
 at(&mut c,DRV_ENABLE_MASK,3,3,2000);
 at(&mut c,DRV_ACTION,3,4,2000);
 assert_eq!(c.hardware.enabled,1);assert_eq!(c.value(DRV_STATUS),9);
 c.hardware.fault=0;
 at(&mut c,DRV_ACTION,3,5,2001);
 assert_eq!(c.hardware.enabled,3);assert_eq!(c.value(DRV_STATUS),3);
 assert!(!c.hardware.started);
 at(&mut c,PWM_ACTION,1,6,4001);at(&mut c,PWM_ACTION,2,7,4001);
 assert!(c.hardware.started);
 c.hardware.fault=8;c.poll(4002);
 assert_eq!(c.hardware.enabled,0);assert!(!c.hardware.started);
}

#[test]
fn failed_shutdown_fences_outputs_spi_save_and_reboot_until_explicit_retry() {
 let mut c=new();at(&mut c,DRV_ENABLE_MASK,3,1,0);at(&mut c,DRV_ACTION,3,2,0);
 at(&mut c,PWM_ACTION,1,3,3000);at(&mut c,PWM_ACTION,2,4,3000);assert!(c.hardware.started);
 c.hardware.enable_fail=true;c.safe_off();assert!(!c.hardware.started);assert!(!c.enable_state_known());
 assert_eq!(c.value(DRV_APPLIED_ENABLE_MASK),3);assert!(!c.can_save());
 let sequence=c.value(DRV_COMMAND_SEQUENCE);
 assert!(matches!(c.request(timed_request(DRV_ACTION,4,5),3500),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::OutputUnavailable},..}));
 c.request(ProviderRequest::ReleaseOutcome{operation:op(5)},3500);
 assert_eq!(c.value(DRV_COMMAND_SEQUENCE),sequence+1);assert_eq!(c.value(DRV_STATUS),6);
 let starts=c.hardware.starts;let frames=c.hardware.frames.len();
 for (n,(idx,value)) in [(PWM_ACTION,1),(PWM_ACTION,2),(PWM_ACTION,4),(DRV_ACTION,1),(DRV_ACTION,2),(DRV_ACTION,3),(BENCH_ACTION,1),(ADC_DIAG_ACTION,1),(REBOOT_ACTION,1)].into_iter().enumerate() {
  let seq=10+n as u64;
  assert!(matches!(c.request(timed_request(idx,value,seq),4000),ProviderCompletion::Write{result:WriteResult::Rejected{reason:RejectionReason::OutputUnavailable},..}));
  c.request(ProviderRequest::ReleaseOutcome{operation:op(seq)},4000);
 }
 c.hardware.enable_fail=false;c.poll(5000);assert!(!c.enable_state_known());
 let sequence=c.value(DRV_COMMAND_SEQUENCE);at(&mut c,DRV_ACTION,4,30,5001);assert_eq!(c.value(DRV_COMMAND_SEQUENCE),sequence+1);assert_eq!(c.value(DRV_STATUS),4);assert!(c.enable_state_known());assert_eq!(c.value(DRV_APPLIED_ENABLE_MASK),0);
 assert_eq!(c.hardware.starts,starts);assert_eq!(c.hardware.frames.len(),frames);assert!(!c.hardware.started);assert!(c.can_save());
 // Disable retry discards the old applied configuration: START cannot resume it.
 at(&mut c,PWM_ACTION,2,31,5002);assert_eq!(c.hardware.starts,starts);assert!(!c.hardware.started);
}
#[test]
fn failed_constructor_disable_is_unknown_and_gpio_confirmation_required() {
 let mut c=Controller::new(Model{enable_fail:true,..Model::default()});assert_eq!(c.value(DRV_ENABLE_STATE_KNOWN),0);assert!(!c.can_save());
 c.synchronize(1);assert!(!c.enable_state_known());c.hardware.enable_fail=false;
 at(&mut c,PWM_ACTION,3,1,0);assert!(c.enable_state_known());assert_eq!(c.hardware.starts,0);
 assert_eq!(FIELDS[DRV_ENABLE_STATE_KNOWN].id,297);assert_eq!(REGIONS[DRV_ENABLE_STATE_KNOWN].address,0x1534);assert_eq!(COUNT,252);
 assert!(FIELDS.iter().all(|f|!(222..=226).contains(&f.id)));
}
#[test]
fn uncertain_lifecycle_preflight_is_forwarded_and_fences_wire_session() {
 let uncertain=ProviderCompletion::Synchronized{session_generation:1,result:SynchronizeResult::Uncertain};
 assert!(completion_requires_fence(uncertain));
 let mut session=Session::new();let mut port=Port::default();let map=VirtualMap::new(&REGIONS).unwrap();
 session.handle_packet(&map,&Packet::try_from_slice(&[0xff,0]).unwrap(),0,&mut port);
 assert_ne!(session.complete(uncertain,&mut port),Dispatch::Respond(Packet::try_from_slice(&[0xff]).unwrap()));
 assert!(completion_requires_fence(ProviderCompletion::Quiesced{correlation:op(1).correlation(),result:QuiesceResult::Uncertain}));
 assert!(!completion_requires_fence(ProviderCompletion::Quiesced{correlation:op(1).correlation(),result:QuiesceResult::Quiesced}));
}
