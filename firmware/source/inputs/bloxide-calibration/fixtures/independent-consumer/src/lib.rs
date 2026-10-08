#![no_std]

use bloxide_calibration::{BoundedI32, Celsius};

pub type IndependentTemperature = BoundedI32<Celsius, -4_000, 12_500>;

pub fn checked_temperature(value: i32) -> Option<i32> {
    IndependentTemperature::try_new(value)
        .ok()
        .map(IndependentTemperature::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_public_validation_without_drone_or_xcp_dependencies() {
        assert_eq!(checked_temperature(2500), Some(2500));
        assert_eq!(checked_temperature(20_000), None);
    }
}
