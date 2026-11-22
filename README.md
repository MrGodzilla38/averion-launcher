# Averion Launcher

A native Rust desktop launcher for Minecraft Java Edition, targeting Vanilla on x86_64 Linux and Windows. The launcher shell has seven destinations, original offline scenery, a selected-version preview linking to Versions and a session-only Reduced motion control. Click **Startup details** or press **F1** to inspect the interface and renderer.

Select a Minecraft release and press **Download** on Home. Averion verifies the selected release's game files, extracts its native libraries, and selects a compatible 64-bit Java runtime using Mojang's version metadata. It can reuse system Java or provision Mojang's runtime under `.averion/java/`. Progress, Cancel, and Retry are available from Home. Averion stores its files under `~/.averion/` on Linux and `%APPDATA%\.averion\` on Windows. After installation, save an offline username under **Accounts**, then select **Play** on Home. Play revalidates the installation and starts Minecraft in a separate process. Use **Repair** if Averion finds missing or damaged files. The latest launch status names the output folder under `.averion/logs/`. [Download and Java architecture](docs/ARCHITECTURE.md#java-runtime-management)

## Build and run

Install stable Rust with Cargo and rustup. The repository requests rustfmt and Clippy. Iced itself requires Rust 1.88 or newer. Use current stable for the locked application dependencies.

- **Linux** is the primary development platform. Install a C compiler/linker, pkg-config, OpenSSL development libraries and a system CA bundle, X11/XCB, Wayland and xkbcommon development libraries, and system fonts. Run within a Wayland or X11 desktop session. On Ubuntu 24.04 the exact package installation command is in the [CI workflow](.github/workflows/ci.yml).
- **Windows** uses the x86_64 MSVC Rust toolchain, Visual Studio C++ Build Tools and a Windows SDK. The Windows CI runner includes the native toolchain.

```sh
cargo run --locked
cargo fmt --check
cargo test --locked
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

The release binary is under `target/release/` as `averion-launcher` on Linux or `averion-launcher.exe` on Windows.

## Architecture and status

Cargo builds one Rust executable. Iced handles events, widgets and rendering through tiny-skia. Future Minecraft logic belongs in UI-independent Rust modules. SQLite is a candidate for the future local database. The database choice awaits user approval.

Offline profiles support local play with a UUID derived from the username. Microsoft sign-in, account ownership, entitlement checks, and authenticated multiplayer are pending.

The launcher must be usable on a computer with **2 GB of total physical RAM**, including its rendering resources and any helper processes. Minecraft has separate requirements. Constrained-system performance measurements and Windows runtime validation are pending. GitHub Actions checks formatting, runs tests and Clippy, checks compilation and builds the release on Linux and Windows.

- [Architecture and renderer decision](docs/ARCHITECTURE.md)
- [Source tree](docs/PROJECT-STRUCTURE.md)
- [Implemented UI and keyboard controls](docs/UI-DESIGN.md)
- [Phase 03 design decisions](docs/DESIGN-DECISIONS.md)
- [Performance and validation](docs/PERFORMANCE.md)
- [Development rules](AGENTS.md)

## License

Averion is intended to be open-source. The project license is undecided.
