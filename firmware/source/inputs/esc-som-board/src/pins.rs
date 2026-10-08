//! Wiring extracted from corrected source 802aacd, current SysConfig v3 authority.
//! Package/mux evidence is recorded separately; chip Pin contains only port/bit.
use am13_rs::gpio::{
    Pin,
    Port::{A, B, C},
};
pub const BSL_UART_TX: Pin = Pin::new(A, 0);
pub const BSL_UART_RX: Pin = Pin::new(A, 1);
pub const BSL_CAN_TX: Pin = Pin::new(A, 12);
pub const BSL_CAN_RX: Pin = Pin::new(A, 11);
pub const BSL_INVOKE: Pin = Pin::new(A, 6);
pub const DRV_ENABLE: Pin = Pin::new(B, 10);
pub const INL_ENABLE: Pin = Pin::new(B, 11);
pub const STATUS_LED: Pin = Pin::new(B, 18);
pub const CAN_TERM: Pin = Pin::new(B, 9);
pub const I2C_SDA: Pin = Pin::new(A, 22);
pub const I2C_SCL: Pin = Pin::new(A, 23);
pub const SPI_MOSI: Pin = Pin::new(C, 6);
pub const SPI_MISO: Pin = Pin::new(C, 4);
pub const SPI_SCLK: Pin = Pin::new(C, 2);
pub const NFAULT: [Pin; 4] = [
    Pin::new(A, 30),
    Pin::new(B, 15),
    Pin::new(B, 17),
    Pin::new(B, 0),
];
pub const DRIVER_CS: [Pin; 4] = [
    Pin::new(C, 13),
    Pin::new(B, 14),
    Pin::new(B, 16),
    Pin::new(C, 5),
];

/// All motor PWM pads from v3, including the extra Motor1 3B output.
/// LED-cal holds these as GPIO-low; it never selects the PWM mux.
pub const MOTOR_OUTPUTS: [Pin; 13] = [
    Pin::new(B, 6),
    Pin::new(B, 7),
    Pin::new(B, 8),
    Pin::new(B, 12),
    Pin::new(C, 25),
    Pin::new(C, 26),
    Pin::new(C, 18),
    Pin::new(B, 13),
    Pin::new(A, 10),
    Pin::new(A, 25),
    Pin::new(C, 1),
    Pin::new(C, 3),
    Pin::new(A, 24),
];
/// (pad, mux, MCPWM module, output index 0..5 for 1A,1B,2A,2B,3A,3B).
/// v3 contains only these 13 motor outputs; MCPWM4 DSHOT/buzzer routes are excluded.
pub const PWM_ROUTES: [(Pin, u8, u8, u8); 13] = [
    (Pin::new(B, 13), 5, 0, 1),
    (Pin::new(A, 10), 7, 0, 4),
    (Pin::new(A, 25), 13, 0, 5),
    (Pin::new(B, 6), 5, 1, 0),
    (Pin::new(B, 7), 5, 1, 2),
    (Pin::new(B, 8), 5, 1, 4),
    (Pin::new(B, 12), 5, 1, 5),
    (Pin::new(C, 1), 7, 2, 0),
    (Pin::new(C, 3), 7, 2, 2),
    (Pin::new(A, 24), 3, 2, 4),
    (Pin::new(C, 25), 4, 3, 0),
    (Pin::new(C, 26), 4, 3, 2),
    (Pin::new(C, 18), 4, 3, 4),
];
pub const DSHOT_INPUTS: [Pin; 4] = [
    Pin::new(A, 27),
    Pin::new(A, 4),
    Pin::new(B, 28),
    Pin::new(A, 26),
];
