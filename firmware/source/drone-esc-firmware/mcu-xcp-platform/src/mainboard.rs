//! Concrete I/O only. Policy and command state live in generated MainboardOwner.
use am13_rs::{io::{Mmio,RegisterIo},pwm};
use esc_som_board::{peripherals::{Analog,GateDrivers},pins::PWM_ROUTES};
use mainboard_characterization::{Hardware,HardwareError,Effective,ALL_MOTOR_SELECTOR,route_mask};
pub struct Board<I:RegisterIo=Mmio> {pub analog:Analog<I>,pub pwm:[pwm::Pwm<I>;4],pub drivers:GateDrivers<I>}
fn effective(e:pwm::Effective)->Effective {Effective{frequency:e.frequency_hz,high_ns:((e.high_ticks as u64*e.divider as u64*1_000_000_000+16_000_000)/32_000_000) as u32,period:e.period_ticks,high:e.high_ticks,divider:e.divider}}
fn route(channel:u16)->Result<(usize,u8),HardwareError>{PWM_ROUTES.get(channel as usize).map(|r|(r.2 as usize,r.3)).ok_or(HardwareError::Invalid)}
impl<I:RegisterIo> Hardware for Board<I> {
 fn start_quadrature(&mut self)->Result<[u32;4],HardwareError>{
  self.drivers.set_inl_enabled(false).map_err(|_|HardwareError::Transport)?;
  let result=(||{
   let pre=pwm::Pwm::quadrature_prepare(&mut self.pwm).map_err(|_|HardwareError::Transport)?;
   if self.faults()!=0 {return Err(HardwareError::Transport);}
   self.drivers.set_inl_enabled(true).map_err(|_|HardwareError::Transport)?;
   self.pwm[0].quadrature_release().map_err(|_|HardwareError::Transport)?;
   Ok(pre)
  })();
  if result.is_err(){self.stop();let _=self.enables(0);let _=self.pwm[0].quadrature_release();}
  result
 }
 fn adc_bounded(&mut self,c:usize)->Result<u16,HardwareError>{
  self.analog.capture(c,448,false,|_|{}).map(|v|v.raw).map_err(|_|HardwareError::Transport)
 }
 fn stop(&mut self){for p in &mut self.pwm{p.stop();}}
 fn configure(&mut self,c:u16,h:u32,d:u16)->Result<Effective,HardwareError>{
  self.stop();
  let result=(|| {
   let mask=route_mask(c);if mask==0 {return Err(HardwareError::Invalid);}
   let mut configured=0u8;let mut last=None;
   for (i,(_,_,m,o)) in PWM_ROUTES.iter().enumerate() {
    if mask&(1<<i)==0 {continue;}
    let p=&mut self.pwm[*m as usize];
    if configured&(1<<m)==0 {p.configure_frequency(h).map_err(|_|HardwareError::Invalid)?;configured|=1<<m;}
    let e=p.set_duty_permyriad(*o,d).map(effective).map_err(|_|HardwareError::Invalid)?;
    if last.is_some_and(|prior|prior!=e) {return Err(HardwareError::Invalid);}
    last=Some(e);
   }
   last.ok_or(HardwareError::Invalid)
  })();
  if result.is_err(){self.stop();}result
 }
 fn start(&mut self,c:u16)->Result<(),HardwareError>{
  let result=(|| {
   if c==ALL_MOTOR_SELECTOR {for p in &mut self.pwm {p.start().map_err(|_|HardwareError::Transport)?;}}
   else {let(m,_)=route(c)?;self.pwm[m].start().map_err(|_|HardwareError::Transport)?;}
   Ok(())
  })();
  if result.is_err(){self.stop();}result
 }
 fn pulse(&mut self,c:u16,n:u32)->Result<Effective,HardwareError>{let(m,o)=route(c)?;self.pwm[m].single_shot_ns(o,n).map(effective).map_err(|_|HardwareError::Invalid)}
 fn pulse_done(&mut self,c:u16)->bool{route(c).is_ok_and(|(m,_)|self.pwm[m].poll_single_shot())}
 fn enables(&mut self,mask:u16)->Result<(),HardwareError>{
  // Inhibit input gating first when disabling; keep enable assertions explicit.
  if mask==0 {
   // Independent inhibit paths: attempt both even if the first confirmation fails.
   let inl=self.drivers.set_inl_enabled(false);
   let drv=self.drivers.set_enabled(false);
   return if inl.is_ok() && drv.is_ok() {Ok(())}else{Err(HardwareError::Transport)};
  }
  self.drivers.set_inl_enabled(false).map_err(|_|HardwareError::Transport)?;
  self.drivers.set_enabled(mask&1!=0).map_err(|_|HardwareError::Transport)?;
  self.drivers.set_inl_enabled(mask&2!=0).map_err(|_|HardwareError::Transport)
 }
 fn transfer(&mut self,d:u16,f:u16)->Result<u16,HardwareError>{self.drivers.transfer(d as usize,f).map_err(|_|HardwareError::Transport)}
 fn adc(&mut self,c:usize)->Result<u16,HardwareError>{self.analog.read(c).map(|r|r.raw).map_err(|_|HardwareError::Transport)}
 fn diagnostic_sample(&mut self,c:u32,cycles:u32,mode:u32,offset:u32)->Result<mainboard_characterization::DiagnosticSample,HardwareError>{
  if c>3 || mode>1 {return Err(HardwareError::Invalid);}
  if mode==1 {self.pwm[0].adc_trigger_configure(offset).map_err(|_|HardwareError::Invalid)?;}
  let counters=[self.pwm[0].counter(),self.pwm[1].counter(),self.pwm[2].counter(),self.pwm[3].counter(),self.pwm[0].counter()];
  let pwm=&mut self.pwm[0];
  let raw=self.analog.capture([13,12,16,17][c as usize],cycles,mode==1,|on|pwm.adc_trigger_enable(on));
  let flags=if mode==1 {pwm.adc_trigger_flags()}else{0};let eoc=pwm.counter();
  let raw=raw.map_err(|_|HardwareError::Transport)?.raw;
  if mode==1 && flags&1==0 {return Err(HardwareError::Transport);}
  Ok(mainboard_characterization::DiagnosticSample{raw,counters,flags,eoc})
 }
 fn faults(&self)->u16{(0..4).fold(0,|v,i|v|if self.drivers.fault_asserted(i).unwrap_or(true){1<<i}else{0})}
}
