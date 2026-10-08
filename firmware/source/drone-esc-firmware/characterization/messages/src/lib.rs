#![no_std]
#[derive(Clone,Copy,Debug)]
pub struct Provider { pub request:xcp_core::ProviderRequest, pub now_us:u64 }
#[derive(Clone,Copy,Debug)]
pub struct Tick { pub now_us:u64 }
#[derive(Clone,Copy,Debug)]
pub enum MainboardMsg { Provider(Provider), Tick(Tick), TransportLost }
