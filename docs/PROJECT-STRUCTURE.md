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
  main.rs                  Entry point and window setup
  app.rs                   Iced update and view delegation, message-path test
  app/state.rs             Route, selection, focus order, preferences and disclosure tests
  catalog.rs               Local sample IDs and deterministic scene lookup
  message.rs               Application message enum
  theme.rs                 Central dark palette
  ui/
    mod.rs                 Shared shell, navigation and controls
    input.rs               Iced keyboard event translation and tests
    home.rs                Selected scene, Versions entry card, Launch and F1 disclosure
    pages.rs               Six secondary destinations and announcement rows
    art.rs                 Stable handles for offline artwork and icons
assets/terrain/            Original generated RGBA foreground/background plates and authoring script
assets/icons/              Original icon strip and its vector source
```

Add UI-independent core modules when the first launcher operation needs them. Phase 03 extends the Phase 02 shell with presentation state and local samples. Future domain and infrastructure modules are listed separately in the [growth map](PROPOSED-PROJECT-STRUCTURE.md#growth-map-once-a-feature-has-a-caller).
