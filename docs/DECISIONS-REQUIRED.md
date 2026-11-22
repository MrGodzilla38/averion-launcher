# Decisions requiring approval

Phase 02 decision record. Rust, Cargo, the single native application, Iced 0.14, tiny-skia, the dark Modern Gaming direction, x86_64 Linux/Windows and the initial Vanilla scope are established. Phase 02 implemented the approved keyboard and state extractions. The options below for future features still require separate review. A format preference does not approve a parser, dependency or storage implementation.

Phase 03 superseded the route deferral and enabled Iced image handles without codecs. Phase 05 superseded the historical data location and transport recommendations below. It established `~/.averion/` and `%APPDATA%\.averion\` and selected Reqwest with native TLS on Iced's Tokio executor. Serde JSON covers Mojang metadata. The public settings format and other persistence decisions remain open. [Current architecture](ARCHITECTURE.md)

## Shell choices reviewed in Phase 02

| Question | Options and consequences | Recommendation |
|---|---|---|
| How much to extract now? | Keep the current flat shell for the smallest diff, at the cost of input mapping sharing the application message module. Or isolate keyboard translation and plain state using the existing disclosure as a working example. A multi-crate workspace would enforce public boundaries but introduce package APIs and build maintenance immediately. | Keep the single crate. The two extractions are complete in the [plan](PHASE-02-PLAN.md). |
| Who owns state? | Independent screen models suit isolated workflows but complicate shared jobs and drafts. One undivided root is simple today but invites a large match. A root with feature handlers preserves one owner while allowing local interaction state. | Root plus handlers as features arrive. |
| When to introduce routes? | Creating all seven destinations now fixes information architecture early and produces empty screens. Waiting preserves a working Home and lets the second use case establish the navigation contract. | Defer route and UI until two real destinations are approved. Review destination labels, shortcuts and draft behavior with that feature. |

## Decisions before persistence or diagnostics

| Question | Options and consequences | Recommendation |
|---|---|---|
| Public settings format | TOML provides comments and named sections for manual edits, with an additional parser and possible loss of comments on rewrite. JSON provides a small structured interchange format that may reuse a future metadata parser, with no standard comments. A custom line format avoids a crate but creates escaping, migration and validation work. | Prefer JSON if machine-managed settings and reuse are priorities. Choose TOML if preserving human edits is a product requirement and decide how to handle comments. Both need schema versions. [JSON specification](https://www.rfc-editor.org/rfc/rfc8259) · [TOML specification](https://toml.io/en/v1.0.0) |
| Data location | App-managed roots keep installs isolated. Reusing a shared Minecraft directory saves duplication but creates conflicts and deletion risks. Custom roots support external drives and introduce availability and path-validation cases. Roaming Windows preferences require separating portable values from machine paths. | Start with XDG roots and Windows LocalAppData. Keep game content in an app-managed subtree. Defer shared/imported installations and roaming. |
| Concurrent settings saves | Exclusive writer across processes simplifies consistency but may deny a second session write access. Coordinated locking and version comparisons allow explicit conflicts at the cost of more failure paths. | Allow one writer with read-only fallback for competing sessions. Approve a platform mechanism and failure recovery before persistence ships. |
| Local database | Structured files give direct recovery and simple backups. Cross-file transactions and large queries are harder. SQLite provides transactions and indexing but needs a binding, migrations, locking and recovery tests. | Use ordinary records for the first launcher if queries and atomicity fit one file. Revisit SQLite when a specific operation needs cross-record transactions or measured query performance warrants it. |
| Diagnostics | Development stderr is easy to inspect but may be unavailable on end-user desktops. A bounded release file survives restart and costs disk writes. A memory-only summary avoids persistent data but is lost on crash. | Propose stderr in development and a bounded local file plus a user-visible summary for release. Agree on retention, levels and export consent before enabling storage. |

## Decisions before operations and distribution

| Question | Options and consequences | Recommendation |
|---|---|---|
| Executor and transport | An executor-neutral client fits the Iced thread pool if TLS and streaming work on both targets. A bounded blocking adapter simplifies compatibility at the cost of occupied threads. A runtime-bound client may require changing the executor and dependency graph. | Evaluate streaming, cancellation, certificate validation and proxy behavior with a concrete HTTP requirement. Keep the established executor in Phase 02. |
| Process lifetime | Detach on launcher exit preserves play but relinquishes monitoring and may require redirecting output. Waiting for exit keeps ownership but delays closing. Termination risks loss of game progress. | Obtain a product decision before launch support. Prefer allowing the game to continue if streams and handles can be safely released. |
| Accounts and vaults | Phase 10 uses system-browser Authorization Code Flow with PKCE and OS-backed refresh tokens. | Rust Keyring uses Linux Secret Service and the current user’s local Windows Credential Manager. Vault failures surface as account errors. Live verification requires an approved Minecraft API registration. |
| Application updates | A user-initiated release link is simple but requires manual installation. In-app replacement needs verified artifacts, rollback, packaging and platform-specific recovery. | Defer selection until the release/distribution model is known. |
| Project license | Permissive terms and reciprocal terms impose different downstream obligations. Deferral leaves redistribution permissions unsettled. | Owner to select terms separately before inviting redistribution. The upstream GUI license does not choose Averion's license. |

## Dependency review by capability

No candidate below is selected. Compare exact versions, upstream activity, security advisories, maintenance ownership, MSRV, feature graph, target builds and release footprint at the time of a concrete request. Bring the evidence and alternatives to the owner before changing Cargo files.

| Possible addition | Benefit and necessity trigger | Costs, maintenance and platform considerations |
|---|---|---|
| Structured format parser/serializer | Required once persisted user settings have a format. A maintained codec avoids hand-written parsing and may also serve version metadata. | Parser, derive and feature costs vary. Review input bounds, schema compatibility and update history. Test round trips on Linux and Windows. |
| Platform directory resolver or small OS binding | Use if standard Rust plus XDG handling cannot reliably resolve redirected Windows known folders. | An OS binding adds target-specific maintenance. A portable wrapper can hide errors or choose different defaults. Verify actual paths on both targets. |
| HTTP/TLS client and integrity/archive libraries | Required for trusted network retrieval, expected digest checks and safe extraction. Reuse one client across operations. | TLS backends, compression and async features can add substantial build scope, buffers and worker requirements. Review trust roots, advisory history and archive path safety on both platforms. |
| Logging facade/sink and error derive helper | Helpful if several modules need filtering, rotation or repetitive source-chain boilerplate. Standard Rust suffices for the shell. | Avoid parallel logging stacks. Review formatting/redaction paths and feature costs. Test file rotation with open handles on Windows and Linux. |
| Credential-store integration | Phase 10 persists refresh tokens through the native Keyring backends. | Linux Secret Service is tested with a synthetic credential. Windows still needs a native Credential Manager round trip. |
| SQLite binding | Useful if a proven transactional or indexed workload exceeds structured files. | Engine footprint, bundled/system choices, schema migrations and locking need continuing tests on both targets. Defer. |
| Async runtime or test framework | A second executor is justified only by an approved hard requirement after comparing alternatives. The built-in test harness already covers the state boundary. | Extra scheduling, features or harness setup complicate tests, shutdown and platform CI. No present requirement. |
