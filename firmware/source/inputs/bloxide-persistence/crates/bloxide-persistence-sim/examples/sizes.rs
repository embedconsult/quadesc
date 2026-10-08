use bloxide_persistence::{PersistenceService, Schema, SchemaError};
use bloxide_persistence_calibration::Broker;

#[derive(Clone, Copy)]
struct MaximumSchema;

impl Schema for MaximumSchema {
    fn id(&self) -> u16 {
        1
    }

    fn payload_len(&self) -> u16 {
        256
    }

    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        out.fill(0);
        Ok(())
    }

    fn validate(&self, bytes: &[u8]) -> Result<(), SchemaError> {
        (bytes.len() == 256)
            .then_some(())
            .ok_or(SchemaError::InvalidValue)
    }
}

fn main() {
    let service = core::mem::size_of::<PersistenceService<MaximumSchema>>();
    let broker = core::mem::size_of::<Broker<4>>();
    println!("service_bytes={service}");
    println!("broker_4_bytes={broker}");
    println!("combined_bytes={}", service + broker);
    println!(
        "service_align={}",
        core::mem::align_of::<PersistenceService<MaximumSchema>>()
    );
    println!("broker_4_align={}", core::mem::align_of::<Broker<4>>());
}
