# Embedded system generation

System generation supports a hosted profile (the default) and an explicit
`profile = "embedded"` with runtime `embassy` and target
`thumbv8m.main-none-eabi`. Embedded applications use one thread-mode executor,
`#![no_std]`, `#![no_main]`, a Cortex-M Rust entrypoint, and explicit runtime
features. They never request hosted executor/time/critical-section features or
emit logging/println. Platform code supplies the allocator, panic/fault policy,
critical-section implementation, linker memory map and time driver.

The `[platform]` table names `impl_crate`, `entry = "cortex-m-rt"`, `init`,
`on_error`, `freeze`, and `resources` (the type returned by successful init).
`cleanup = "process-lifetime"` declares that application resources and runtime
storage must remain live until MCU reset; actor reset never reacquires them.
Init returns Result<Resources, E>; on_error accepts E and diverges. Service
startup functions return Result<(), E>. The freeze hook is synchronous and
infallible. It arms a guard for alloc/alloc_zeroed/realloc/dealloc; it never
unfreezes on Reset, Stop, fault, reconnect or completion. Implementations must
qualify every reachable post-freeze path; generation/linking alone cannot prove
this. Resources are exhaustively destructured; the platform must dispose of
any unused initialization resources before returning its Resources value.

`[[services]]` declares name, impl_crate, start and resource. `message_path` and
`capacity` are an optional pair: both present creates one bounded typed channel;
both absent creates no channel, ActorRef or receiver. Exactly one present is an
error. Start receives (spawner, resource, [receiver], injected outputs in sorted
field order). Services are concrete async I/O owners, not actors with lifecycle
semantics. Their startup function may submit tasks but cannot admit external
application work before setup finishes and the freeze barrier is armed.

An actor constructor uses `source = "resource", field = "name"` to move one
initialized field. Service resource names and all resource injections share a
single ownership ledger; a second move is rejected during generation and Rust
ownership checks the concrete types. `source = "service", service = "name"`
injects only a mailbox-bearing service ref. It is invalid for mailbox-free
services. Actor output refs support named mailbox selection. Portable producer
handles and service-owned consumer/hardware resources are separate fields.

Startup order is init; channels/supervision; contexts/machines; service/task
submission; freeze; bootstrap
admission; first executor poll (and actor Start). Init/service errors diverge
through on_error before admission. Bootstrap send failures panic explicitly
under the platform fatal policy. The embedded profile rejects dynamic actors
and the allocating generic timer service pending a qualified bounded adapter.
Platform services may use a fixed-capacity time driver directly.

The generic Cortex-M33 fixture uses Rust 1.94.0, software float, 512 KiB flash
and 96 KiB RAM. Its platform code proves startup/time/critical-section linkage.
It does not establish AM13 registers, clock rates, pin assignments or hardware
behavior. Existing observed diagnostic bringup and proposed Embassy integration
remain separate evidence.

## Explicit packages and reproducibility

A repository declares `blox-package.toml` with format=1, package name/version and
blox-schema=1. `[[crates]]` and `[[tools]]` export Cargo name/path pairs;
`[[bloxes]]` exports name/path/crate triples. Paths cannot escape the package.
Pure TOML exports contain no Cargo.toml; their generated name must match the
actor name. Consumers select all already-fetched packages explicitly in
blox-workspace.toml. A selection contains package plus path, or package plus
git/rev/checkout (full 40-character commit and exact origin URL). Overrides
contain path and apply to an existing alias. Refer to an imported actor as
`blox = "alias::export"`; generated Cargo names remain unambiguous.

`cargo blox resolve` explicitly records blox.lock. Git inputs must be clean,
with matching URL/commit/tree; local overrides carry content SHA-256 and dirty
status, not a Git provenance claim. Duplicate package/Cargo names, missing
selected dependencies, conflicting declared revisions and cycles are errors.
No command fetches a declarative package or scans arbitrary sibling roots.
Ordinary Cargo resolution still handles Rust dependencies and features. Git
exports retain Git/rev dependency tables; local overrides patch all exported
crates for the declared Git source. Build scripts consume imported TOML by
relative source path and use the selected codegen dependency.

Generation verifies an existing composition lock even without `--locked`.
`cargo blox lock` explicitly updates and saves the generated Cargo.lock as
locks/blox-generated.Cargo.lock. `generate --locked` requires that tracked lock
for composed applications. Root and generated Cargo locks are separate. Scope
embedded builds with `cargo blox build --example NAME -- --locked --release`;
the CLI selects the system's target and runs Cargo from the generated workspace
so its Cortex-M linker configuration applies. Plain Cargo users pass the target
explicitly from that directory. Run commands reject embedded targets; device
operations belong to a reviewed platform runner.

The repeatable external fixture is `scripts/test-embedded-consumer.py`. It
creates independent feature/consumer Git inputs under target, imports the
framework via its declared exports, generates both stages through cargo-blox,
checks concrete action effects on the host, and links the embedded ELF with
committed Cargo lock inputs. `--update-locks` is an explicit maintenance step.
Deleting the generated tree must leave the source and lock inputs sufficient
for locked regeneration. Fixture sources are templates under the CLI tests,
never committed generated Rust.

Imported blox package versions and license metadata come from their source
package, not the consumer. Generator revision and SHA-256 are captured when
the tool is compiled; build-script resynchronization verifies that identity
and package contents against blox.lock. A stale tool cannot claim newer source
bytes as its own build identity. Generated file hashes and source hashes are
recorded in generation-manifest.json.

## Package and lock validation boundaries

Source inventory does not consult Git ignore rules. Path snapshots include
ignored files. Git packages require every inventory file to be a regular file
in the pinned commit with identical bytes and executable mode; ignored or
untracked source cannot acquire clean Git provenance. Symlinks (including
symlinked export path components), submodules and nested repositories are
unsupported. The inventory excludes only root Git administrative data, root
`target/`, Cargo package `target/` directories, `.vscode/settings.json`, root
`blox.lock`, `locks/blox-generated.Cargo.lock`, and `src/generated/` belonging
to a Cargo crate with a sibling `blox.toml`. These are reserved output locations:
exports cannot point into them. Ordinary files named `target`, `blox.lock` or
`generated` elsewhere remain source. Tracked source in reserved directories is
rejected, except the two tracked composition lock outputs. Other source files
are hashed regardless of extension. Ignored source makes a path snapshot dirty.

Literal Rust `include!`, `include_str!`, `include_bytes!` and `#[path]` inputs
are checked for package/output-directory escapes. Computed macro paths remain
subject to the dynamic-input boundary below.

This inventory captures repository files, not arbitrary execution of Rust
build scripts/procedural macros, environment variables, network reads or
computed external include paths. Such dynamic inputs require separate release
capture/audit; package locking alone does not certify them. Explicit exported
paths, Cargo target/build-script paths, and selected Cargo path dependencies
must remain within selected source roots. A package may not use output directories as handwritten source inputs.

The dependency access ledger includes inferred dependencies and all explicit
normal/dev dependencies. Workspace dependency renames are checked using the
Cargo `package` identity, not the local alias. Declared cross-bundle access and
cycles use the same ledger; ordinary third-party Cargo dependencies stay usable.

Every CLI compilation graph is validated before any compilation starts: root,
generated and applicable standalone impl workspaces. Cargo selects the active
package/features/target/dependency kinds; inactive target alternatives must not
be combined into a false duplicate. Metadata identifies the sources in that
selected graph. Lock/offline/frozen options apply to preflight and compilation.

`generate --locked` (and `--frozen`) checks all applicable saved Cargo locks
using Cargo offline, without dependency discovery on the network or updates to
source/lock files. Missing, malformed and stale root, generated or standalone
locks fail at generation. Generated directories remain disposable, including
on failure. Source Rust is never rewritten by generation. `cargo blox lock`
is the explicit update operation for root, generated and standalone locks;
every invoked standalone workspace lock is tracked next to its manifest.

Preflight and compilation use the same manifest directory as their working
directory, so Cargo configuration discovery agrees. Named Cargo test/example/
bench targets (including `--test=NAME`) include dev-dependency identities.
Arguments after the binary argument separator do not select Cargo graph edges.
Impl crates that are ordinary workspace members use that workspace's lock;
only truly standalone workspaces need their own lock. Cargo's locate-project
command determines lock ownership; directory placement is not the authority.
