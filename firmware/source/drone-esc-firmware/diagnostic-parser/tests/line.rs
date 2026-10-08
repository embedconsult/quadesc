use am13_diagnostic_parser::{Command, Line, parse};
use led_messages::{DUTY_ID, PERIOD_ID};
fn command(bytes: &[u8]) -> Command {
    let mut line = Line::new();
    for b in bytes {
        line.push(*b)
    }
    parse(&line)
}
#[test]
fn target_parser_maps_bounded_commands_and_rejects_invalid_input() {
    assert_eq!(command(b"GET\n"), Command::Read);
    assert_eq!(command(b"SAVE\n"), Command::Save);
    assert_eq!(command(b"STATUS\r\n"), Command::Status);
    assert_eq!(command(b"FLASH\n"), Command::Flash);
    assert_eq!(command(b"SET PERIOD 100\n"), Command::Apply(PERIOD_ID, 100));
    assert_eq!(
        command(b"SET PERIOD 10000\n"),
        Command::Apply(PERIOD_ID, 10000)
    );
    assert_eq!(command(b"SET DUTY 0\n"), Command::Apply(DUTY_ID, 0));
    assert_eq!(command(b"SET DUTY 1000\n"), Command::Apply(DUTY_ID, 1000));
    for bytes in [
        b"SET PERIOD 99\n".as_slice(),
        b"SET PERIOD 10001\n",
        b"SET DUTY 1001\n",
        b"SET DUTY -1\n",
        b"SET DUTY 1x\n",
        b"GET",
        b"UNKNOWN\n",
    ] {
        assert_eq!(command(bytes), Command::Invalid);
    }
    assert_eq!(command(&[b'X'; 65]), Command::Invalid);
    let mut l = Line::new();
    for b in [b'X'; 65] {
        l.push(b)
    }
    l.push(b'\n');
    assert_eq!(parse(&l), Command::Invalid);
    l.clear();
    for b in b"GET\n" {
        l.push(*b)
    }
    assert_eq!(parse(&l), Command::Read);
}
