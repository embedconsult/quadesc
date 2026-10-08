use xcp_sxi::{DiscardReason, ParseEvent, Parser};
#[test]
fn explicit_prearrival_poll_enforces_all_literal_connect_split_deadlines() {
    let bytes = [2, 0xaa, 0xff, 0, 0xab];
    for split in 1..bytes.len() {
        for gap in [19_999, 20_000, 20_001] {
            let mut p = Parser::new();
            for &b in &bytes[..split] {
                assert_eq!(p.feed_byte(100, b), ParseEvent::Pending);
            }
            let event = p.poll(100 + gap);
            if gap < 20_000 {
                assert_eq!(event, ParseEvent::Pending);
                let mut last = ParseEvent::Pending;
                for &b in &bytes[split..] {
                    last = p.feed_byte(100 + gap, b);
                }
                assert!(matches!(last, ParseEvent::Frame(_)));
            } else {
                assert_eq!(event, ParseEvent::Discarded(DiscardReason::Truncated));
                // An arriving suffix is not an observed-idle boundary.
                for &b in &bytes[split..] {
                    assert_eq!(p.feed_byte(100 + gap, b), ParseEvent::Pending);
                }
                assert_eq!(p.observe_idle(100 + gap + 19_999), ParseEvent::Pending);
                assert_eq!(p.observe_idle(100 + gap + 20_000), ParseEvent::Recovered);
            }
        }
    }
}
#[test]
fn clock_overflow_poll_and_feed_discard_then_explicit_recovery() {
    for use_poll in [false, true] {
        let mut p = Parser::new();
        p.feed_byte(u64::MAX - 1, 2);
        let e = if use_poll {
            p.poll(0)
        } else {
            p.feed_byte(0, 0)
        };
        assert_eq!(e, ParseEvent::Discarded(DiscardReason::ClockRegression));
        assert_eq!(p.observe_idle(19_999), ParseEvent::Pending);
        assert_eq!(p.observe_idle(20_000), ParseEvent::Recovered);
    }
}
#[test]
fn every_coalescing_split_preserves_three_distinct_counters() {
    let bytes = [1, 0xfe, 0xfd, 0xfc, 1, 0, 0xfd, 0xfe, 1, 0, 0xfd, 0xfe];
    for split in 0..=bytes.len() {
        let mut p = Parser::new();
        let mut frames = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            if let ParseEvent::Frame(f) = p.feed_byte(if i < split { 100 } else { 200 }, b) {
                frames.push(f);
            }
        }
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].counter, 0xfe);
        assert_eq!(frames[1].counter, 0);
        assert_eq!(frames[2].counter, 0);
        assert!(matches!(
            frames[1].counter_observation,
            xcp_sxi::CounterObservation::Gap {
                expected: 0xff,
                observed: 0
            }
        ));
        assert!(matches!(
            frames[2].counter_observation,
            xcp_sxi::CounterObservation::Repeat { observed: 0 }
        ));
    }
}
