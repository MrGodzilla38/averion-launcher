# Performance validation

The complete launcher must stay usable on a machine with 2 GB total physical RAM. This includes the OS sharing that RAM, renderer allocations and all launcher helper processes. Evaluate the launcher independently of Minecraft. Build on a development machine and test the release executable on the constrained system.

## Reproducible measurement protocol

1. Use a physical machine or VM with 2 GB assigned, separately on Linux and Windows. Record OS, CPU, RAM, graphics driver, screen size, scale, desktop/session, swap or pagefile, build revision, renderer and measurement tools.
2. Establish an OS idle baseline. Time a cold launch through the first rendered window, then close and repeat for warm startup. Sample the whole process tree during startup and record peak and settled memory.
3. Leave the window idle for a recorded interval. Record CPU time over elapsed time, memory and compositor/GPU activity where attributable. Repeat at the same scale and geometry. Record sampling intervals and any other workloads.
4. Toggle startup details, resize and scroll. Check input latency and frame stalls under memory pressure. Repeat after an extended idle session, look for persistent growth and confirm resources are released at shutdown.
5. Once downloads exist, repeat while streaming a large file and after cancellation. Track peak memory, responsiveness, temporary resources and behavior over repeated operations. Bound queues, release buffers and avoid whole-file allocations.

On Linux sample process RSS, PSS from smaps_rollup and peak RSS. Summed RSS can double-count shared mappings. On Windows report private working set, commit size and peaks per process, using Performance Monitor or Process Explorer. Record system paging and graphics allocations separately. Keep platform-specific metrics labeled.

## Validation status

The 2 GB usability requirement, GPU usage and extended-session behavior await validation on constrained systems. There is no numerical application memory budget or measured renderer comparison.

In one unconstrained Linux Xvfb release run on 2026-11-17, the harness detected a mapped window after 524 ms and sampled 13,984 KiB RSS at that point. After F1, a button click and resizing to 820 × 520, the process reported 15,088 KiB RSS, 9,744 KiB PSS and 15,088 KiB peak RSS. Its CPU time counters did not advance during a subsequent 10-second idle sample. These figures cover the launcher process on a host with about 19.2 GiB of RAM. The timing includes harness polling and does not measure time to first rendered frame.

### Phase 01 functional checks — 2026-11-17

On x86_64 Omarchy 4.5.0 with Rust/Cargo 1.100.0, `cargo fmt --check`, `cargo test` (one state-transition test), `cargo check`, `cargo build --release` and `cargo clippy --locked --all-targets -- -D warnings` passed.

The release executable opened a native Wayland window under Hyprland and exited successfully after a compositor close request. In a separate Xvfb X11 session, a window capture showed the dark palette, version, title, description and startup status. Sending F1 displayed the renderer details, clicking the button hid them, and a WM_DELETE_WINDOW event closed the process with exit code 0.

The smoke harness initially missed the button text during OCR and raced the X server startup on retry. The successful run used the button position verified from the capture and waited for the fresh display number. An unavailable X display triggers an event-loop initialization panic inside Iced 0.14.1's windowing integration. An active desktop session is required.

Actionlint 1.7.12 accepted the workflow. The local functional checks ran on an unconstrained Linux development machine. For commit `e290b13`, the hosted Ubuntu 24.04 and Windows Server 2022 jobs both passed formatting, tests, compilation checks, Clippy and the release build. An interactive Windows desktop run remains pending. [Hosted CI results](https://github.com/MrGodzilla38/averion-launcher/actions/runs/44169697622)

### Phase 03 UI foundation · 18 November 2026

The Linux release passed format, locked check, 11 headless tests, all-target Clippy with warnings denied, release build and diff whitespace checks. An isolated X11/Xvfb session exercised the 1120 × 720 opening size, 1440 × 900 and 640 × 440 client areas. Window captures were inspected during layout iteration. The final run checked all seven routes, Versions selection and return, previous/next scene changes by pointer and keyboard, profile entry, F1 and button disclosure, Tab/Shift+Tab, Enter/Space, the reduced-motion toggle, compact menu expansion and Escape, compact source placement and normal close. A native Wayland launch also mapped a window and closed through the compositor successfully.

After stepping the carousel and settling at 1120 × 720, the X11 release process reported 25,164 KiB resident and peak resident memory. User/system CPU counters stayed at 1,371/18 ticks across a 10-second idle interval (100 ticks/second). These are process measurements from one unconstrained Linux host with 20,181,280 KiB physical memory. They exclude the compositor and do not establish 2 GB system usability, animation frame times or Windows behavior. Interactive Windows, constrained hardware on both OSes, assistive technology and multi-DPI testing remain open.
