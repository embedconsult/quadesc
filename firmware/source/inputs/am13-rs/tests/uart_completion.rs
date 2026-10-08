#[path = "support/uart_model.rs"]
mod model;
use am13_rs::uart::Error;
use model::Model;

#[test]
fn untouched_and_full_fifo_rejections_do_not_create_outstanding_tx() {
    let m = Model::default();
    let mut u = m.uart();
    m.rx_busy.set(true);
    m.ris.set(0x1002); // old EOT plus receive error
    m.full.set(true);
    let clears = m.clears.get();
    for _ in 0..100 {
        assert!(u.is_tx_complete());
        assert!(!u.try_write(0x55));
    }
    assert_eq!(m.writes.get(), 0);
    assert_eq!(m.clears.get(), clears);
    assert_eq!(m.ris.get(), 0x1002);
    m.full.set(false);
    assert!(u.try_write(0x55));
    assert_eq!(m.ris.get(), 2); // only EOT was cleared
    assert!(!u.is_tx_complete());
}

#[test]
fn data_and_final_stop_bit_must_leave_before_complete_even_with_empty_fifo() {
    let m = Model::default();
    let mut u = m.uart();
    m.rx_busy.set(true);
    for _batch in 0..8 {
        m.ris.set(m.ris.get() | 0x1000); // previous batch's stale completion
        let accesses = m.accesses.get();
        assert!(u.try_write(0xa5));
        assert!(m.accesses.get() - accesses <= 4);
        m.full.set(true);
        assert!(!u.try_write(0x99));
        assert!(!u.is_tx_complete());
        m.full.set(false);
        for phase in 1..=3 {
            assert_eq!(m.phase.get(), phase);
            for _poll in 0..10 {
                assert!(!u.is_tx_complete());
                assert!(!u.try_write(0x99)); // FIFO space alone cannot admit
            }
            m.advance();
        }
        for _poll in 0..10 {
            assert!(u.is_tx_complete());
        }
        assert!(m.rx_busy.get());
    }
    assert_eq!(m.writes.get(), 8);
    assert_eq!(m.clears.get(), 8);
}

#[test]
fn eot_and_receive_errors_are_independent_in_both_orders() {
    let m = Model::default();
    let mut u = m.uart();
    m.rx_busy.set(true);
    for first_read in [false, true] {
        assert!(u.try_write(0xa5));
        m.ris.set(0x2001e); // all receive errors, no EOT
        assert_eq!(u.try_read(), Err(Error::Receive));
        assert!(!u.is_tx_complete());
        m.finish();
        m.ris.set(m.ris.get() | 0x2001e);
        if first_read {
            assert_eq!(u.try_read(), Err(Error::Receive));
        }
        assert!(u.is_tx_complete());
        if !first_read {
            assert_eq!(u.try_read(), Err(Error::Receive));
        }
        assert_eq!(m.ris.get(), 0x1000);
        assert_eq!(u.try_read(), Ok(None));
    }
    assert_eq!(u.rx_errors(), 4);
}

#[test]
fn completion_at_each_admission_mmio_boundary_cannot_complete_the_next_byte() {
    // Complete old TX before status read, before raw status read, or after a
    // rejected admission. Every interleaving must protect the next byte.
    for boundary in 1..=3 {
        let m = Model::default();
        let mut u = m.uart();
        assert!(u.try_write(1));
        m.finish_on_access.set(m.accesses.get() + boundary);
        let admitted = u.try_write(2);
        if !admitted {
            m.finish();
            assert!(u.try_write(2));
        }
        assert_eq!(m.writes.get(), 2);
        assert!(!u.is_tx_complete());
        m.finish();
        assert!(u.is_tx_complete());
    }
}

#[test]
fn completion_during_preemption_after_txdata_is_retained_and_full_rejection_preserves_it() {
    let m = Model::default();
    let mut u = m.uart();
    m.complete_in_write.set(true); // emulate arbitrary preemption after TXDATA
    assert!(u.try_write(1));
    assert_eq!(m.ris.get(), 0x1000);
    m.full.set(true);
    assert!(!u.try_write(2));
    assert_eq!(m.ris.get(), 0x1000);
    assert!(u.is_tx_complete());
    m.full.set(false);
    assert!(u.try_write(2));
    assert!(u.is_tx_complete());
    assert_eq!(m.writes.get(), 2);
}
