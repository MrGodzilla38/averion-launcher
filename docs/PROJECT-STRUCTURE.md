# Source tree

```text
.github/workflows/ci.yml    Linux and Windows Cargo validation
.gitignore                 Local Cargo build output
AGENTS.md                  Project engineering and phase rules
Cargo.toml                 Binary package, version and Iced features
Cargo.lock                 Reproducible dependency resolution
rust-toolchain.toml        Stable Rust and development components
README.md                  Setup, scope and project status
docs/
  ARCHITECTURE.md          Event path, renderer and future core boundaries
  PERFORMANCE.md           Measurement protocol and validation record
  PROJECT-STRUCTURE.md     Source map
  APP-ARCHITECTURE.md      Implemented shell and guidance for later features
  DECISIONS-REQUIRED.md    Shell choices and outstanding product decisions
  PHASE-02-PLAN.md         Accepted stages, outcomes and verification record
  PROPOSED-PROJECT-STRUCTURE.md  Phase 02 tree and deferred module proposals
  UI-DESIGN.md             Native widget and interaction guidance
src/
  main.rs                  Desktop entry point
  lib.rs                   Window setup and download workflow entry points
  app.rs                   Iced update and view delegation, message-path test
  app/installation.rs      Active download plan, cancellation, retry and progress updates
  app/state.rs             Route, selection, focus order, preferences and disclosure tests
  catalog.rs               Local sample IDs and deterministic scene lookup
  download.rs              Sequential transfers, byte progress and safe publication
  download/batch.rs        Bounded parallel verification and aggregate progress
  download/integrity.rs    Trusted digest verification
  download/tests.rs        Deterministic transport and real filesystem tests
  game_directory.rs        Managed Linux and Windows data layout
  http.rs                  Shared HTTPS transport and request limits
  mojang.rs                Mojang manifest, release validation and download planning
  mojang/assets.rs         Bounded parsing and hash-based asset object plans
  mojang/cache.rs          Bounded, staged metadata cache
  mojang/java.rs           Runtime requirements, Mojang catalog and managed reuse
  mojang/java/detect.rs    System Java discovery and bounded compatibility probe
  mojang/java/package.rs   Runtime manifest validation and verified provisioning
  mojang/java/tests.rs     Catalog, compatibility and opt-in provisioning tests
  mojang/java/package/tests.rs  Package, path and decoder safety fixtures
  mojang/libraries.rs      Platform rules, library JARs and native classifiers
  mojang/libraries/tests.rs  Rule and artifact fixtures
  mojang/natives.rs        Verified ZIP extraction with no-clobber publication
  mojang/natives/tests.rs  Archive safety and reuse fixtures
  mojang/tests.rs          Metadata fixtures and opt-in live HTTPS verification
  message.rs               Application message enum
  theme.rs                 Central dark palette
  ui/
    mod.rs                 Shared shell, navigation and controls
    input.rs               Iced keyboard event translation and tests
    home.rs                Selected scene, Versions entry card, download status and F1 disclosure
    pages.rs               Six secondary destinations and announcement rows
    art.rs                 Stable handles for offline artwork and icons
assets/terrain/            Original generated RGBA foreground/background plates and authoring script
assets/icons/              Original icon strip and its vector source
```

The download manager and Mojang resolver are independent of the UI. The application bridges selected-version resolution and downloads into Iced messages. Future domain and infrastructure modules are listed in the [growth map](PROPOSED-PROJECT-STRUCTURE.md#growth-map-once-a-feature-has-a-caller).
