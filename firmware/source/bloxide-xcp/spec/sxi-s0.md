# SxI scalar S0 transport design

Status: T34 local implementation design, subordinate to immutable
`led-xcp/0.1.0`. It does not revise the accepted profile.

## Wire format

Each frame is exactly `LEN:u8 | CTR:u8 | PDU[LEN] | SUM:u8`, where `LEN` is
1..8 and `SUM` is the low eight bits of `LEN + CTR + every PDU byte`. There is
no padding, delimiter, sync byte, byte-order option, escaping, or in-band scan
for a plausible header. The maximum frame occupies 11 bytes.

Each direction owns its own transmit counter, initially zero and incremented
modulo 256 only when an encoded frame is produced. A receiver records the first
counter and diagnoses subsequent repeats or gaps relative to the expected
modulo-256 value. It still admits an otherwise valid frame. CTR is never echoed,
never compared across directions, and never used as an XCP request identity.

SUM8 catches only some corruptions. A checksum pass is not proof of an
uncorrupted physical transfer and this profile is not a motor safety mechanism.

## Parser and time contract

The parser owns one `[u8; 11]`, a byte count, an optional expected frame length,
the last observed byte timestamp, and counter diagnostics. It has two modes:

- `Accepting`: a fresh LEN begins a frame; one completed valid frame is returned
  directly to the caller before any later coalesced byte is consumed.
- `DiscardUntilIdle`: all bytes are dropped and update the last-byte timestamp.
  Only an explicit idle observation can return to `Accepting`.

LEN0/LEN9, checksum failure, UART framing/overrun/parity indication, incomplete
frame expiry, timestamp regression, and caller overflow enter
`DiscardUntilIdle`. None yields a PDU. A checksum failure consumes its declared
frame then still requires idle; bytes following it in the same batch are not
scanned as headers.

The injected time domain is monotonic microseconds. `observe_idle(now)` proves
idle only when `now >= last_byte` and `now - last_byte >= 20_000`; equality is
expired. `now < last_byte` is clock regression/overflow and remains discarded
until a later representable observed-idle interval. `poll(now)` on an incomplete
frame uses the same comparison and enters discard at exactly 20,000 us. `feed_byte` checks that same expiry BEFORE replacing the arrival timestamp,
so a late suffix is discarded even without a preceding poll. The arriving byte
then starts the next observed-idle interval. Arrival never implicitly recovers
or establishes a fresh header; only explicit `observe_idle` can do that.
The adapter's `observe_idle(now, provider)` durably fences Session and TX on
clock regression, including wrapping from u64::MAX to zero.

## Bounded adapter

The adapter owns the parser, one complete RX PDU slot, one complete encoded TX
frame slot, a sender counter, and diagnostics. It wraps an existing T08 Session,
VirtualMap and ProviderPort by reference/ownership chosen by the composition;
it does not replicate XCP semantics.

One public service step performs bounded work. Control (`Session::service` and
provider completion) receives priority over accepting another RX PDU. At most
one PDU is admitted to Session and at most one response is encoded per step.
When RX is full, additional complete input is rejected into discard-until-idle;
there is no unbounded byte or packet queue. When TX is full, no next command is
admitted. A response counter advances only when the response frame is admitted
to the owned TX slot.

TX ownership states are `Empty`, `Ready(frame)` and `InFlight(send)`.
`with_adapter(|adapter| ...)` initializes one owned adapter with a fresh invariant
lifetime brand. Move it into the service/context inside this scope at startup;
the brand and leases cannot escape or cross into another adapter scope. There is
no public new/default/clone/copy, no global registry, pointer identity or instance
integer. Instance collision/wrap/exhaustion is structurally unrepresentable.
Logical Stop/Reset uses `lifecycle_fence` on the same adapter and Session; it must
never reconstruct resources or discard outstanding owner truth.

`take_tx` moves out an opaque, non-Clone/non-Copy `TxLease` with the owning brand
and a private nonwrapping u64 send sequence. `frame()` borrows immutable bytes;
the platform checks `lease_is_current` immediately before each bounded emission
and consumes the token with `complete_tx` when finished. Fences invalidate an
in-flight token, which can still be returned but cannot complete a later send.
A consumed token cannot be used for delayed duplicate completion. Copying bytes
is not copying send authority. Driver cancellation/draining of already admitted
hardware bytes remains a platform obligation before reconnect.

Send sequence is reserved when a response enters TX. Its exhaustion and transport
epoch exhaustion are terminal for RX/TX admission; neither wraps nor resets on
logical lifecycle. Session control/completions still reconcile retained work.
Wire CTR remains an independent modulo-256 diagnostic counter.

Every service call first drives one bounded Session control/deadline/retry step,
even with queued RX, taken TX or a partial frame. Session returns `ServiceResult`:
`dispatch` carries durable delivery state and `new_fault` identifies Unavailable
observed during this particular release/ResolveOrCancel attempt. A fresh fault
invalidates the partial/RX/TX boundary and epoch once even when an older fence
was already handled. Busy, successful retry, confirmed admission and no-work
reports do not constitute fresh faults. Repeated unchanged durable reports must
preserve the recovered boundary. A newly admitted CONNECT re-arms durable-fence
tracking. No transport_lost call is repeated from this service path, so a service
fault does not trigger another owner attempt. At most one RX PDU follows control.

The provenance is a local return value, not a stored flag, counter or sequence.
Every actual Unavailable attempt is a fresh fault; no identity can wrap or alias.
Logical Reset preserves all existing ownership and checked identity state. After
a new release fault at20402us, partial CONNECT0200ff from20401us is discarded;
matching Released then suffix0001 cannot synchronize without another explicit
idle boundary. Busy at20402us preserves the partial CONNECT and progress.

The fence never clears the T08 provider effect, pending write, retained outcome,
operation key, or retirement. Byte recovery proves no mutation result. The
adapter does not manufacture Busy, rejection, Ready, Released or readback.

## Host endpoint

The T34 endpoint is a host executable using the actual public Session and this
adapter with a fixed-capacity deterministic provider. It receives raw bytes from
a PTY, supplies monotonic timestamps, polls on 10ms select timeouts and immediately
before each arriving chunk at its arrival timestamp, and emits only a current
send. Idle recovery is an explicit harness operation after observed quiet;
periodic polling never automatically calls observe_idle. The OS scheduling
interval is not a hard real-time guarantee; pre-arrival expiry is authoritative. It records raw reads/writes,
parser dispositions, Session admission, provider commands/completions and the
specific command/response association consumed by the pinned client.

The endpoint supports forced malformed input and response-backpressure schedules
for tests. Recovery stops requests, closes/discards the host client and parser,
observes at least 50 ms of quiet (therefore crossing the target's 20 ms boundary),
creates a fresh client transport, CONNECTs, and performs fresh readback. It never
blindly retries a timed-out DOWNLOAD.

This endpoint's provider is explicitly a qualification fake. Production T07/LED
ownership and platform UART binding remain T11/T35 work.

## Versioned serial profiles

- `reviewed-19200-8n1`: selected external diagnostic baseline, 19200 baud, 8
  data bits, no parity, one stop bit, full duplex, LEN8/CTR8/SUM8, no escaping.
- `reference-pty-38400-8n1`: historical candidate retained for T10 fixture
  reproducibility only.

A PTY does not measure or qualify electrical baud, modem control, reset/BSL
interaction, native ARM64, target timer accuracy, or physical reliability.

## Correction test equivalence

The original normative assertions are identified by source revision `0e55ab84`.
API adaptation does not treat compilation failure alone as proof of runtime
fencing; the retained tests assert the runtime behavior directly.

| Historical normative assertion | Corrected equivalent |
|---|---|
| F1 arrival_at_deadline | Same runtime assertion in review_adversarial; no pre-poll required |
| F1 all_splits_exact_arrival_deadlines | Same 19999/20000/20001 schedules in review_extended; additional adapter zero-provider-admission test |
| F1 actual30/80ms witnesses | verify_pty_deadline.py asserts zero response/backend admission and no in-band recovery; fresh CONNECT only after explicit idle |
| F2 taken response idle regression | Same genuine token/current check; observe_idle now takes the real provider |
| F2 queued response idle regression | Same no-take assertion |
| F2 pending write/u64 wrap | Same full-key ResolveOrCancel; added Applied/outcome/actual Released and reset retention |
| F3 100 RX opportunities | Same release retry schedule; no fabricated Released |
| F3 100 held TX opportunities | Same release retry schedule with a live genuine token |
| F3 partial frame owner deadline10300us | Same ResolveOrCancel assertion at the exact owner deadline |
| F3 original worker run2 partial boundary | Delivered truncation test plus repeated durable fence/service schedule retaining all3 bytes until exact expiry |
| F4 completed prior send becomes current | Consumed-token use is compiler-rejected; runtime fenced old token remains false across a fresh send |
| F4 cross-instance lease | Nested brand current-check/completion and prior-scope escape are compiler-rejected, even with send1 in both instances |
| F4 delayed duplicate completion clears current | Compiler rejects second completion of consumed token; real unconsumed old fenced completion returns false and preserves the newer genuine token |
| F4 Adapter Copy | Copy and Clone compiler rejection; runtime ownership move preserves genuine token semantics |
| F4 forged public lease | Private-field construction compiler rejection |
| F4 identity exhaustion | Runtime final-send/terminal send exhaustion and epoch exhaustion, including retained Applied/release; instance integer exhaustion cannot occur because there is no finite instance namespace, with cross-scope/escape/reconstruction compiler tests |

Rust compiler tests are executable rustdoc examples in src/ownership.md. The
library runtime tests preserve all17 delivered tests and add the unchanged
positive arithmetic/coalescing/overflow review schedules. The accepted Session
lifecycle fixture is generated from TOML with pinned codegen, never hand-edited.

## Remaining integration obligation I1

The generative scope is unchanged. A concrete initialized-once selected-executor
binding must place the owned service/future within that scope and prove its
post-freeze lifetime and Reset behavior. A scoped host positive does not prove a
static runtime binding. T11/T36 retain this unresolved obligation.
