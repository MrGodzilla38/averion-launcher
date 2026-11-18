# Application architecture and future feature design

**Historical Phase 02 design record.** Baseline descriptions and package counts below apply to Phase 02. Phase 03 implemented page routing, offline sample presentation and keyboard traversal. See the [current architecture](ARCHITECTURE.md) and [implemented UI](UI-DESIGN.md). Backend, persistence and job designs remain proposals.

## Baseline and constraints

One Rust 2024 binary uses Cargo and Iced 0.14.0 with tiny-skia on x86_64 Linux and Windows. Boot constructs a default Launcher containing one UI-independent State. The subscription uses ui::input::shortcut and the Home button emits the same Message::ToggleDetails. Launcher delegates the toggle to State and passes the current visibility to Home. Seven headless tests cover input, state transitions and the application message path. CI is configured for both platforms. See the [Phase 02 verification record](PHASE-02-PLAN.md#verification-record) for results by revision. The startup shell provides a working test case for separating state transitions from presentation.

The only direct crate is exact-version iced with defaults disabled and tiny-skia, thread-pool, x11 and wayland explicitly enabled. The lockfile has 267 package entries including the application, across targets. An entry alone does not establish linkage on a given OS. Immediate dependencies of iced in the inspected host tree include iced_core, iced_debug, iced_futures, iced_renderer, iced_runtime, iced_widget, iced_winit and thiserror. Deeper dependencies include winit (native windows), softbuffer (presentation), tiny-skia (rasterization) and cosmic-text (text layout). Use `cargo tree --locked --target x86_64-unknown-linux-gnu` and its `x86_64-pc-windows-msvc` counterpart for the full target graphs, with `-e features` for feature provenance. A transitive crate is not approval to use it as a direct dependency. [Iced feature definitions](https://docs.rs/crate/iced/0.14.0/features)

## Boundaries within the binary

| Owner | Responsibility | Allowed dependencies |
|---|---|---|
| main and Iced-facing Launcher in app | Wire boot, update, view, subscriptions and concrete operations. Interpret a user intent, start work and route results to the right state. | UI, application state, domain, infrastructure and Iced |
| Application state | Own selection, validated shared data, current workflow and operation summaries. Apply synchronous transitions, enforce ordering and select recovery actions. | Domain and standard Rust |
| UI and theme | Compose native controls from borrowed inputs, translate gestures and shortcuts to typed messages, render feedback and local drafts. | Iced, presentation messages, read-only data passed by the caller |
| Domain | Describe valid versions, installations, profiles, runtime requirements and launch inputs. Resolve metadata rules and build a launch specification as pure transformations. | Standard Rust and approved domain-specific types |
| Infrastructure | Resolve platform paths, read/write files, fetch and verify bytes, access credential services, spawn and wait on children. Return typed data or errors. | Domain contracts, OS APIs and approved libraries |

Keep wire messages in a small leaf module. UI dispatches by returning a value. It never imports the Launcher to call back into it. Infrastructure returns results to its caller and never imports app or UI. Domain owns the types shared by its callers. Put an interface at a caller boundary only when multiple backends or a concrete test require substitution. Prefer passing values, functions or a concrete adapter first. Use private modules and narrow visibility. Review imports for direction even when Rust would permit a circular reference.

For example, a future Install click carries a version ID. The application admits a job. Domain logic resolves a validated manifest into required files. Infrastructure streams those files into staging and checks their digests. The application accepts the completion for the matching job and updates the installed inventory. UI reads that inventory next time it renders. No widget or async executor is required to test the manifest rules.

## State, messages and navigation

Keep one authoritative Launcher. A flat update match is sufficient today. With many screens it would accumulate unrelated transitions. A separate application per screen would duplicate account selection, installations and job lifetimes. Prefer a single dispatcher with small feature handlers as messages accumulate. Add a nested message enum when a feature has several related actions. Map its widget messages into the root enum with Element::map. Keyboard translation lives in ui/input.rs, keeping the application message type independent of Iced. The existing Copy derive is convenient for a flag toggle. Reassess it when messages own results, and keep raw credentials out of messages and derived Debug output.

Introduce a closed Route enum with Home and the first approved second destination together. A Navigate(route) message changes selection. The shell matches the route to a borrowed view. Re-selecting the same route has no effect. Introduce Installations, Versions, Profiles, Downloads, Settings and Accounts only when their use cases exist. Installations would show local content, Versions the discoverable catalog, Profiles a configuration and instance selection, Downloads active and recent jobs. Home can summarize those same records by ID. Avoid a route stack until a workflow needs back navigation.

Share settings, selected profile, account display status and operations across routes. A filter can be discarded on exit. Keep an unsaved settings draft and its validation errors until save or explicit discard, with committed settings separately available to running work. Store screen-specific fields only for actual interactions. Views borrow the relevant slice and emit intent. Building a view never triggers filesystem or network access. Keep F1 mapped to startup details on Home. Define its meaning on any additional route during that screen's review.

## Operations and resource ownership

Iced 0.14 exposes Task::perform for a future, Task::run for a stream, and Task::abortable returning a task plus a Handle. Subscription describes an event stream kept alive by its identity while requested. Use these APIs at the application edge. Ordinary synchronous transitions can keep their current update signature until the first effect. [Iced Task](https://docs.rs/iced/0.14.0/iced/struct.Task.html) · [Iced Subscription](https://docs.rs/iced/0.14.0/iced/struct.Subscription.html)

| Work | Proposed execution and completion |
|---|---|
| List installed versions or verify files | On-demand bounded worker for blocking directory traversal or hashing. Send compact results through a task. |
| Download files or install a runtime | Bounded stream of progress and one terminal result. Stream into private staging, verify integrity and validated archive paths before publication. |
| Launch Minecraft | Construct arguments in pure code, validate prerequisites, then spawn via an adapter. Pass arguments individually. Never log a credential-bearing command line. |
| Monitor a child | Keep a single owner of its process handle and receive an exit notification via a blocking wait off the UI and futures threads. Route changes leave ownership intact. Decide exit/detach policy before shipping launch. |
| External events | Keep the existing keyboard subscription. Add a stable identity per external event source only while relevant. No heartbeat to discover work. |

Start with a single active heavy job and a bounded pending queue when the first operation needs queuing. Reject duplicates for the same destination and expose queue-full feedback. Increase parallel transfers only after measurement. Give each attempt a fresh ID. The application owns its visible status and cancellation control across navigation. The worker owns file handles, buffers, staging paths and cleanup. Coalesce progress before dispatch, e.g. emit the latest changed percentage when available, with a bounded update rate for unknown lengths. Always deliver terminal status. Avoid one message per chunk and whole-file allocations.

On cancel, mark cancelling, signal the producer, stop admitting dependent work and wait for cleanup acknowledgement before freeing the destination. Aborting an Iced stream can drop its receiver. It cannot unwind a blocking worker, undo a rename or reap a child. Keep the cleanup acknowledgement path alive, and have the worker check cancellation between chunks. Serialize cancellation with the final publish step. Once publication wins, report completion. Discard stale progress and completions by attempt ID. On close, stop admissions and coordinate cleanup before releasing job ownership. Recover abandoned app-owned staging on a later relevant operation after checking ownership. Keep worker queues bounded and blocking workers separate from the existing Iced futures executor. A runtime-specific HTTP client would require an explicit compatibility decision.

## Paths and persistence

Resolve roots once through a small platform adapter and inject them into file operations. Use PathBuf/Path throughout and preserve native paths. A persisted path representation must round-trip on each supported OS or reject unsupported input visibly. Create directories on first use. The following layout is proposed.

| Purpose | Linux root followed by averion-launcher/ | Windows known folder followed by Averion Launcher/ |
|---|---|---|
| Public settings | XDG_CONFIG_HOME (fallback ~/.config) | LocalAppData, config/ |
| Durable launcher records | XDG_DATA_HOME (fallback ~/.local/share) | LocalAppData, data/ |
| Logs | XDG_STATE_HOME (fallback ~/.local/state), logs/ | LocalAppData, logs/ |
| Rebuildable metadata | XDG_CACHE_HOME (fallback ~/.cache) | LocalAppData, cache/ |

Use absolute XDG overrides and fall back for unset, empty or relative values. Obtain Windows roots through known-folder resolution so redirected folders work. A local default avoids roaming machine-specific paths. Report an unavailable root for the affected action. [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/) · [Windows known folders](https://learn.microsoft.com/en-us/windows/win32/shell/knownfolderid)

Under durable data, profiles/ holds launcher-owned records and game/ holds managed versions/, libraries/, assets/, runtimes/ and instances/<id>/ for per-instance saves and options. A profile references a version, runtime and instance by stable ID. Shared artifacts can serve several instances. Keep user saves outside all cache eviction and staging cleanup. Use unique sibling staging on the destination volume for downloads. Bound expendable metadata by size and evict stale entries on access. Revalidate stale remote indexes on demand. A custom game root or import from an existing .minecraft requires approval. Treat manifest paths and archive entries as untrusted, reject absolute paths and traversal and prevent symlink or junction escapes before writing.

Prefer one small versioned settings file and separate profile records initially. Compare formats in [Decisions required](DECISIONS-REQUIRED.md). On missing settings, use typed defaults in memory (current dark appearance and platform-managed roots). Write only after an explicit successful save. Bound input size, decode to a disk schema, validate field ranges, path suitability and cross-field invariants, then publish the entire settings value. For malformed, unreadable or future-version data, explain the specific recovery action and preserve the original. Allow session defaults where safe, with persistence disabled until recovery. Never silently downgrade an unknown schema.

Migrate supported older schemas one version at a time in memory, validate, preserve a recoverable copy, then replace on successful save. Serialize saves in process. Before enabling simultaneous writers, agree a cross-process exclusion or conflict policy. A timestamp check alone leaves a race. Create a unique, exclusive temporary file beside the destination, write, flush and sync, close, then replace on the same filesystem. Preserve the prior file if replacement fails, including on Windows sharing violations. Clean up only the writer's temporary file. Validate replacement and directory-sync support on each target filesystem before promising crash durability. Never truncate the live file first. [Rust rename semantics](https://doc.rust-lang.org/std/fs/fn.rename.html)

## Failures and diagnostics

Use small feature error enums and Result with the standard Error/source chain and ? at I/O boundaries. Classify a failure where the recovery decision is known. At the application boundary pair a safe summary and action with an operation ID. Keep diagnostic fields allowlisted. A raw upstream error may contain a URL, account data or a local username even when Display looks harmless.

| Failure | Presentation and recovery |
|---|---|
| Invalid UI input or conflicting application action | Field-level feedback or a status beside the action. Preserve the draft or show the active job. |
| Filesystem access or disk full | Identify the operation and invite directory selection, space recovery or retry. Retain the last usable data. |
| Network unavailable or download interrupted | Show retry on the affected job. Use bounded retries only for transient idempotent requests. |
| Download digest mismatch or invalid installation metadata | Discard unverified staging, block install/launch and offer fresh trusted metadata or re-download. |
| Missing/incompatible Java | Show the required runtime and offer runtime selection or approved installation. |
| Authentication, unavailable vault or failed entitlement | Distinguish sign-in required, unlock storage and missing ownership. Block protected operations. Never persist a plaintext fallback. |
| Failed process spawn or non-success exit | Show a launch or exit summary and safe diagnostics. Release resources. |
| Fatal startup failure | If a window exists, show a single actionable message. Otherwise return an error and use available local diagnostics. Treat panic capture as best effort. |

Prefer development stderr with opt-in debug detail. For release, propose warn/error plus sparse info for operation boundaries in a size-limited local log. Windows already uses the GUI subsystem, so a terminal alone is unreliable for end users. Rotate at write time by size with a fixed retention count set at implementation review. Avoid an always-awake logger, per-chunk events and blocking file writes in view/update. Batch routine events off the UI path when I/O is needed. Drop excess low-priority events with a count. If logging fails, preserve core operation and expose diagnostics availability once.

Record version, OS family, stage, safe error category and attempt ID. Exclude tokens, auth codes, cookies, headers, full URLs, paths, account identifiers, full process output and raw panic payloads. Redact before formatting or queueing. A future diagnostics view can show the recent bounded summary and let the user inspect a local export. For crashes record minimal safe context when possible. No automatic upload or dump collection. Test with synthetic secret markers.

## Testing and resource validation

- Unit-test domain resolution and settings validation with table-driven inputs. Drive the application with typed messages and assert state and requested actions directly. Keep keyboard mapping tests at the presentation boundary.
- Exercise infrastructure against a unique temporary root, including missing files, interrupted replacement, invalid encodings, cache loss, Windows sharing failures and cleanup. Supply failures through narrow injected functions when OS errors cannot be induced reliably.
- Use deterministic event sequences for progress, cancellation, duplicate starts, stale completion and cleanup races. Use a controllable local reader/transport and fake child events. Integrate real file adapters and a harmless test child separately. Avoid live sign-in, real game downloads and sleep-based assertions.
- Keep small synthetic metadata, migration and corrupt-file fixtures beside their tests. Use the standard Rust harness. In a binary crate, cross-module tests can live in a crate-local test module. Introduce a library target only if external integration consumers warrant it.
- Retain the Linux/Windows CI matrix. Manually check keyboard, pointer, sizing and legibility on both desktops. Interactive Windows validation is pending. Scope native tests to interactions that headless transitions cannot verify.
- Boot from cheap defaults, defer directory scans and network access, and project only the visible rows for large catalogs when profiling warrants it. Borrow records by ID and release buffers and finished job details. Bound cache, history and download memory. Let Iced schedule redraws in response to events. Defer the [two-platform 2 GB validation](PERFORMANCE.md) until representative launcher workflows exist.

## Later extensions

Keep authentication protocol and OS vault access separate from public account-display metadata. Research Microsoft, Xbox and Minecraft requirements, including ownership and entitlement, before selecting a flow. Isolate a future updater from game downloads: application replacement needs its own trust, platform packaging and rollback policy. Prefer an explicit update check initially. Add feature modules behind these existing boundaries as needs emerge. A plugin system, database and separate service process each need a demonstrated use case and separate approval.
