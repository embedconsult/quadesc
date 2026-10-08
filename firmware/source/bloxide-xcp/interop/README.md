# Independent S0 XCP interoperability harness

This directory is a host-only interoperability fixture for the reviewed
`led-xcp/0.1.0` S0 subset. It uses upstream pyXCP and pya2ldb at immutable Git
commits, unmodified. It does not import the Rust/product endpoint and does not
claim full ASAM conformity.

The documented reference endpoint is intentionally independent. It implements
only the accepted observable S0 table over a pseudoterminal: CONNECT,
DISCONNECT, GET_STATUS, SYNCH, SET_MTA, UPLOAD, DOWNLOAD, checked virtual
addresses, and the selected error mapping. All unlisted commands, including
DAQ allocation, return `ERR_CMD_UNKNOWN` while connected.

The harness imports the A2L through pya2l into a typed operation model. Scalar
addresses, address extensions, widths/types, byte order, units, access, and
bounds come from that parsed model rather than profile constants. Before the
first DOWNLOAD, it reads and compares both 32-byte build/schema identities from
the frozen S0 metadata addresses in legal UPLOAD chunks. A mismatch disables
writes.

The selected serial implementation is the owned synchronous `S0SerialTransport`
through pyXCP's supported transport factory. Unmodified pyXCP builds requests,
checks SxI framing and parses the exact consumed PDU. [Ownership design and
limits](OWNERSHIP-DESIGN.md) explain why an acquisition-policy observer cannot
prove that identity with upstream asynchronous SxI queues. Locally observed
queued, unsolicited or duplicate traffic fences the session; timeout, close and
DISCONNECT require a new ClientSession. Request/response counters are independent.

## Reproducible environment

The lock is resolved for Python `>=3.10,<3.15`; the recorded execution uses
Python 3.12. Install and test without modifying the lock:

```sh
cd interop
uv sync --frozen --extra test
uv run --frozen --extra test pytest
```

`uv.lock` pins every transitive Python artifact. The two primary tools are
direct Git dependencies at:

- pyXCP 0.29.18: `016cf3e44364e9cd93966d144a39d342578a0391`
- pya2ldb 1.0.353 (`import pya2l`):
  `c19c3ad2f285d1230e334bad81eeb09cbaaa3031`

The CLI rejects a distribution whose installed version or `direct_url.json`
commit differs. No patches are applied.

## Run and evidence

```sh
uv run --frozen xcp-s0-interop reference --output artifacts/reference-run
```

The output path must not exist. Each invocation gets a fresh directory so an
earlier run's JSONL stream or manifest cannot be mixed with new evidence.

The run creates:

- `raw-uart.jsonl`: endpoint-observed complete raw SxI frames, including
  rejected/discarded corruption cases;
- `host-wire.jsonl`: host-observed reads and explicitly marked pre-write attempts;
- `transport-events.jsonl`: completed serial write counts (an attempted write may
  be refused by the ownership check);
- `decoded-events.jsonl` and `decoded-samples.jsonl`: decoded outcomes with
  addresses and units;
- `a2l-result.json`: pya2l import and descriptor comparison;
- `environment.json`: source pins, direct URLs, platform, interpreter and
  compiler identities;
- `result.json` and `SHA256SUMS`: machine-readable verdict and evidence hashes.

To run the safe subset against an already-authorized external serial endpoint:

```sh
uv run --frozen xcp-s0-interop endpoint \
  --connection-profile reviewed-19200-8n1 \
  --port /dev/ttyACM0 \
  --output artifacts/device-run
```

The external path connects, reads the two scalars, performs only a same-value
write, verifies DAQ rejection and SYNCH, and disconnects. Deliberate line
corruption and lost-response injection run only against the reference endpoint.
Opening a real adapter can affect modem-control lines; fixture review and the
serialized Jetson runner remain prerequisites for physical use.

The external profile is separate from the PTY profile and records the reviewed
current fixture choice, 19200 8N1. This is configuration support only. The
current 19200 core 0.1.2 firmware speaks a diagnostic protocol and is **not** an
XCP endpoint; do not run pyXCP against it. A future XCP target still requires
endpoint-identity and serial/modem/reset/BSL review before device open.

## T34 product endpoint qualification

T34 adds `tools/product_endpoint_pty.py`. It connects this unchanged pinned
client path to `fixtures/sxi-endpoint`, which runs the real Rust `xcp-sxi`
adapter and accepted transport-neutral Session. The bridge retains raw bytes,
endpoint events and request/response attribution. It also tests a suppressed
DOWNLOAD response, explicit transport loss, at least 50 ms observed quiet,
fresh client/transport creation and readback before any further mutation.

```sh
PYTHONPATH=interop/src python interop/tools/product_endpoint_pty.py \
  --endpoint target/debug/xcp-sxi-endpoint \
  --output /fresh/evidence/path
```

The endpoint provider is a deterministic host qualification fake. It is not the
T11 production T07/LED binding, and the PTY run is not native ARM64, target
runtime, electrical-baud, GPIO or optical evidence.

## Scope notes

`Applied` in these results means committed software state with bounded output
admission. It does not mean GPIO execution or optical observation. 38400 8N1 is
retained only as the historical PTY candidate; it is not a physical target
claim. CAN is represented explicitly as `not_run`; no UART/PTTY result is
relabeled as CAN evidence. S1 IF_DATA and selectable DAQ belong to T38.

The product bridge polls the real adapter every10ms select timeout and before
feeding an arriving chunk. It requires explicit observed-idle recovery. Run
`tools/verify_pty_deadline.py --endpoint <built-endpoint> --output <fresh-dir>`
for the actual30/80ms expired-suffix regression: zero backend admission, no
response and no in-band recovery, followed by explicit idle/fresh CONNECT.
These are x86_64 host scheduling tests, not native ARM64 or physical baud proof.
