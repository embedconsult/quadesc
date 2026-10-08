//! Host-only actual P service and SxI stream fixture. Identity bytes are loaded
//! at startup from a separately hashed host bundle; zero default is inadmissible.
use bloxide_calibration::{
    ApplyField, ApplyRequest, CommitGate, CommitRecord, EncodedValue, GateError,
    OperationKey as OwnerKey, Owner, OwnerOutcome, RejectReason as OwnerReject,
};
use bloxide_persistence::{
    Classification, Geometry, PersistenceService, Schema, SchemaError, recover,
};
use bloxide_persistence_calibration::{Broker, CanonicalCalibration};
use bloxide_persistence_sim::Simulator;
use std::io::{self, BufRead, Write};
use xcp_profile_p::{Admission, Domain, ProfileP, Reply, ScalarError};
use xcp_sxi::{Encoder, ParseEvent, Parser};

#[derive(Clone, Copy, Eq, PartialEq)]
struct Led {
    period: u16,
    duty: u16,
}
impl CanonicalCalibration for Led {
    fn encode(self, out: &mut [u8; 256]) -> u16 {
        out[..2].copy_from_slice(&self.period.to_le_bytes());
        out[2..4].copy_from_slice(&self.duty.to_le_bytes());
        4
    }
}
#[derive(Clone, Copy)]
struct LedSchema;
impl Schema for LedSchema {
    fn id(&self) -> u16 {
        1
    }
    fn payload_len(&self) -> u16 {
        4
    }
    fn defaults(&self, out: &mut [u8; 256]) -> Result<(), SchemaError> {
        Led {
            period: 1000,
            duty: 500,
        }
        .encode(out);
        Ok(())
    }
    fn validate(&self, b: &[u8]) -> Result<(), SchemaError> {
        if b.len() != 4 {
            return Err(SchemaError::InvalidValue);
        }
        let p = u16::from_le_bytes([b[0], b[1]]);
        let d = u16::from_le_bytes([b[2], b[3]]);
        if (100..=10000).contains(&p) && d <= 1000 {
            Ok(())
        } else {
            Err(SchemaError::InvalidValue)
        }
    }
}
struct Gate;
impl CommitGate<Led> for Gate {
    type Permit = ();
    fn try_reserve(&mut self, _: &Led) -> Result<(), GateError> {
        Ok(())
    }
    fn publish(&mut self, _: (), _: CommitRecord<Led>) {}
}
struct LedDomain {
    owner: Owner<Led>,
    seq: u64,
}
impl Domain<Led> for LedDomain {
    fn owner(&self) -> &Owner<Led> {
        &self.owner
    }
    fn read_scalar(&self, address: u32) -> Option<[u8; 2]> {
        match address {
            0x1000 => Some(self.owner.active().period.to_le_bytes()),
            0x1002 => Some(self.owner.active().duty.to_le_bytes()),
            _ => None,
        }
    }
    fn write_scalar(&mut self, address: u32, value: [u8; 2], now: u64) -> Result<(), ScalarError> {
        let id = match address {
            0x1000 => 1,
            0x1002 => 2,
            _ => return Err(ScalarError::Bounds),
        };
        let n = u16::from_le_bytes(value);
        if (id == 1 && !(100..=10000).contains(&n)) || (id == 2 && n > 1000) {
            return Err(ScalarError::Bounds);
        }
        let key = OwnerKey {
            service_epoch: 7,
            session_generation: 3,
            sequence: self.seq,
        };
        self.seq += 1;
        let req = ApplyRequest {
            field: ApplyField {
                key,
                field_id: id,
                encoded_value: EncodedValue::try_from_slice(&value).unwrap(),
                expires_at_us: now + 100,
            },
            expected_revision: None,
        };
        let result = self.owner.apply(
            req,
            || now,
            |mut led, id, v| {
                let n = u16::from_le_bytes(v.as_slice().try_into().unwrap());
                match id {
                    1 => led.period = n,
                    2 => led.duty = n,
                    _ => return Err(OwnerReject::UnknownVariable),
                }
                Ok(led)
            },
            &mut Gate,
        );
        self.owner.release(key).unwrap();
        match result {
            OwnerOutcome::Applied { .. } => Ok(()),
            _ => Err(ScalarError::Policy),
        }
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn main() {
    let identity = std::env::var("XCP_P_IDENTITY")
        .ok()
        .or_else(|| std::env::args().nth(1))
        .map(|path| std::fs::read(path).expect("identity"))
        .expect("matched host identity path required");
    let mut block = [0u8; 128];
    block.copy_from_slice(&identity);
    let mut p = ProfileP::new(block, 1).expect("profile P identity layout");
    let mut d = LedDomain {
        owner: Owner::new(
            Led {
                period: 1000,
                duty: 500,
            },
            7,
            3,
        ),
        seq: 1,
    };
    let g = Geometry::new(1024, 8).unwrap();
    let mut sim = Simulator::new(g, 100, 44);
    let config = sim.open_session().unwrap();
    let recovery = recover(Classification::Empty, Classification::Empty, &LedSchema).unwrap();
    let mut service = PersistenceService::new(g, LedSchema, recovery, true, 1, 1, 1, 0, config);
    let mut broker = Broker::<4>::new(1, 1);
    let mut parser = Parser::new();
    let mut encoder = Encoder::new();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut words = line.split_whitespace();
        let op = words.next().unwrap_or("");
        let now = words
            .next()
            .and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(0);
        match op {
            "feed" => {
                for token in words {
                    let byte = u8::from_str_radix(token, 16).unwrap();
                    match parser.feed_byte(now, byte) {
                        ParseEvent::Frame(frame) => {
                            println!("rx {}", hex(frame.packet.as_slice()));
                            let r = p.handle(
                                &frame.packet,
                                &mut d,
                                &LedSchema,
                                &mut service,
                                &mut broker,
                                Admission {
                                    disarmed: true,
                                    maintenance: true,
                                    schema_known: true,
                                },
                                0,
                                || 0,
                            );
                            match r {
                                Reply::Packet(packet) => {
                                    println!("tx {}", hex(encoder.encode(&packet).as_slice()))
                                }
                                Reply::Fenced => println!("fenced"),
                                Reply::Ignored => println!("ignored"),
                            }
                        }
                        ParseEvent::Discarded(why) => println!("discard {why:?}"),
                        ParseEvent::Recovered => println!("recovered"),
                        ParseEvent::Pending => {}
                    }
                }
            }
            "poll" => {
                if let ParseEvent::Discarded(why) = parser.poll(now) {
                    println!("discard {why:?}")
                }
            }
            "idle" => {
                println!("{:?}", parser.observe_idle(now))
            }
            "drive" => {
                for _ in 0..1000 {
                    if service.ownership_settled() {
                        break;
                    }
                    let command = service.take_command(0).unwrap().unwrap();
                    service.complete(sim.execute(command)).unwrap();
                }
                p.observe_all(&mut service, &mut broker);
                println!("settled {}", service.ownership_settled())
            }
            "loss" => {
                p.fence_wire();
                println!("fenced")
            }
            "quit" => {
                println!("done");
                io::stdout().flush().unwrap();
                break;
            }
            _ => println!("unknown"),
        }
        println!("done");
        io::stdout().flush().unwrap();
    }
}
