#![no_std]
//! Bounded UC4 operations used by the diagnostic service.
use am13_rs::{
    io::RegisterIo,
    uart::{self, Uart},
};
use bloxide_calibration::OperationKey;
use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::{Read, Write};

/// 320 response bytes at 19,200 8N1 need at most 167 ms on the wire;
/// 400 ms includes over twice that serialization time plus executor margin.
pub const TX_BUDGET_MS: u64 = 400;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxFault {
    Timeout,
    Driver(uart::Error),
}

/// Whole-response bound, including the final stop bit. Cancellation leaves
/// ownership with the caller, which must end the session: bytes may be out.
pub async fn write_bounded<I: RegisterIo>(uart: &mut Uart<I>, bytes: &[u8]) -> Result<(), TxFault> {
    match with_timeout(Duration::from_millis(TX_BUDGET_MS), async {
        uart.write_all(bytes).await?;
        uart.flush().await
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(TxFault::Driver(error)),
        Err(_) => Err(TxFault::Timeout),
    }
}

/// The caller keeps the owner result retained until this returns Ok, then
/// confirms Release before accepting another mutation. A wire fault latches
/// the exact admitted key before the caller ends its session.
pub async fn write_admitted_response<I: RegisterIo>(
    uart: &mut Uart<I>,
    bytes: &[u8],
    key: OperationKey,
    record_uncertain: fn(OperationKey),
) -> Result<(), TxFault> {
    let result = write_bounded(uart, bytes).await;
    if result.is_err() {
        record_uncertain(key);
    }
    result
}

/// One read attempt. Even a synchronous register error or empty read yields
/// one millisecond before the service can poll UC4 again.
pub async fn read_yielding<I: RegisterIo>(uart: &mut Uart<I>) -> Result<Option<u8>, uart::Error> {
    let mut byte = [0];
    match uart.read(&mut byte).await {
        Ok(1) => Ok(Some(byte[0])),
        Ok(_) => {
            Timer::after(Duration::from_millis(1)).await;
            Ok(None)
        }
        Err(error) => {
            Timer::after(Duration::from_millis(1)).await;
            Err(error)
        }
    }
}
