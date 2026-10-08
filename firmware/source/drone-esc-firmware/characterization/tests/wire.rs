use mainboard_characterization::*;
use xcp_core::*;
#[derive(Default)]struct Port{requests:std::collections::VecDeque<ProviderRequest>}
impl ProviderPort for Port{fn try_submit(&mut self,r:ProviderRequest)->Result<(),SubmitError>{self.requests.push_back(r);Ok(())}}
struct NoHardware;
impl Hardware for NoHardware{
 fn stop(&mut self){} fn configure(&mut self,_:u16,_:u32,_:u16)->Result<Effective,HardwareError>{panic!("staging must not touch hardware")}
 fn start(&mut self,_:u16)->Result<(),HardwareError>{panic!()} fn pulse(&mut self,_:u16,_:u32)->Result<Effective,HardwareError>{panic!()}
 fn pulse_done(&mut self,_:u16)->bool{false} fn enables(&mut self,m:u16)->Result<(),HardwareError>{assert_eq!(m,0);Ok(())}
 fn transfer(&mut self,_:u16,_:u16)->Result<u16,HardwareError>{panic!()}fn adc(&mut self,_:usize)->Result<u16,HardwareError>{panic!()}fn faults(&self)->u16{0}
}
fn packet(b:&[u8])->Packet{Packet::try_from_slice(b).unwrap()}
#[test]
fn actual_xcp_download_upload_u32_and_partial_write_rejection(){
 let map=VirtualMap::new(&REGIONS).unwrap();let mut s=Session::new();let mut port=Port::default();let mut owner=Controller::new(NoHardware);
 assert_eq!(s.handle_packet(&map,&packet(&[0xff,0]),0,&mut port),Dispatch::Deferred);
 let ProviderRequest::Synchronize{session_generation}=port.requests.pop_front().unwrap() else{panic!()};owner.synchronize(session_generation);
 assert!(matches!(s.complete(ProviderCompletion::Synchronized{session_generation,result:SynchronizeResult::Ready{service_epoch:1}},&mut port),Dispatch::Respond(_)));
 let address=REGIONS[PWM_FREQUENCY_HZ].address;
 let mut mta=vec![0xf6,0,0,0];mta.extend_from_slice(&address.to_le_bytes());
 s.handle_packet(&map,&packet(&mta),1,&mut port);
 assert_eq!(s.handle_packet(&map,&packet(&[0xf0,4,0x40,0x0d,3,0]),2,&mut port),Dispatch::Deferred); //200000Hz
 let apply=port.requests.pop_front().unwrap();let completion=owner.request(apply,3);
 assert_eq!(owner.value(PWM_FREQUENCY_HZ),200000);
 assert_eq!(s.complete(completion,&mut port),Dispatch::Respond(packet(&[0xff])));
 let release=port.requests.pop_front().unwrap();s.complete(owner.request(release,4),&mut port);
 s.handle_packet(&map,&packet(&mta),5,&mut port);
 assert_eq!(s.handle_packet(&map,&packet(&[0xf5,4]),6,&mut port),Dispatch::Deferred);
 let read=port.requests.pop_front().unwrap();assert_eq!(s.complete(owner.request(read,7),&mut port),Dispatch::Respond(packet(&[0xff,0x40,0x0d,3,0])));
 s.handle_packet(&map,&packet(&mta),8,&mut port);
 assert_eq!(s.handle_packet(&map,&packet(&[0xf0,2,0,0]),9,&mut port),Dispatch::Respond(packet(&[0xfe,0x22])));
 assert_eq!(owner.value(PWM_FREQUENCY_HZ),200000);
}

#[test]
fn float32_xcp_wire_download_upload_preserves_signed_fraction(){
 let map=VirtualMap::new(&REGIONS).unwrap();let mut s=Session::new();let mut port=Port::default();let mut owner=Controller::new(NoHardware);
 s.handle_packet(&map,&packet(&[0xff,0]),0,&mut port);
 let ProviderRequest::Synchronize{session_generation}=port.requests.pop_front().unwrap() else{panic!()};owner.synchronize(session_generation);
 s.complete(ProviderCompletion::Synchronized{session_generation,result:SynchronizeResult::Ready{service_epoch:1}},&mut port);
 let address=REGIONS[ADC_FIELDS[0][1]].address;let mut mta=vec![0xf6,0,0,0];mta.extend_from_slice(&address.to_le_bytes());
 s.handle_packet(&map,&packet(&mta),1,&mut port);
 let bytes=(-0.040283203125f32).to_le_bytes();let mut download=vec![0xf0,4];download.extend_from_slice(&bytes);
 assert_eq!(s.handle_packet(&map,&packet(&download),2,&mut port),Dispatch::Deferred);
 let apply=port.requests.pop_front().unwrap();assert_eq!(s.complete(owner.request(apply,3),&mut port),Dispatch::Respond(packet(&[0xff])));
 let release=port.requests.pop_front().unwrap();s.complete(owner.request(release,4),&mut port);
 s.handle_packet(&map,&packet(&mta),5,&mut port);s.handle_packet(&map,&packet(&[0xf5,4]),6,&mut port);
 let read=port.requests.pop_front().unwrap();let mut expected=vec![0xff];expected.extend_from_slice(&bytes);
 assert_eq!(s.complete(owner.request(read,7),&mut port),Dispatch::Respond(packet(&expected)));
}

#[test]
fn reboot_wire_requires_full_valid_download_then_positive_transmitted_ack() {
 for (data,armed) in [(vec![0xf5,2],false),(vec![0xf0,1,1],false),(vec![0xf0,2,2,0],false),(vec![0xf0,2,0,0],false),(vec![0xf0,2,1,0],true)] {
  let map=VirtualMap::new(&REGIONS).unwrap();let mut s=Session::new();let mut port=Port::default();let mut owner=Controller::new(NoHardware);let mut gate=RebootAfterAck::default();
  s.handle_packet(&map,&packet(&[0xff,0]),0,&mut port);
  let ProviderRequest::Synchronize{session_generation}=port.requests.pop_front().unwrap() else{panic!()};owner.synchronize(session_generation);
  s.complete(ProviderCompletion::Synchronized{session_generation,result:SynchronizeResult::Ready{service_epoch:1}},&mut port);
  let mut mta=vec![0xf6,0,0,0];mta.extend_from_slice(&0x1400u32.to_le_bytes());s.handle_packet(&map,&packet(&mta),1,&mut port);
  let mut dispatch=s.handle_packet(&map,&packet(&data),2,&mut port);
  while let Some(request)=port.requests.pop_front() {
   let completion=owner.request(request,3);gate.observe(request,completion);
   let next=s.complete(completion,&mut port);if matches!(next,Dispatch::Respond(_)) {dispatch=next;}
  }
  let Dispatch::Respond(response)=dispatch else{panic!("no response: {dispatch:?}")};
  assert_eq!(gate.response_sent(response.as_slice(),true),armed);
  assert_eq!(owner.value(REBOOT_ACTION),0);
 }
}
