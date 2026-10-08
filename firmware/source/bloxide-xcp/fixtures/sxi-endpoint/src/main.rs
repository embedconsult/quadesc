//! Host-only line driver for the real allocation-free SxI adapter and Session.
//! Its provider is a deterministic qualification fake, not the T11 LED owner.

use std::io::{self, BufRead, Write};
use xcp_core::{ProviderOperation, ProviderPort, ProviderRequest, Region, SubmitError, VirtualMap};
use xcp_messages::{
    ProviderCompletion, QuiesceResult, ReadData, ReadResult, RejectionReason, SynchronizeResult,
    WriteResult,
};
use xcp_sxi::{with_adapter, Adapter, AdapterEvent};

const BUILD_ID: [u8; 32] = [0xcd; 32];
const SCHEMA_ID: [u8; 32] = [0xab; 32];

#[derive(Default)]
struct Port {
    pending: Option<ProviderRequest>,
}

impl ProviderPort for Port {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        if self.pending.is_some() {
            Err(SubmitError::Busy)
        } else {
            self.pending = Some(request);
            Ok(())
        }
    }
}

struct Owner {
    period_ms: u16,
    duty_permille: u16,
    active_revision: u32,
    phase_us: u32,
    retained: Option<(ProviderOperation, WriteResult)>,
}

impl Owner {
    fn new() -> Self {
        Self {
            period_ms: 1000,
            duty_permille: 500,
            active_revision: 0,
            phase_us: 0x1234,
            retained: None,
        }
    }

    fn complete(&mut self, request: ProviderRequest) -> ProviderCompletion {
        match request {
            ProviderRequest::Synchronize { session_generation } => {
                ProviderCompletion::Synchronized {
                    session_generation,
                    result: SynchronizeResult::Ready { service_epoch: 1 },
                }
            }
            ProviderRequest::Read(request) => {
                let source = self.read_region(request.descriptor_id);
                let start = request.offset as usize;
                let end = start + request.length as usize;
                let mut bytes = [0; 7];
                let result = if end <= source.len() {
                    bytes[..request.length as usize].copy_from_slice(&source[start..end]);
                    ReadResult::Data(ReadData::new(bytes, request.length).unwrap())
                } else {
                    ReadResult::AccessDenied
                };
                ProviderCompletion::Read {
                    correlation: request.correlation,
                    result,
                }
            }
            ProviderRequest::Apply(request) => {
                let value =
                    u16::from_le_bytes([request.encoded_value[0], request.encoded_value[1]]);
                let (active, valid) = match request.descriptor_id {
                    2 => (&mut self.period_ms, (100..=10_000).contains(&value)),
                    3 => (&mut self.duty_permille, value <= 1000),
                    _ => {
                        let result = WriteResult::Rejected {
                            reason: RejectionReason::UnknownVariable,
                        };
                        self.retained = Some((request.operation, result));
                        return ProviderCompletion::Write {
                            correlation: request.operation.correlation(),
                            result,
                        };
                    }
                };
                let result = if valid {
                    let changed = *active != value;
                    if changed {
                        *active = value;
                        self.active_revision = self.active_revision.wrapping_add(1);
                        self.phase_us = self.phase_us.wrapping_add(1);
                    }
                    WriteResult::Applied {
                        changed,
                        active_revision: self.active_revision,
                    }
                } else {
                    WriteResult::Rejected {
                        reason: RejectionReason::Bounds,
                    }
                };
                self.retained = Some((request.operation, result));
                ProviderCompletion::Write {
                    correlation: request.operation.correlation(),
                    result,
                }
            }
            ProviderRequest::ResolveOrCancel { operation } => {
                let result = self
                    .retained
                    .filter(|(key, _)| *key == operation)
                    .map_or(WriteResult::Cancelled, |(_, result)| result);
                if self.retained.is_none() {
                    self.retained = Some((operation, result));
                }
                ProviderCompletion::Write {
                    correlation: operation.correlation(),
                    result,
                }
            }
            ProviderRequest::Quiesce { correlation } => ProviderCompletion::Quiesced {
                correlation,
                result: QuiesceResult::Quiesced,
            },
            ProviderRequest::ReleaseOutcome { operation } => {
                if self.retained.is_some_and(|(key, _)| key == operation) {
                    self.retained = None;
                }
                ProviderCompletion::Released {
                    correlation: operation.correlation(),
                }
            }
        }
    }

    fn read_region(&self, descriptor_id: u32) -> Vec<u8> {
        match descriptor_id {
            1 => [BUILD_ID.as_slice(), SCHEMA_ID.as_slice()].concat(),
            2 => self.period_ms.to_le_bytes().to_vec(),
            3 => self.duty_permille.to_le_bytes().to_vec(),
            4 => {
                let mut data = vec![0; 36];
                data[8..12].copy_from_slice(&self.active_revision.to_le_bytes());
                data[12..16].copy_from_slice(&self.phase_us.to_le_bytes());
                data[16..18].copy_from_slice(&self.period_ms.to_le_bytes());
                data[18..20].copy_from_slice(&self.duty_permille.to_le_bytes());
                data
            }
            _ => Vec::new(),
        }
    }
}

fn main() {
    let regions = [
        Region::read_only(1, 0, 64),
        Region::calibration(2, 0x1000, 2),
        Region::calibration(3, 0x1002, 2),
        Region::read_only(4, 0x2000, 36),
    ];
    let map = VirtualMap::new(&regions).unwrap();
    with_adapter(|mut adapter| {
        let mut port = Port::default();
        let mut owner = Owner::new();
        let mut drop_next_tx = false;

        for line in io::stdin().lock().lines() {
            let line = line.unwrap();
            let mut fields = line.split_whitespace();
            let command = fields.next().unwrap_or("");
            let now_us = fields
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            match command {
                "feed" => {
                    let bytes = fields.map(|value| u8::from_str_radix(value, 16).unwrap());
                    for byte in bytes {
                        let event = adapter.feed_byte(now_us, byte, &mut port);
                        println!("event {event:?}");
                    }
                    drive(
                        &mut adapter,
                        &map,
                        &mut port,
                        &mut owner,
                        now_us,
                        &mut drop_next_tx,
                    );
                }
                "poll" => {
                    println!("event {:?}", adapter.poll(now_us, &mut port));
                    drive(
                        &mut adapter,
                        &map,
                        &mut port,
                        &mut owner,
                        now_us,
                        &mut drop_next_tx,
                    );
                }
                "idle" => println!("event {:?}", adapter.observe_idle(now_us, &mut port)),
                "loss" | "stop" | "reset" => {
                    adapter.lifecycle_fence(now_us, &mut port);
                    println!("event LifecycleFence");
                }
                "uart-error" => println!("event {:?}", adapter.uart_error(now_us, &mut port)),
                "drop-next-tx" => {
                    drop_next_tx = true;
                    println!("event DropArmed");
                }
                "status" => println!(
                "status state={:?} epoch={} period={} duty={} revision={} phase={} retirement={}",
                adapter.session().state(),
                adapter.epoch(),
                owner.period_ms,
                owner.duty_permille,
                owner.active_revision,
                owner.phase_us,
                adapter.session().has_retirement(),
            ),
                "quit" => {
                    println!("done");
                    io::stdout().flush().unwrap();
                    break;
                }
                _ => println!("error unknown-command"),
            }
            println!("done");
            io::stdout().flush().unwrap();
        }
    });
}

fn drive(
    adapter: &mut Adapter,
    map: &VirtualMap<'_>,
    port: &mut Port,
    owner: &mut Owner,
    now_us: u64,
    drop_next_tx: &mut bool,
) {
    for _ in 0..16 {
        let event = if let Some(request) = port.pending.take() {
            println!("provider {request:?}");
            let completion = owner.complete(request);
            adapter.complete(completion, port)
        } else {
            adapter.service(map, now_us, port)
        };
        println!("event {event:?}");
        if event == AdapterEvent::ResponseQueued {
            let lease = adapter.take_tx().expect("queued response");
            if *drop_next_tx {
                println!("dropped {}", hex(lease.frame().as_slice()));
                *drop_next_tx = false;
            } else {
                assert!(adapter.lease_is_current(&lease));
                println!("tx {}", hex(lease.frame().as_slice()));
            }
            assert!(adapter.complete_tx(lease));
        }
        if matches!(
            event,
            AdapterEvent::Pending | AdapterEvent::Backpressured | AdapterEvent::Fenced(_)
        ) && port.pending.is_none()
        {
            break;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}
