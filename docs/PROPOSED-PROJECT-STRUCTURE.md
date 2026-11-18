# Proposed project structure

The repository stays one Cargo package and one desktop binary. Paths below are relative to the repository root. The first tree preserves the historical Phase 02 shell and complements the [current source map](PROJECT-STRUCTURE.md). The second tree records possible future files.

## Implemented Phase 02 shell

```text
Cargo.toml                    Package and explicit GUI features
Cargo.lock                    Exact resolved versions
rust-toolchain.toml           Stable toolchain with rustfmt and Clippy
.github/workflows/ci.yml      Ubuntu and Windows verification
AGENTS.md                     Scope, security and development rules
README.md                     Entry point for setup and project status
docs/                         Design records, Phase 02 outcomes and open decisions
src/
  main.rs                     Boot, native window, theme and subscription wiring
  app.rs                      Iced-facing Launcher, update and view delegation
  app/
    state.rs                  Disclosure state, synchronous toggle and headless tests
  message.rs                  Application message vocabulary
  theme.rs                    Shared dark palette
  ui/
    mod.rs                    Private UI organization and module visibility
    input.rs                  Keyboard event to message translation and headless tests
    home.rs                   Home presentation, disclosure button and details
```

The app.rs plus app/state.rs layout uses Rust's existing file-module convention. The parent app module hosts the Iced adapter. Its state submodule holds the authoritative disclosure value, so the adapter need not mirror it. Keep the public surface within the crate as small as callers permit.

## Growth map once a feature has a caller

These paths illustrate ownership for later reviews. Creating empty files would obscure which features actually work. Keep small responsibilities together until the split simplifies a real operation.

```text
src/
  app/
    navigation.rs             Route values and navigation transitions
    settings.rs               Validated current settings and draft/save workflow
    operations.rs             Admission, attempt IDs, cancellation and UI status
    feedback.rs               Failure-to-user-action mapping
  ui/
    shell.rs                  Shared layout and active-view selection
    installations.rs          Local installation inventory
    versions.rs               Discoverable version catalog
    profiles.rs               Profile and instance selection
    downloads.rs              Borrowed progress summaries and user actions
    settings.rs               Draft controls and field-level feedback
    accounts.rs               Public account status and sign-in intents
  domain/
    mod.rs                    Domain module declarations
    versions.rs               Metadata rules and version resolution
    installation.rs           Required artifacts and install completeness
    profiles.rs               Profile, instance and version references
    java.rs                   Runtime requirements and compatibility
    launch.rs                 Validated game launch specification
  infrastructure/
    mod.rs                    Concrete adapter declarations
    paths.rs                  Resolve and validate OS-specific roots
    settings.rs               Disk schema, migration and safe replacement
    downloads.rs              Transport, streaming and digest verification
    installs.rs               Staging and publication of verified artifacts
    runtimes.rs               Locate, provision and inspect Java installations
    processes.rs              Spawn, handle ownership and exit notification
    accounts.rs               Approved authentication protocol interaction
    credentials.rs            Platform secret-store access
    diagnostics.rs            Redacted local sinks and retention
    updates.rs                Approved release source and update verification
```

Give a shared settings value its own plain type when both the application and disk codec need it, with conversion at the adapter boundary. Keep disk representations private to infrastructure. Separate profile metadata from the mutable content of an instance. Domain launch specifications should use account references where possible. Supply short-lived secret material only at the protected operation boundary.

A view receives narrow inputs and emits messages. The composition point in app.rs may import the view and an adapter. Neither imports app.rs in return. The domain never imports Iced, tasks, paths resolved from the environment or infrastructure. Every future module should have a working caller and tests before it lands. Start with module-local tests, then introduce fixture directories only with the corresponding feature. For the installed application's on-disk layout see [Paths and persistence](APP-ARCHITECTURE.md#paths-and-persistence).
