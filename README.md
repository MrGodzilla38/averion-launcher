# Averion Launcher

Averion is a native desktop launcher for Minecraft Java Edition on x86_64 Linux and Windows. It builds as a single Rust executable with Iced and the tiny-skia renderer. Every installation and launch uses Fabric Loader.

## Install and play

1. Open **Versions**, choose a version family, then select a Minecraft release. The built-in catalog lists 35 releases from 1.16.5 through 26.3. Installation requires compatible builds of Fabric Loader, Fabric API and Averion Client for the exact release.
2. Select **Download** on **Home**. Averion downloads and verifies the game files, extracts native libraries and uses Mojang metadata to select the required Java version. It reuses a compatible 64-bit system Java installation or downloads Mojang's runtime. **Home** shows progress and offers **Cancel** during installation or **Retry** after a failure.
3. Open **Profile** from the top-right profile button. Enter a username with 3–16 ASCII letters, digits or underscores and choose **Save offline profile**. Select **Use account** to switch to another saved profile. Offline profiles support local play.
4. Return to **Home** and select **Play**. Averion checks the installation and starts Minecraft in a separate process. Use **Repair** when the launcher reports missing or damaged files. The launch status gives the output folder containing `stdout.log` and `stderr.log`.

Installation, repair and managed mod updates use internet access to resolve and download files. If installation reports a missing Averion Client build, choose a release with a published `averion-client-<version>.jar` and a SHA-256 digest. An installed version can start using local files with an offline profile. [Installation and verification details](docs/ARCHITECTURE.md#fabric-installation)

**Settings** includes a **Reduced motion** control for the current launcher session. On **Home**, click **Startup details** or press **F1** to see the launcher version and renderer. [Interface and keyboard controls](docs/UI-DESIGN.md)

## Manage mods

Every installation includes Fabric Loader, Fabric API and Averion Client. Averion also installs compatible releases of the optional curated mods Sodium, Lithium and FerriteCore when available.

On **Mods**, select **Update Fabric and managed mods** to resolve the latest compatible stack for the selected installed version. **Browse Modrinth** and **Open mods folder** help you add or remove your own Fabric JARs. Manual mods have a separate folder for each Minecraft release. Worlds and game settings are shared across releases in the Averion game directory.

## Microsoft sign-in

**Sign in with Microsoft** is temporarily disabled in **Profile**. Offline profiles are available in the current build.

The authentication implementation uses browser sign-in and checks Minecraft Java ownership and the player's profile. It stores Microsoft refresh credentials in Secret Service on Linux or Credential Manager on Windows.

Developer builds with sign-in enabled require a Microsoft Entra registration that supports personal accounts, lists `http://localhost` under **Mobile and desktop applications**, and has Minecraft Java Game Service API approval. Supply the public **Application (client) ID** as a hyphenated UUID through `AVERION_MICROSOFT_CLIENT_ID` at build time or before starting Averion. A runtime value takes precedence over the build-time default. On Linux, the flow also requires `xdg-open` and an available, unlocked Secret Service provider. [Registration and authentication details](docs/ARCHITECTURE.md#microsoft-sign-in-registration-prerequisites)

## Game files and logs

Averion stores its data under `~/.averion/` on Linux and `%APPDATA%\.averion\` on Windows.

| Path relative to the game directory | Contents |
| --- | --- |
| `saves/` | Local worlds |
| `versions/<version>/` | Per-version game files and managed Fabric stack |
| `profiles/<version>/mods/` | Manually installed mods for that version |
| `java/` | Java runtimes downloaded by Averion |
| `logs/minecraft-*/` | Captured output for each Minecraft launch |

## Build and run from source

Install Rust through rustup. The repository requests stable Rust with rustfmt and Clippy.

- **Linux** requires an X11 or Wayland desktop session. Install a C compiler and linker, pkg-config, OpenSSL development libraries, CA certificates, X11/XCB, Wayland and xkbcommon development libraries, and system fonts. The CI workflow lists the packages for Ubuntu 24.04. [Linux build dependencies](.github/workflows/ci.yml)
- **Windows** uses the x86_64 MSVC Rust toolchain, Visual Studio C++ Build Tools and a Windows SDK.

From the repository root, start the launcher with:

```sh
cargo run --locked
```

Build an optimized executable with:

```sh
cargo build --release --locked
```

The executable is `target/release/averion-launcher` on Linux and `target/release/averion-launcher.exe` on Windows.

## Development and status

GitHub Actions runs formatting, tests, compilation checks, Clippy and release builds on Linux and Windows. Run the development checks locally with:

```sh
cargo fmt --check
cargo test --locked
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
```

The launcher targets machines with **2 GB of total physical RAM**. Minecraft has separate requirements. Usability on 2 GB systems and interactive Windows runtime testing remain pending. [Performance measurements and validation plan](docs/PERFORMANCE.md)

- [Architecture](docs/ARCHITECTURE.md)
- [Source tree](docs/PROJECT-STRUCTURE.md)
- [UI design decisions](docs/DESIGN-DECISIONS.md)
- [Development rules](AGENTS.md)

## License

Averion is intended to be open-source. The project license is undecided.
