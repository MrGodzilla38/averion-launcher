# Application architecture

Cargo builds a single native Rust process. Phase 01 introduced the window and startup screen. Phase 02 isolated keyboard translation and the synchronous disclosure state within the same binary. The approved platforms are x86_64 Linux and Windows.

## Event path

1. The entry point passes the initial state, update and view functions to Iced. It sets the window title, logical dimensions, dark palette and keyboard subscription. It returns `iced::Result`. On Linux, Iced's windowing integration can panic during event-loop initialization if the display is unavailable.
2. Keyboard, window sizing and control presses feed the Iced-facing Launcher. Keyboard translation returns typed intents. The synchronous State owns page, version index, focus, compact menu, reduced motion and disclosure visibility. The local catalog maps stable version IDs to scene families with a fallback.
3. Home, the Versions list and the background all read the same selection. Launcher separately owns a short-lived presentation transition and subscribes to frame events only while it is active. Navigation and traversal can request a scroll position. Views borrow state and never start network or filesystem operations.

Add core modules as domain behavior arrives. Keep version resolution, runtime management, installation, authentication and process launching independently testable. Return typed errors to the application boundary where the UI can present a recovery action without exposing secrets.

## Renderer and dependencies

Iced 0.14.0 is the only direct crate. The upstream release declares Rust 1.88 and MIT licensing and supports Linux and Windows. Cargo.lock records exact transitive versions. Upstream ships two native renderers. Both are maintained in the Iced project. [Released crate](https://docs.rs/crate/iced/0.14.0) · [Release feature definitions](https://github.com/iced-rs/iced/blob/0.14.0/Cargo.toml)

| Decision factor | tiny-skia | wgpu |
|---|---|---|
| Rendering work | Rasterizes on CPU, presents a pixel buffer to the OS. Redrawing large surfaces can be CPU-intensive. | Submits drawing work to a graphics adapter. Driver availability and adapter selection affect startup and rendering. |
| Memory to account for | CPU-side surfaces, text caches and window buffers | Device resources, textures, staging buffers, text caches and driver allocations |
| Compatibility | Software rasterization works without an application-level graphics API device. A functioning window system is still required. | Requires a compatible backend and driver, such as Vulkan on Linux or Direct3D 12 on Windows. Software adapters depend on the host. |
| Build | Includes the software rasterizer and presentation glue | Adds the GPU abstraction, shader machinery and enabled native backends |

Use tiny-skia alone for the native artwork, text and controls. This removes the need to initialize a compatible GPU adapter. The choice follows the hardware compatibility requirement. Comparative CPU, GPU and memory costs have yet to be benchmarked.

Enabled Iced features are `tiny-skia`, `wayland`, `x11` and `thread-pool`, plus `image-without-codecs` for bundled raw pixel plates. The thread pool supplies Iced's native futures executor. OS window handling comes through winit 0.30.13, CPU rendering through iced_tiny_skia 0.14.1 and tiny-skia 0.11.4, presentation through softbuffer 0.4.8, and font selection/shaping through cosmic-text 0.15.0. Platform-specific dependencies can be inspected with `cargo tree --locked --target x86_64-pc-windows-msvc` or the Linux GNU target.

## Later phases

- Use Iced tasks to send async completion or failure messages. Add explicit cancellation and bound concurrency for I/O. Keep blocking filesystem and database operations off the UI thread. Use one async runtime.
- SQLite is proposed for local non-secret data. The database choice awaits user approval. If SQLite is selected, review the Rust binding, schema and migrations before implementation.
- Future HTTP operations should reuse connections, stream downloads incrementally, verify trusted metadata and integrity, and clean up cancelled transfers. HTTP client and TLS features require platform validation.
- Account security requires secure Microsoft authentication, account ownership and Minecraft entitlement checks, and OS credential storage for authentication secrets. Handle unavailable credential stores explicitly and keep tokens out of plaintext files, application settings and logs.
- Before implementing accounts, verify current Microsoft, Xbox and Minecraft requirements and research credential storage on Linux and Windows. The authentication flow, platform storage APIs and Rust crates require user approval.
