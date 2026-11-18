# Averion Launcher

A native Rust desktop launcher for Minecraft Java Edition, targeting Vanilla on x86_64 Linux and Windows. Phase 03 opens a blue/cyan launcher shell with seven destinations, original offline scenery, a selected-version preview linking to Versions and a session-only Reduced motion control. Click **Startup details** or press **F1** to inspect the interface and renderer. Game installation, accounts and launching belong to later phases.

## Build and run

Install stable Rust with Cargo and rustup. The repository requests rustfmt and Clippy. Iced itself requires Rust 1.88 or newer. Use current stable for the locked application dependencies.

- **Linux** is the primary development platform. Install a C compiler/linker, pkg-config, X11/XCB, Wayland and xkbcommon development libraries, and system fonts. Run within a Wayland or X11 desktop session. On Ubuntu 24.04 the exact package installation command is in the [CI workflow](.github/workflows/ci.yml).
- **Windows** uses the x86_64 MSVC Rust toolchain, Visual Studio C++ Build Tools and a Windows SDK. The Windows CI runner includes the native toolchain.

```sh
cargo run --locked
cargo fmt --check
cargo test --locked
cargo check --locked
cargo build --release --locked
```

The release binary is under `target/release/` as `averion-launcher` on Linux or `averion-launcher.exe` on Windows.

## Architecture and status

Cargo builds one Rust executable. Iced handles events, widgets and rendering through tiny-skia. Future Minecraft logic belongs in UI-independent Rust modules. SQLite is a candidate for the future local database. The database choice awaits user approval.

Future account support requires secure Microsoft authentication and OS credential storage for authentication secrets. The authentication flow and platform-specific storage choices still require research and user approval.

The launcher must be usable on a computer with **2 GB of total physical RAM**, including its rendering resources and any helper processes. Minecraft has separate requirements. Constrained-system performance measurements and Windows runtime validation are pending. GitHub Actions checks formatting, runs tests and Clippy, checks compilation and builds the release on Linux and Windows.

- [Architecture and renderer decision](docs/ARCHITECTURE.md)
- [Source tree](docs/PROJECT-STRUCTURE.md)
- [Implemented UI and keyboard controls](docs/UI-DESIGN.md)
- [Phase 03 design decisions](docs/DESIGN-DECISIONS.md)
- [Performance and validation](docs/PERFORMANCE.md)
- [Development rules](AGENTS.md)

## License

Averion is intended to be open-source. The project license is undecided.
