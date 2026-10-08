#![no_std]
//! Application policy and bounded command lifecycle, independent of executor/HAL.
use xcp_core::{ProviderOperation, ProviderRequest};
use xcp_messages::{ProviderCompletion, ReadData, ReadResult, RejectionReason as Reject, WriteResult};
#[derive(Clone, Copy)]
pub struct Field { pub id:u32, pub width:u8, pub min:f32, pub max:f32, pub initial:u32, pub writable:bool, pub float:bool }
include!("generated.rs");
/// Flags: 1 computed+raw valid+fresh, 2 rail, 4 unmapped, 8 assumed/unverified CSA,
/// 16 nominal electrical assumptions, 32 stale, 64 no Celsius law, 128 invalid math/range.
/// Linear channels: raw*gain+offset. NTC: (10000*raw/(4096-raw))*gain+offset.
pub fn convert_adc(index:usize,raw:u16,cal:[f32;2])->(f32,u32) {
    let mut flags=1u32|ADC_ASSUMPTIONS[index];
    if raw==0 || raw>=4095 {flags|=2;flags&=!1;}
    if ADC_KIND[index]==3 {flags|=4;}
    let nominal=if ADC_KIND[index]==2 {10000.0*raw as f32/(4096.0-raw as f32)}else{raw as f32};
    let result=nominal*cal[0]+cal[1];
    if raw>4095 || !result.is_finite() || result.abs()>1e9 {return (0.0,(flags|128)&!1);}
    (result,flags)
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Effective { pub frequency:u32, pub high_ns:u32, pub period:u32, pub high:u32, pub divider:u32 }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HardwareError { Transport, Invalid }
#[derive(Clone,Copy,Debug,Default)]
pub struct DiagnosticSample {pub raw:u16,pub counters:[u32;5],pub flags:u32,pub eoc:u32}
pub trait Hardware {
    fn start_quadrature(&mut self)->Result<[u32;4],HardwareError>{Err(HardwareError::Invalid)}
    fn adc_bounded(&mut self,c:usize)->Result<u16,HardwareError>{self.adc(c)}
    fn diagnostic_sample(&mut self,_c:u32,_cycles:u32,_mode:u32,_offset:u32)->Result<DiagnosticSample,HardwareError>{Err(HardwareError::Invalid)}
    fn stop(&mut self);
    fn configure(&mut self, channel:u16, hz:u32, duty:u16) -> Result<Effective,HardwareError>;
    fn start(&mut self, channel:u16) -> Result<(),HardwareError>;
    fn pulse(&mut self, channel:u16, ns:u32) -> Result<Effective,HardwareError>;
    fn pulse_done(&mut self, channel:u16) -> bool;
    fn enables(&mut self, mask:u16) -> Result<(),HardwareError>;
    fn transfer(&mut self, device:u16, frame:u16) -> Result<u16,HardwareError>;
    fn adc(&mut self, channel:usize) -> Result<u16,HardwareError>;
    fn faults(&self) -> u16;
}
/// Nonrenewing diagnostic window, measured from the physical enable assertion.
pub const DRV_DIAGNOSTIC_TIMEOUT_US:u64=5_000_000;
pub const ALL_MOTOR_SELECTOR:u16=13;
pub const ALL_MOTOR_ROUTE_MASK:u32=0x1fbf;
pub fn route_mask(channel:u16)->u32 {if channel==ALL_MOTOR_SELECTOR {ALL_MOTOR_ROUTE_MASK} else if channel<13 {1u32<<channel} else {0}}
/// One selected route or all twelve motor phases; reconfiguration is global.
/// Every configuration edit is staged. Only command writes act on hardware.
pub struct Controller<H> {
    pub hardware:H,
    values:[u32;COUNT],
    retained:Option<(ProviderOperation,WriteResult)>,
    retired:Option<ProviderOperation>,
    generation:u32,
    revision:u32,
    applied:bool,
    running:bool,
    shot:Option<(u16,u32,u64)>,
    enable_at:u64,
    diagnostic_timed_out:bool,
    reboot_pending:bool,
    csa:[Option<u16>;4],
    scan:[u16;30],
    scan_valid:u32,
    scan_errors:u32,
    scan_index:usize,
    scan_at:Option<u64>,
    next_sample:u64,
    bench_at:u64, bench_last_request:u64, bench_fresh:[Option<u64>;4],
    diag_at:u64, diag_last_request:u64, diag_raw:[u16;2048], diag_hist:[u32;32],
}
impl<H:Hardware> Controller<H> {
    pub fn new(mut hardware:H)->Self {
        hardware.stop(); let enable_known=hardware.enables(0).is_ok();
        Self { hardware,values:core::array::from_fn(|i|if i==DRV_ENABLE_STATE_KNOWN {enable_known as u32}else{FIELDS[i].initial}),retained:None,retired:None,
            generation:0,revision:0,applied:false,running:false,shot:None,enable_at:0,diagnostic_timed_out:false,reboot_pending:false,
            csa:[None;4],scan:[0;30],scan_valid:0,scan_errors:0,scan_index:0,scan_at:None,next_sample:0,bench_at:0,bench_last_request:0,bench_fresh:[None;4],diag_at:0,diag_last_request:0,diag_raw:[0;2048],diag_hist:[0;32] }
    }
    pub fn value(&self,index:usize)->u32 { self.values[index] }
    pub fn enable_state_known(&self)->bool {self.values[DRV_ENABLE_STATE_KNOWN]==1}
    pub fn calibration(&self)->[[f32;2];30] {
        core::array::from_fn(|i|[f32::from_bits(self.values[ADC_FIELDS[i][1]]),f32::from_bits(self.values[ADC_FIELDS[i][2]])])
    }
    /// Called once after validated flash restore, before exposing the generated actor.
    pub fn restore_calibration(&mut self,cal:[[f32;2];30])->bool {
        if !cal.iter().all(|v|v[0].is_finite() && v[0].abs()<=10000.0 && v[1].is_finite() && v[1].abs()<=1_000_000.0) {return false;}
        for (i,v) in cal.iter().enumerate(){self.values[ADC_FIELDS[i][1]]=v[0].to_bits();self.values[ADC_FIELDS[i][2]]=v[1].to_bits();}
        self.refresh_engineering();true
    }
    fn refresh_engineering(&mut self) {
        for i in 0..30 {
            let fields=ADC_FIELDS[i];let raw=self.values[8+i] as u16;
            let cal=[f32::from_bits(self.values[fields[1]]),f32::from_bits(self.values[fields[2]])];
            let (value,mut flags)=convert_adc(i,raw,cal);
            if self.values[ADC_VALID_MASK]&(1<<i)==0 {flags &= !1;}
            if self.values[ADC_AGE_MS]>250 {flags|=32;flags&=!1;}
            if ADC_KIND[i]==1 {
                let verified=ADC_MOTOR[i].checked_sub(1).and_then(|m|self.csa[m as usize]).is_some_and(|r|r & 0x6dc == 0x280);
                if !verified {flags=(flags|8)&!1;}
            }
            self.values[fields[0]]=value.to_bits();self.values[fields[3]]=flags;
        }
    }
    pub fn safe_off(&mut self) {
        if self.values[BENCH_STATUS]==1 {self.values[BENCH_STATUS]=3;}
        self.values[BENCH_EFFECTIVE_MODE]=0;self.values[BENCH_CHANNEL_MASK]=0;
        self.values[BENCH_PHASE_STATUS]=0;self.values[BENCH_LEASE_MS]=0;
        if self.values[ADC_DIAG_STATUS]==1 {self.values[ADC_DIAG_STATUS]=3;}
        self.hardware.stop();
        let confirmed=self.hardware.enables(0).is_ok();
        self.values[DRV_ENABLE_STATE_KNOWN]=confirmed as u32;
        if confirmed {self.values[DRV_APPLIED_ENABLE_MASK]=0;}
        else {self.values[DRV_STATUS]=6;}
        self.csa=[None;4];
        self.running=false;self.applied=false;self.values[PWM_APPLIED_ROUTE_MASK]=0;
        if let Some((_,sequence,_))=self.shot.take() { self.values[PWM_COMPLETED_SEQUENCE]=sequence; }
        self.values[PWM_STATUS]=5;
    }
    /// Called on transport loss, protocol disconnect/quiesce, or session reset.
    pub fn synchronize(&mut self,generation:u32) { self.safe_off();self.diagnostic_timed_out=false;self.reboot_pending=false;self.generation=generation; }
    pub fn can_save(&self)->bool { self.values[DRV_ENABLE_STATE_KNOWN]==1 && !self.reboot_pending && self.values[ADC_DIAG_STATUS]!=1 && self.values[BENCH_STATUS]!=1 && !self.running && self.shot.is_none() && self.values[DRV_APPLIED_ENABLE_MASK]==0 }
    fn bench_watchdog(&mut self,now:u64) -> bool {
        if self.values[BENCH_STATUS]!=1 {return false;}
        let elapsed=now.saturating_sub(self.bench_at);
        self.values[BENCH_ELAPSED_MS]=(elapsed/1000).min(330000) as u32;
        self.values[BENCH_LEASE_MS]=(1_500_000u64.saturating_sub(now.saturating_sub(self.bench_last_request))/1000) as u32;
        let mut stale=false;
        for m in 0..4 {
            let age=self.bench_fresh[m].map_or(elapsed,|at|now.saturating_sub(at));
            self.values[BENCH_AGE_M1+m]=self.bench_fresh[m].map_or(u32::MAX,|_|(age/1000).min(u32::MAX as u64) as u32);
            stale |= age>=100_000;
        }
        if elapsed>=330_000_000 || now.saturating_sub(self.bench_last_request)>=1_500_000 || stale {
            self.safe_off();self.values[BENCH_STATUS]=4;return true;
        }
        false
    }
    pub fn poll(&mut self,now:u64) {
        self.bench_watchdog(now);
        if self.values[ADC_DIAG_STATUS]==1 && (now.saturating_sub(self.diag_at)>=15_000_000 || now.saturating_sub(self.diag_last_request)>=1_500_000) {
            self.safe_off();self.values[ADC_DIAG_STATUS]=4;
        }
        if let Some((channel,sequence,deadline))=self.shot {
            if self.hardware.pulse_done(channel) {
                self.shot=None;self.values[PWM_APPLIED_ROUTE_MASK]=0;self.values[PWM_STATUS]=4;self.values[PWM_COMPLETED_SEQUENCE]=sequence;
            } else if now>=deadline { self.safe_off();self.values[PWM_STATUS]=9; }
        }
        self.enforce_driver_safety(now);
        if now>=self.next_sample {
            self.next_sample=now.saturating_add(1000);
            let index=self.scan_index;
            let therm=[13,12,16,17].iter().position(|i|*i==index);
            let bench=self.values[BENCH_STATUS]==1;
            let result=if bench {
                if let Some(m)=therm {
                    self.hardware.diagnostic_sample(m as u32,128,1,self.values[BENCH_EFFECTIVE_OFFSET]).and_then(|v|{
                        if v.flags&1==0 {return Err(HardwareError::Transport);}
                        self.values[BENCH_COUNTER0..=BENCH_COUNTER4].copy_from_slice(&v.counters);
                        self.values[BENCH_TRIGGER_FLAGS]=v.flags;
                        
                        self.bench_fresh[m]=Some(now);
                        let stat=BENCH_M1_MIN+m*3;
                        self.values[stat]=self.values[stat].min(v.raw as u32);
                        self.values[stat+1]=self.values[stat+1].max(v.raw as u32);
                        self.values[stat+2]+=1;
                        Ok(v.raw)
                    })
                } else {self.hardware.adc_bounded(index)}
            } else {self.hardware.adc(index)};
            match result {
                Ok(raw) if raw<=4095=>{self.scan[index]=raw;self.scan_valid|=1<<index;},
                _=>{self.scan[index]=0;self.scan_errors|=1<<index;if bench {self.safe_off();self.values[BENCH_STATUS]=4;}if self.values[ADC_DIAG_STATUS]==1 {self.safe_off();self.values[ADC_DIAG_STATUS]=4;}}
            }
            if let Some(m)=therm {
                let field=THERMAL_SAMPLE_M1+m;
                let raw=if self.scan_errors&(1<<index)==0 {self.scan[index] as u32}else{65535};
                self.values[field]=(self.values[field].wrapping_add(1<<16)&0xffff0000)|raw;
            }
            // At most ONE bounded conversion per 1ms tick; legacy scan still
            // advances normally, and both paths share this exclusive owner.
            if self.values[ADC_DIAG_STATUS]==1 {
                let result=self.hardware.diagnostic_sample(self.values[ADC_DIAG_CHANNEL],self.values[ADC_DIAG_WINDOW_CYCLES],self.values[ADC_DIAG_MODE],self.values[ADC_DIAG_OFFSET_TICKS]);
                match result {
                    Ok(v) if v.raw<=4095 => {
                        let n=self.values[ADC_DIAG_COUNT] as usize;if n<2048 {self.diag_raw[n]=v.raw;}
                        self.values[ADC_DIAG_COUNT]+=1;
                        let bin=((v.raw as i32-self.values[ADC_DIAG_CENTER] as i32+256).div_euclid(16)).clamp(0,31) as usize;self.diag_hist[bin]+=1;
                        self.values[ADC_DIAG_MIN]=self.values[ADC_DIAG_MIN].min(v.raw as u32);
                        self.values[ADC_DIAG_MAX]=self.values[ADC_DIAG_MAX].max(v.raw as u32);
                        self.values[ADC_DIAG_SUM]+=v.raw as u32;
                        if (v.raw as u32).abs_diff(self.values[ADC_DIAG_CENTER])>self.values[ADC_DIAG_DEVIATION] {self.values[ADC_DIAG_OUTLIERS]+=1;}
                        self.values[ADC_DIAG_TRIGGER_FLAGS]|=v.flags;
                        self.values[ADC_DIAG_EOC_COUNTER]=v.eoc;
                        self.values[ADC_DIAG_COUNTER0_BEFORE..=ADC_DIAG_COUNTER0_AFTER].copy_from_slice(&v.counters);
                        self.values[ADC_DIAG_DURATION_US]=now.saturating_sub(self.diag_at).min(u32::MAX as u64) as u32;
                        if self.values[ADC_DIAG_COUNT]==self.values[ADC_DIAG_SAMPLES] {self.values[ADC_DIAG_STATUS]=2;self.values[ADC_DIAG_RAW]=self.diag_raw[0] as u32;}
                    },
                    _=>{self.safe_off();self.values[ADC_DIAG_STATUS]=4;}
                }
                self.enforce_driver_safety(now);
            }
            if bench {self.enforce_driver_safety(now);}
            self.scan_index+=1;
            if self.scan_index==30 {
                for i in 0..30 { self.values[8+i]=self.scan[i] as u32; }
                self.values[ADC_VALID_MASK]=self.scan_valid;
                self.values[ADC_ERROR_MASK]=self.scan_errors;
                self.values[ADC_SCAN_SEQUENCE]=self.values[ADC_SCAN_SEQUENCE].wrapping_add(1);
                self.scan_at=Some(now);self.scan_index=0;self.scan_valid=0;self.scan_errors=0;
            }
        }
        self.values[ADC_AGE_MS]=self.scan_at.map_or(u32::MAX,|at| (now.saturating_sub(at)/1000).min(u32::MAX as u64) as u32);
        self.refresh_engineering();
    }
    fn diagnostic(&self)->bool { self.values[DRV_APPLIED_ENABLE_MASK]==1 }
    fn effective_faults(&mut self,_now:u64,_fresh:bool)->Option<u16> {
        let raw=self.hardware.faults();self.values[FAULT_ASSERTED_MASK]=raw as u32;
        Some(raw)
    }
    fn enforce_driver_safety(&mut self,now:u64)->bool {
        let faults=self.effective_faults(now,false).unwrap_or(u16::MAX);
        if self.values[DRV_ENABLE_STATE_KNOWN]==0 {return false;}
        let age=now.saturating_sub(self.enable_at);
        if self.diagnostic() && age>=DRV_DIAGNOSTIC_TIMEOUT_US {
            self.safe_off();self.diagnostic_timed_out=true;self.values[DRV_STATUS]=9;return true;
        } else if faults!=0 && self.values[DRV_APPLIED_ENABLE_MASK]!=0
            && !self.diagnostic() && age>=2000 {
            self.safe_off();self.values[PWM_STATUS]=6;self.values[DRV_STATUS]=6;return true;
        }
        false
    }
    pub fn handles(&self,request:ProviderRequest)->bool {
        match request {
            ProviderRequest::Read(r)=>r.descriptor_id>=3,
            ProviderRequest::Apply(w)=>w.descriptor_id>=3,
            ProviderRequest::ResolveOrCancel{operation}|ProviderRequest::ReleaseOutcome{operation}=>
                self.retained.is_some_and(|(op,_)|op==operation) || self.retired==Some(operation),
            _=>false,
        }
    }
    pub fn request(&mut self,request:ProviderRequest,now:u64)->ProviderCompletion {
        let shutdown_tripped=self.bench_watchdog(now) | self.enforce_driver_safety(now);
        // Only real provider traffic refreshes the communication lease.
        self.diag_last_request=now;
        self.bench_last_request=now;
        match request {
            ProviderRequest::Read(r)=>{
                let result=match FIELDS.iter().position(|f|f.id==r.descriptor_id) {
                    Some(i) if i>=2 && r.length>0 && r.offset.checked_add(r.length as u32).is_some_and(|end|end<=FIELDS[i].width as u32)=>{
                        let bytes=self.values[i].to_le_bytes();let mut data=[0;7];
                        data[..r.length as usize].copy_from_slice(&bytes[r.offset as usize..r.offset as usize+r.length as usize]);
                        ReadResult::Data(ReadData::new(data,r.length).unwrap())
                    },Some(_)=>ReadResult::OutOfRange,None=>ReadResult::AccessDenied,
                };ProviderCompletion::Read{correlation:r.correlation,result}
            }
            ProviderRequest::Apply(w)=>{
                let op=w.operation;
                let result=if let Some((prior,result))=self.retained { if op==prior {result} else {WriteResult::Busy} }
                else if self.retired.is_some_and(|p|p.service_epoch==op.service_epoch && p.session_generation==op.session_generation && p.sequence>=op.sequence) { WriteResult::Retired }
                else {
                    let result=if op.session_generation!=self.generation { Err(Reject::WrongLifecycle) }
                    else if now>=w.expires_at_us { Err(Reject::Expired) }
                    else if let Some(index)=FIELDS.iter().position(|f|f.id==w.descriptor_id) {
                        if w.length!=FIELDS[index].width { Err(Reject::BadEncoding) }
                        // A request discovering a trip cannot re-enable outputs in
                        // the same turn, even if the raw GPIO has gone clear.
                        else if shutdown_tripped && ((index==PWM_ACTION && matches!(u32::from_le_bytes(w.encoded_value),1|2|4))
                            || (index==DRV_ACTION && matches!(u32::from_le_bytes(w.encoded_value),2|3))) {Err(Reject::OutputUnavailable)}
                        else { self.write(index,u32::from_le_bytes(w.encoded_value),now) }
                    } else { Err(Reject::UnknownVariable) };
                    let result=match result { Ok(changed)=>{self.revision=self.revision.wrapping_add(1);WriteResult::Applied{changed,active_revision:self.revision}},Err(reason)=>WriteResult::Rejected{reason} };
                    self.retained=Some((op,result));result
                };
                ProviderCompletion::Write{correlation:op.correlation(),result}
            }
            ProviderRequest::ResolveOrCancel{operation}=>ProviderCompletion::Write{correlation:operation.correlation(),result:self.retained.filter(|(op,_)|*op==operation).map_or(WriteResult::Retired,|(_,r)|r)},
            ProviderRequest::ReleaseOutcome{operation}=>{
                if self.retained.is_some_and(|(op,_)|op==operation) {self.retained=None;self.retired=Some(operation);}
                ProviderCompletion::Released{correlation:operation.correlation()}
            }
            _=>unreachable!("adapter routes only owned effects"),
        }
    }
    fn write(&mut self,index:usize,value:u32,now:u64)->Result<bool,Reject> {
        if self.reboot_pending {return Err(Reject::WrongLifecycle)}
        let disable=(index==DRV_ACTION && (value==4 || value==3 && self.values[DRV_ENABLE_MASK]==0))
            || index==PWM_ACTION && value==3 || index==BENCH_ACTION && value==2
            || index==ADC_DIAG_ACTION && value==2;
        if self.values[DRV_ENABLE_STATE_KNOWN]==0 && matches!(index,PWM_ACTION|DRV_ACTION|BENCH_ACTION|ADC_DIAG_ACTION|REBOOT_ACTION) {
            if !disable {return Err(Reject::OutputUnavailable);}
            // Authorized retries use the normal command path and command telemetry.
        }
        let f=FIELDS[index];
        if index<2 {return Err(Reject::UnknownVariable)}
        if !f.writable {return Err(Reject::ReadOnly)}
        let physical=if f.float {f32::from_bits(value)}else{value as f32};
        if (f.float && (!physical.is_finite() || physical<f.min || physical>f.max)) || (!f.float && (value<f.min as u32 || value>f.max as u32)) {return Err(Reject::Bounds)}
        if index==BENCH_ACTION {
            if value==0 {return Ok(false);}
            if value==2 {self.safe_off();return if self.enable_state_known() {Ok(true)}else{Err(Reject::OutputUnavailable)};}
            if self.values[BENCH_STATUS]==1 || self.values[ADC_DIAG_STATUS]==1 || self.running || !self.applied
                || self.shot.is_some()
                || self.values[DRV_APPLIED_ENABLE_MASK]!=3 || now.saturating_sub(self.enable_at)<2000
                || self.values[PWM_ACTIVE_CHANNEL]!=13 || self.values[PWM_PERIOD_TICKS]!=640
                || self.values[PWM_HIGH_TICKS]!=320 || self.values[PWM_EFFECTIVE_FREQUENCY_HZ]!=50000
                || self.values[PWM_CLOCK_DIVIDER]!=1 || self.effective_faults(now,true).unwrap_or(u16::MAX)!=0 {
                return Err(Reject::OutputUnavailable);
            }
            let pre=match self.hardware.start_quadrature() {Ok(p)=>p,Err(_)=>{
                self.safe_off();self.values[BENCH_STATUS]=4;return Err(Reject::OutputUnavailable);
            }};
            self.values[BENCH_PRELOAD_PWM0..=BENCH_PRELOAD_PWM3].copy_from_slice(&pre);
            self.values[BENCH_STATUS]=1;self.values[BENCH_EFFECTIVE_MODE]=1;
            self.values[BENCH_CHANNEL_MASK]=(1<<13)|(1<<12)|(1<<16)|(1<<17);
            self.values[BENCH_TRIGGER]=6;self.values[BENCH_WINDOW_CYCLES]=128;
            self.values[BENCH_EFFECTIVE_OFFSET]=self.values[BENCH_OFFSET_TICKS];
            self.values[BENCH_PHASE_STATUS]=1;
            self.values[BENCH_ELAPSED_MS]=0;self.values[BENCH_LEASE_MS]=1500;
            self.values[BENCH_AGE_M1..=BENCH_AGE_M4].fill(u32::MAX);
            for m in 0..4 {let stat=BENCH_M1_MIN+m*3;self.values[stat..stat+3].copy_from_slice(&[4095,0,0]);}
            self.bench_at=now;self.bench_last_request=now;self.bench_fresh=[None;4];
            self.scan_index=0;self.scan_valid=0;self.scan_errors=0;
            self.running=true;self.values[PWM_STATUS]=2;return Ok(true);
        }
        // Only read-only DRV inspection and stop/disconnect remain available.
        // Freeze all staged settings/calibration to make mode identity unambiguous.
        if self.values[BENCH_STATUS]==1 && !matches!(index,DRV_DEVICE|DRV_REGISTER)
            && !(index==PWM_ACTION && value==3) && !(index==DRV_ACTION && matches!(value,1|4))
            && index!=REBOOT_ACTION {return Err(Reject::WrongLifecycle);}
        if index==ADC_DIAG_ACTION {
            if value==0 {return Ok(false);}
            if value==2 {self.safe_off();return if self.enable_state_known() {Ok(true)}else{Err(Reject::OutputUnavailable)};}
            if self.values[ADC_DIAG_STATUS]==1 {return Err(Reject::WrongLifecycle);}
            if self.shot.is_some()
                || ![128,256,448].contains(&self.values[ADC_DIAG_WINDOW_CYCLES])
                || self.values[ADC_DIAG_SAMPLES]==0
                || (self.values[ADC_DIAG_MODE]==1 && (!self.running || self.values[PWM_ACTIVE_CHANNEL]!=ALL_MOTOR_SELECTOR as u32
                    || ![20000,50000].contains(&self.values[PWM_EFFECTIVE_FREQUENCY_HZ])
                    || self.values[PWM_HIGH_TICKS]*2!=self.values[PWM_PERIOD_TICKS]
                    || self.values[ADC_DIAG_OFFSET_TICKS]>=self.values[PWM_PERIOD_TICKS])) {return Err(Reject::OutputUnavailable);}
            self.values[ADC_DIAG_SEQUENCE]=self.values[ADC_DIAG_SEQUENCE].wrapping_add(1);
            for i in ADC_DIAG_COUNT..=ADC_DIAG_COUNTER0_AFTER {self.values[i]=0;}
            self.values[ADC_DIAG_MIN]=4095;self.values[ADC_DIAG_CURSOR]=0;
            self.diag_hist=[0;32];self.diag_at=now;self.diag_last_request=now;self.values[ADC_DIAG_STATUS]=1;return Ok(true);
        }
        if index==ADC_DIAG_CURSOR {
            if self.values[ADC_DIAG_STATUS]!=2 || (value>=self.values[ADC_DIAG_COUNT].min(2048) && value<8192) {return Err(Reject::WrongLifecycle);}
            self.values[index]=value;self.values[ADC_DIAG_RAW]=if value>=8192 {self.diag_hist[(value-8192) as usize]}else{self.diag_raw[value as usize] as u32};return Ok(true);
        }
        if self.values[ADC_DIAG_STATUS]==1 && ((ADC_DIAG_MODE..ADC_DIAG_ACTION).contains(&index)
            || index==PWM_ACTION && value!=3 || index==DRV_ACTION && value!=4) {return Err(Reject::WrongLifecycle);}
        if index==REBOOT_ACTION {
            if value==0 {return Ok(false)}
            self.safe_off();
            // Do not acknowledge reset if the concrete output disable reports failure.
            if self.values[DRV_ENABLE_STATE_KNOWN]!=1 {return Err(Reject::OutputUnavailable);}
            self.reboot_pending=true;return Ok(true);
        }
        if index==PWM_ACTION { if value!=0 { self.pwm_command(value,now); } return if self.enable_state_known() {Ok(value!=0)}else{Err(Reject::OutputUnavailable)}; }
        if index==DRV_ACTION { if value!=0 { self.drv_command(value,now); } return if self.enable_state_known() {Ok(value!=0)}else{Err(Reject::OutputUnavailable)}; }
        let changed=self.values[index]!=value;self.values[index]=value;self.refresh_engineering();Ok(changed)
    }
    fn effective(&mut self,channel:u16,e:Effective) {
        self.values[PWM_ACTIVE_CHANNEL]=channel as u32;
        self.values[PWM_APPLIED_ROUTE_MASK]=route_mask(channel);
        self.values[PWM_EFFECTIVE_FREQUENCY_HZ]=e.frequency;
        self.values[PWM_EFFECTIVE_HIGH_NS]=e.high_ns;self.values[PWM_PERIOD_TICKS]=e.period;
        self.values[PWM_HIGH_TICKS]=e.high;self.values[PWM_CLOCK_DIVIDER]=e.divider;
    }
    fn pwm_command(&mut self,command:u32,now:u64) {
        if self.values[ADC_DIAG_STATUS]==1 || self.values[BENCH_STATUS]==1 {self.safe_off();}
        // Busy pulse rejects retrigger without replacing its completion sequence.
        if self.shot.is_some() && command!=3 { self.values[PWM_STATUS]=7; return; }
        self.values[PWM_COMMAND_SEQUENCE]=self.values[PWM_COMMAND_SEQUENCE].wrapping_add(1);
        let sequence=self.values[PWM_COMMAND_SEQUENCE];
        let channel=self.values[PWM_CHANNEL] as u16;
        // Mask1 is diagnostic-only. Even Apply is rejected, so no staged PWM
        // configuration can be used to start while DRV_ENABLE is diagnostically high.
        if self.diagnostic() && command!=3 {
            self.values[PWM_STATUS]=8;self.values[PWM_COMPLETED_SEQUENCE]=sequence;return;
        }
        if matches!(command,2|4) {
            let faults=self.effective_faults(now,true).unwrap_or(u16::MAX);
            if faults!=0 {self.safe_off();self.values[PWM_STATUS]=6;self.values[PWM_COMPLETED_SEQUENCE]=sequence;return;}
        }
        let result=match command {
            1=>{self.hardware.stop();self.running=false;self.applied=false;self.values[PWM_APPLIED_ROUTE_MASK]=0;
                match self.hardware.configure(channel,self.values[PWM_FREQUENCY_HZ],self.values[PWM_DUTY_PERCENT] as u16) {
                    Ok(e)=>{self.effective(channel,e);self.applied=true;self.values[PWM_STATUS]=1;Ok(())},Err(e)=>Err(e)}},
            2 if self.applied=>{let c=self.values[PWM_ACTIVE_CHANNEL] as u16;
                if c==ALL_MOTOR_SELECTOR && (self.values[DRV_APPLIED_ENABLE_MASK]!=3 || now.saturating_sub(self.enable_at)<2000) {self.safe_off();self.values[PWM_STATUS]=8;self.values[PWM_COMPLETED_SEQUENCE]=sequence;return;}
                self.hardware.start(c).map(|()|{self.running=true;self.values[PWM_STATUS]=2;})},
            3=>{if !self.enable_state_known() {self.safe_off();}self.hardware.stop();self.running=false;self.shot=None;self.applied=false;self.values[PWM_APPLIED_ROUTE_MASK]=0;self.values[PWM_STATUS]=5;Ok(())},
            4 if channel!=ALL_MOTOR_SELECTOR=>{self.hardware.stop();self.running=false;self.applied=false;self.values[PWM_APPLIED_ROUTE_MASK]=0;
                match self.hardware.pulse(channel,self.values[PULSE_WIDTH_NS]) {
                    Ok(e)=>{self.effective(channel,e);self.shot=Some((channel,sequence,now.saturating_add(self.values[PULSE_WIDTH_NS] as u64/1000+100_000)));
                        self.values[PWM_STATUS]=3;Ok(())},Err(e)=>Err(e)}},
            _=>Err(HardwareError::Invalid),
        };
        if let Err(e)=result {self.safe_off();self.values[PWM_STATUS]=if e==HardwareError::Invalid {8}else{6};}
        if self.shot.is_none() {self.values[PWM_COMPLETED_SEQUENCE]=sequence;}
    }
    fn drv_command(&mut self,command:u32,now:u64) {
        self.values[DRV_COMMAND_SEQUENCE]=self.values[DRV_COMMAND_SEQUENCE].wrapping_add(1);
        let mask=self.values[DRV_ENABLE_MASK] as u16;
        if command==4 {self.safe_off();let ok=self.enable_state_known();self.diagnostic_timed_out=false;self.values[DRV_STATUS]=if ok {4}else{6};return;}
        if command==3 {
            if mask==0 {self.safe_off();let ok=self.enable_state_known();self.diagnostic_timed_out=false;self.values[DRV_STATUS]=if ok {4}else{6};return;}
            // After diagnostics, only explicit drive-enable3 is admitted, with fresh clear
            // faults and completed wake. Keep DRV high to preserve configured PWM mode.
            // Timeout still requires an explicit disable.
            if self.diagnostic_timed_out || (self.diagnostic() && mask!=1
                && (mask!=3 || self.effective_faults(now,true).unwrap_or(u16::MAX)!=0 || now.saturating_sub(self.enable_at)<2000)) {
                self.values[DRV_STATUS]=9;return;
            }
            if mask&2!=0 && self.effective_faults(now,true).unwrap_or(u16::MAX)!=0 {self.safe_off();self.values[DRV_STATUS]=6;return;}
            // Repeated enable is a no-op: neither wake delay nor timeout can be renewed.
            if self.values[DRV_APPLIED_ENABLE_MASK]==mask as u32 {self.values[DRV_STATUS]=3;return;}
            if self.running || self.shot.is_some() {self.values[DRV_STATUS]=9;return;}
            self.hardware.stop();self.applied=false;self.values[PWM_APPLIED_ROUTE_MASK]=0;
            self.values[DRV_ENABLE_STATE_KNOWN]=0;
            if self.hardware.enables(mask).is_err() {self.safe_off();self.values[DRV_STATUS]=6;return;}
            if self.values[DRV_APPLIED_ENABLE_MASK]&1==0 || mask&1==0 {self.enable_at=now;}
            self.values[DRV_ENABLE_STATE_KNOWN]=1;
            self.values[DRV_APPLIED_ENABLE_MASK]=mask as u32;self.csa=[None;4];
            self.values[DRV_STATUS]=if mask==0 {4}else{3};return;
        }
        if self.values[DRV_APPLIED_ENABLE_MASK]&1==0 || now.saturating_sub(self.enable_at)<2000 {
            self.values[DRV_STATUS]=7;return;
        }
        let device=self.values[DRV_DEVICE] as u16;let reg=self.values[DRV_REGISTER] as u16;
        let value=self.values[DRV_WRITE_VALUE] as u16;
        if command==2 && (reg<2 || reg==2 && value&!0x3ff!=0) {self.values[DRV_STATUS]=8;return;}
        // Register edits occur only with all PWM stopped; they may change drive mode.
        if command==2 && (self.running || self.shot.is_some()) {self.values[DRV_STATUS]=9;return;}
        let result:Result<bool,HardwareError>=(|| {
            if command==2 {let raw=self.hardware.transfer(device,(reg<<11)|value)?;
                self.values[DRV_RAW_RESPONSE]=raw as u32;if raw==0xffff {return Ok(false);}}
            let raw=self.hardware.transfer(device,0x8000|(reg<<11))?;
            self.values[DRV_RAW_RESPONSE]=raw as u32;
            // Upper5 bits are don't-care. Do not use them as an identity check.
            if raw==0xffff || (reg==2 && raw&0x400!=0) {return Ok(false);}
            self.values[DRV_READBACK]=(raw&0x7ff) as u32;
            if reg==6 {self.csa[device as usize]=Some(raw&0x7ff);}
            Ok(true)
        })();
        self.values[DRV_STATUS]=match result {
            Err(_)=>{self.safe_off();6},Ok(false)=>{self.safe_off();5},
            Ok(true) if command==2=>{
                let compare_mask=if reg==2 {0x3fe}else{0x7ff}; // CLR_FLT self-clears
                if (self.values[DRV_READBACK] as u16 & compare_mask)==(value&compare_mask) {2}else{self.safe_off();9}
            },Ok(true)=>1,
        };
    }
}

impl<T:Hardware+?Sized> Hardware for &mut T {
 fn start_quadrature(&mut self)->Result<[u32;4],HardwareError>{(**self).start_quadrature()}
 fn adc_bounded(&mut self,c:usize)->Result<u16,HardwareError>{(**self).adc_bounded(c)}
 fn stop(&mut self){(**self).stop()}
 fn configure(&mut self,c:u16,h:u32,d:u16)->Result<Effective,HardwareError>{(**self).configure(c,h,d)}
 fn start(&mut self,c:u16)->Result<(),HardwareError>{(**self).start(c)}
 fn pulse(&mut self,c:u16,n:u32)->Result<Effective,HardwareError>{(**self).pulse(c,n)}
 fn pulse_done(&mut self,c:u16)->bool{(**self).pulse_done(c)}
 fn enables(&mut self,m:u16)->Result<(),HardwareError>{(**self).enables(m)}
 fn transfer(&mut self,d:u16,f:u16)->Result<u16,HardwareError>{(**self).transfer(d,f)}
 fn adc(&mut self,c:usize)->Result<u16,HardwareError>{(**self).adc(c)}
 fn diagnostic_sample(&mut self,c:u32,w:u32,m:u32,o:u32)->Result<DiagnosticSample,HardwareError>{(**self).diagnostic_sample(c,w,m,o)}
 fn faults(&self)->u16{(**self).faults()}
}
use core::cell::RefCell;
pub struct ReplySlot(critical_section::Mutex<RefCell<Option<ProviderCompletion>>>);
impl ReplySlot {
 pub const fn new()->Self {Self(critical_section::Mutex::new(RefCell::new(None)))}
 pub fn take(&self)->Option<ProviderCompletion>{critical_section::with(|cs| self.0.borrow(cs).borrow_mut().take())}
 fn put(&self,c:ProviderCompletion)->bool {critical_section::with(|cs|{let mut s=self.0.borrow(cs).borrow_mut();if s.is_some(){false}else{*s=Some(c);true}})}
}
pub struct Owner { pub control:Controller<&'static mut dyn Hardware>, reply:&'static ReplySlot, led_on:fn()->bool, calibration_stage:Option<fn([[f32;2];30])->bool> }
impl Owner {
 pub fn new(h:&'static mut dyn Hardware,reply:&'static ReplySlot,led_on:fn()->bool)->Self {Self{control:Controller::new(h),reply,led_on,calibration_stage:None}}
 pub fn attach_calibration_stage(&mut self,stage:fn([[f32;2];30])->bool){self.calibration_stage=Some(stage);}
 pub fn phase(&self)->u8 {
   if self.control.shot.is_some() {3} else if self.control.running {2} else if self.control.applied {1}
   else if self.control.values[PWM_STATUS]==4 {4} else if matches!(self.control.values[PWM_STATUS],6|8|9) {5} else {0}
 }
}
/// Lifecycle preflight refusals must reach XCP unchanged; LED completion cannot hide them.
pub fn completion_requires_fence(completion:ProviderCompletion)->bool {
    match completion {
        ProviderCompletion::CalibrationStored{verified:false,..}=>true,
        ProviderCompletion::Synchronized{result,..}=>!matches!(result,xcp_messages::SynchronizeResult::Ready{..}),
        ProviderCompletion::Quiesced{result,..}=>result!=xcp_messages::QuiesceResult::Quiesced,
        _=>false,
    }
}
pub fn lifecycle_stop(owner:&mut Owner){owner.control.safe_off();}
pub fn provider(owner:&mut Owner,event:&mainboard_messages::Provider){
 owner.control.values[LED_ON]=(owner.led_on)() as u32;
 let completion=match event.request {
  ProviderRequest::Synchronize{session_generation}=>{owner.control.synchronize(session_generation);ProviderCompletion::Synchronized{session_generation,result:if owner.control.values[DRV_ENABLE_STATE_KNOWN]==1 {xcp_messages::SynchronizeResult::Ready{service_epoch:1}}else{xcp_messages::SynchronizeResult::Uncertain}}},
  ProviderRequest::Quiesce{correlation}=>{owner.control.safe_off();let ok=owner.control.enable_state_known();ProviderCompletion::Quiesced{correlation,result:if ok {xcp_messages::QuiesceResult::Quiesced}else{xcp_messages::QuiesceResult::Uncertain}}},
  ProviderRequest::StoreCalibration{correlation}=>ProviderCompletion::CalibrationStored{correlation,verified:owner.control.can_save() && owner.calibration_stage.is_some_and(|stage|stage(owner.control.calibration()))},
  request=>owner.control.request(request,event.now_us),
 };
 if !owner.reply.put(completion) {owner.control.safe_off();owner.control.values[PWM_STATUS]=6;}
}
pub fn tick(owner:&mut Owner,event:&mainboard_messages::Tick){owner.control.poll(event.now_us);}

/// Transport-owned one-shot reset gate. Admission is the matching successful provider
/// completion; reset permission requires successful transmission of its positive ACK.
#[derive(Default)]
pub struct RebootAfterAck { armed:bool }
impl RebootAfterAck {
    pub fn observe(&mut self,request:ProviderRequest,completion:ProviderCompletion) {
        if let (ProviderRequest::Apply(w),ProviderCompletion::Write{correlation,result:WriteResult::Applied{changed:true,..}})=(request,completion) {
            if w.operation.correlation()==correlation && w.descriptor_id==FIELDS[REBOOT_ACTION].id
                && w.length==2 && w.encoded_value==1u32.to_le_bytes() {self.armed=true;}
        }
    }
    pub fn response_sent(&mut self,bytes:&[u8],transmitted:bool)->bool {
        let reset=self.armed && transmitted && bytes==[0xff];
        self.armed=false;reset
    }
}
