#![no_std]

use xcp_core::{ProviderPort, ProviderRequest, SubmitError};

/// An unrelated example owner with private state. XCP can submit descriptor
/// operations, but cannot borrow or address this state directly.
pub struct ThermostatProvider {
    setpoint_deci_celsius: u16,
    pending: Option<ProviderRequest>,
}

impl ThermostatProvider {
    pub const SETPOINT_DESCRIPTOR: u32 = 0x7001;

    pub const fn new() -> Self {
        Self {
            setpoint_deci_celsius: 215,
            pending: None,
        }
    }

    pub const fn setpoint_deci_celsius(&self) -> u16 {
        self.setpoint_deci_celsius
    }

    pub fn take_request(&mut self) -> Option<ProviderRequest> {
        self.pending.take()
    }

    /// This owner-mediated function is the only mutation path in the fixture.
    pub fn apply_if_valid(&mut self, request: &xcp_core::ApplyRequest) -> bool {
        if request.descriptor_id != Self::SETPOINT_DESCRIPTOR || request.length != 2 {
            return false;
        }
        let candidate = u16::from_le_bytes([request.encoded_value[0], request.encoded_value[1]]);
        if !(150..=300).contains(&candidate) {
            return false;
        }
        self.setpoint_deci_celsius = candidate;
        true
    }
}

impl Default for ThermostatProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderPort for ThermostatProvider {
    fn try_submit(&mut self, request: ProviderRequest) -> Result<(), SubmitError> {
        if self.pending.is_some() {
            Err(SubmitError::Busy)
        } else {
            self.pending = Some(request);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcp_core::{
        command, Dispatch, Packet, ProviderCompletion, Region, Session, SynchronizeResult,
        VirtualMap, WriteResult,
    };

    static REGIONS: [Region; 1] = [Region::calibration(
        ThermostatProvider::SETPOINT_DESCRIPTOR,
        0x3000,
        2,
    )];

    fn packet(bytes: &[u8]) -> Packet {
        Packet::try_from_slice(bytes).unwrap()
    }

    #[test]
    fn independent_owner_consumes_the_declared_core_api() {
        let map = VirtualMap::new(&REGIONS).unwrap();
        let mut provider = ThermostatProvider::new();
        let mut session = Session::new();

        assert_eq!(
            session.handle_packet(&map, &packet(&[command::CONNECT, 0]), 0, &mut provider),
            Dispatch::Deferred
        );
        let generation = match provider.take_request().unwrap() {
            ProviderRequest::Synchronize { session_generation } => session_generation,
            other => panic!("unexpected request: {other:?}"),
        };
        assert!(matches!(
            session.complete(
                ProviderCompletion::Synchronized {
                    session_generation: generation,
                    result: SynchronizeResult::Ready { service_epoch: 1 }
                },
                &mut provider
            ),
            Dispatch::Respond(_)
        ));

        assert!(matches!(
            session.handle_packet(
                &map,
                &packet(&[command::SET_MTA, 0, 0, 0, 0, 0x30, 0, 0]),
                0,
                &mut provider
            ),
            Dispatch::Respond(_)
        ));
        assert_eq!(
            session.handle_packet(
                &map,
                &packet(&[command::DOWNLOAD, 2, 0xFA, 0]),
                10,
                &mut provider
            ),
            Dispatch::Deferred
        );
        let apply = match provider.take_request().unwrap() {
            ProviderRequest::Apply(request) => request,
            other => panic!("unexpected request: {other:?}"),
        };
        assert!(provider.apply_if_valid(&apply));
        assert_eq!(provider.setpoint_deci_celsius(), 250);
        assert!(matches!(
            session.complete(
                ProviderCompletion::Write {
                    correlation: apply.operation.correlation(),
                    result: WriteResult::Applied {
                        changed: true,
                        active_revision: 1
                    }
                },
                &mut provider
            ),
            Dispatch::Respond(_)
        ));
        assert!(matches!(
            provider.take_request(),
            Some(ProviderRequest::ReleaseOutcome { .. })
        ));
    }
}
