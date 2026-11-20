# Application architecture

Cargo builds a single native Rust process. Phase 01 introduced the window and startup screen. Phase 02 isolated keyboard translation and the synchronous disclosure state within the same binary. The approved platforms are x86_64 Linux and Windows.

## Event path

1. The entry point passes the initial state, update and view functions to Iced. It sets the window title, logical dimensions, dark palette and keyboard subscription. It returns `iced::Result`. On Linux, Iced's windowing integration can panic during event-loop initialization if the display is unavailable.
2. Keyboard, window sizing and control presses feed the Iced-facing Launcher. Keyboard translation returns typed intents. The synchronous State owns page, version index, focus, compact menu, reduced motion and disclosure visibility. The local catalog maps stable version IDs to scene families with a fallback.
3. Home, the Versions list and the background all read the same selection. Launcher separately owns a short-lived presentation transition and subscribes to frame events only while it is active. Navigation and traversal can request a scroll position. Views borrow state and never start network or filesystem operations.

Add core modules as domain behavior arrives. Keep version resolution, runtime management, installation, authentication and process launching independently testable. Return typed errors to the application boundary where the UI can present a recovery action without exposing secrets.

## Renderer and dependencies

Iced 0.14.0 and tiny-skia are the direct UI dependencies. Iced's upstream release declares Rust 1.88 and MIT licensing and supports Linux and Windows. Cargo.lock records exact transitive versions. Upstream ships two native renderers. Both are maintained in the Iced project. [Released crate](https://docs.rs/crate/iced/0.14.0) · [Release feature definitions](https://github.com/iced-rs/iced/blob/0.14.0/Cargo.toml)

| Decision factor | tiny-skia | wgpu |
|---|---|---|
| Rendering work | Rasterizes on CPU, presents a pixel buffer to the OS. Redrawing large surfaces can be CPU-intensive. | Submits drawing work to a graphics adapter. Driver availability and adapter selection affect startup and rendering. |
| Memory to account for | CPU-side surfaces, text caches and window buffers | Device resources, textures, staging buffers, text caches and driver allocations |
| Compatibility | Software rasterization works without an application-level graphics API device. A functioning window system is still required. | Requires a compatible backend and driver, such as Vulkan on Linux or Direct3D 12 on Windows. Software adapters depend on the host. |
| Build | Includes the software rasterizer and presentation glue | Adds the GPU abstraction, shader machinery and enabled native backends |

Use tiny-skia alone for the native artwork, text and controls. This removes the need to initialize a compatible GPU adapter. The choice follows the hardware compatibility requirement. Comparative CPU, GPU and memory costs have yet to be benchmarked.

Enabled Iced features are `tiny-skia`, `wayland`, `x11` and `tokio`, plus `image-without-codecs` for bundled raw pixel plates. Iced's Tokio executor runs Reqwest and the download tasks in one runtime. OS window handling comes through winit 0.30.13, CPU rendering through iced_tiny_skia 0.14.1 and tiny-skia 0.11.4, presentation through softbuffer 0.4.8, and font selection/shaping through cosmic-text 0.15.0. Platform-specific dependencies can be inspected with `cargo tree --locked --target x86_64-pc-windows-msvc` or the Linux GNU target.

## Managed data directory

The application uses `~/.averion/` on Linux and `%APPDATA%\.averion\` on Windows. The root is prepared on demand in the download workflow. The standard Rust home-directory and environment APIs supply the base path. The layout separates `versions`, `libraries`, `assets`, `java`, `profiles`, `config`, `cache`, and `logs`. Mojang metadata is stored in `cache/mojang`, client JARs in `versions/<version>`, library JARs and native archives under their Mojang Maven paths in `libraries`, and asset indexes in `assets/indexes`. Asset objects use `assets/objects/<first two hash characters>/<sha1>`. Extracted binaries live in `versions/<version>/natives/<archive sha1>` and keep their archive-relative paths. Managed filesystem verification assumes other processes running under the same account leave the directory hierarchy in place during installation.

## Download infrastructure

A workflow supplies a plan with a component name, destination, expected size and checksum, and an asynchronous response opener for each artifact. Home captures the selected release when Download is pressed, finds that exact stable release in Mojang's official manifest, and verifies its version JSON against the manifest's SHA-1. The resolver requires allowlisted HTTPS hosts and validates IDs and relative paths. It evaluates library rules in manifest order for base Vanilla on x86_64 Linux or Windows, selects legacy classifiers and modern native artifacts, and reports applicable unsupported rule forms as errors. The verified asset index is parsed with bounded input and object counts. Asset hashes are deduplicated before tasks are created.

The shared Reqwest client pools HTTPS connections. Native TLS uses the platform trust store through OpenSSL on Linux and Schannel on Windows. Requests require TLS 1.2 or newer, reject redirects and partial HTTP responses, and have connect, read and overall timeouts. Serde and serde_json perform typed parsing of bounded metadata responses. The manifest is cached for 24 hours, and the selected version's cache entry is keyed by its manifest checksum. Each use revalidates the selected release and its version metadata. Cache updates are staged and published after validation. Only files carrying Averion's cache format marker can be replaced. A failed refresh preserves the previous cache file and reports an error. [Reqwest](https://docs.rs/reqwest/0.13.5/reqwest/) · [Native TLS](https://docs.rs/native-tls/latest/native_tls/)

The app runs one installation at a time on Iced's executor, with at most eight file transfers in flight after verifying the asset index. Each transfer uses a fixed-size buffer. The progress stream reports the current component, aggregate bytes received or verified from cache, and the number of verified files. Aggregate totals become available after the asset index is parsed. Cancellation interrupts metadata and network waits and is checked between file chunks and before publication. A failed or cancelled batch drops its remaining transfers and staging files. A normal window close waits for this cleanup. Already published files remain available for reuse. A forced process exit may leave a staging file.

The manager checks existing regular files against a trusted checksum before reuse and stages each download until its size and checksum match. Mojang plans preserve conflicting destination files, including files created in flight. Native extraction rechecks each archive, rejects links, unsafe paths, overlapping entries and invalid file types, enforces entry and output limits, and extracts platform binaries while skipping metadata and declared exclusions. An extracted file is published without overwriting an existing path. Matching existing native files are compared against the verified archive and reused. Retry re-resolves the same version and verifies existing files again. Starting the app does not initiate a disk scan.

`futures` was already in the Iced dependency graph and supplies the executor-independent stream and cancellation primitives. The RustCrypto `sha1` and `sha2` crates verify upstream SHA-1, SHA-256 and SHA-512 digests. SHA-1 is included for legacy compatibility and depends on the upstream metadata trust model. Prefer SHA-256 or SHA-512 when available. `tempfile` supplies unique staging files, cleanup and replacement on Linux and Windows. Rust's standard library alone has no asynchronous reader trait, digest implementation or temporary-file guard. [RustCrypto hashes](https://github.com/RustCrypto/hashes) · [Temporary file publication](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist)

`zip` reads native JAR archives with only the DEFLATE backend enabled in addition to stored entries. The Rust standard library supplies neither ZIP parsing nor decompression. The extraction code applies its own path and entry policies before it writes any archive contents. [ZIP reader documentation](https://docs.rs/zip/9.8.1/zip/read/struct.ZipArchive.html)

## Java runtime management

The selected release's verified `javaVersion` metadata supplies the required Java major and Mojang runtime component. After verifying the game files, the installer checks Java home variables, absolute PATH entries, and common system and vendor directories. It probes native x86-64 Java executables directly from a temporary working directory with bounded output and a four-second deadline per candidate. Compatibility requires the exact Java major and a 64-bit x86 VM. Probing omits inherited Java injection options. The launcher leaves system Java files, settings, and environment unchanged.

When no system Java matches, the launcher checks stored runtime packages for that major. It verifies their files, declared Linux links, and compatibility before reuse. Fresh provisioning uses Mojang's official catalog for the current platform and component. The runtime manifest is checked against its catalog size and SHA-1, then cached by digest. Packages live side by side in `.averion/java/<major>/<platform>/<manifest-sha1>/`. The verified manifest allows reuse without refreshing the runtime catalog. Previously verified compressed files are retained separately to support retries.

The download manager verifies each raw or compressed file. When Mojang provides a smaller LZMA payload, the launcher decodes it with a per-decoder memory limit and verifies the output size and checksum before publication. Paths and parent directories come from the manifest. Only in-package Linux links to declared files or directories are allowed, and Windows manifests with links are rejected. The installer preserves conflicting files and rejects unexpected entries before probing a managed package. It records the selected Java executable and confirmed major for the current session only, after both the game files and Java pass verification. `lzma-rust2` provides the decoder with optional encoding and other archive formats disabled. Tokio's process support reuses the launcher runtime for timeouts and cancellation. [Official Mojang runtime catalog](https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json) · [LZMA decoder](https://docs.rs/lzma-rust2/0.25.0/lzma_rust2/struct.LzmaReader.html)

## Later phases

- Prepare Minecraft launch inputs using the verified game files and selected Java executable.
- SQLite is proposed for local non-secret data. The database choice awaits user approval. If SQLite is selected, review the Rust binding, schema and migrations before implementation.
- Account security requires secure Microsoft authentication, account ownership and Minecraft entitlement checks, and OS credential storage for authentication secrets. Handle unavailable credential stores explicitly and keep tokens out of plaintext files, application settings and logs.
- Before implementing accounts, verify current Microsoft, Xbox and Minecraft requirements and research credential storage on Linux and Windows. The authentication flow, platform storage APIs and Rust crates require user approval.
