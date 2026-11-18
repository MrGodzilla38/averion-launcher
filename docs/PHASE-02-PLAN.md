# Phase 02 plan and completion record

**Objective.** Give the first real launcher workflow a tested place to live while preserving the Phase 01 experience. The three approved stages are complete under the owner’s closeout instruction. Windows checks unavailable for this working tree remain pending platform validation.

## Review package

- [Application architecture](APP-ARCHITECTURE.md) defines ownership, the Iced event path, future work lifetimes, persistence, diagnostics and verification.
- [Proposed project structure](PROPOSED-PROJECT-STRUCTURE.md) distinguishes the small shell change from modules that become useful only with later features.
- [Decisions required](DECISIONS-REQUIRED.md) compares choices, records recommendations and identifies the feature that would trigger each decision.
- The stage descriptions below preserve the accepted scope and criteria. The verification record states which checks ran.

## Accepted implementation stages

### 1. Separate input translation from application messages

**Completed.** The keyboard subscription now calls `ui::input::shortcut`. The existing button and the initial F1 press both emit `Message::ToggleDetails`. Four tests cover initial press, release, repeat and an unrelated key.

**Objective and prerequisite.** Preserve the existing event contract while making the message vocabulary usable without keyboard types. Requires approval of the single-crate shell plan and an explicit Stage 1 assignment.

**Expected files.** Add `src/ui/input.rs`. Edit `src/main.rs`, `src/ui/mod.rs` and `src/message.rs`.

**Decision and tasks.** Keep ToggleDetails as a plain typed intent. Move the current shortcut function to UI input, declare that module and point the existing filtered keyboard subscription at it. Keep one subscription for F1.

**Tests and acceptance.** Add local input-mapping tests for initial F1 press, repeated press, release and unrelated key. Only the first maps to ToggleDetails. The existing update test passes. Check F1 and the visible button on a desktop. Apply the shared Cargo checks below.

**Risk and exclusion.** Different keyboard events could cause two toggles. Preserve the current repeat filter. No new shortcuts, routes, tasks or widget changes.

### 2. Isolate the synchronous state transition

**Completed.** `app::state::State` owns the private visibility flag. The Iced adapter delegates mutation and reads the result for Home. Two state tests and the original application regression cover this path.

**Objective and prerequisite.** Demonstrate the state boundary with existing working behavior. Requires accepted Stage 1 and a Stage 2 assignment.

**Expected files.** Add `src/app/state.rs` and edit `src/app.rs`.

**Decision and tasks.** Keep Launcher as the Iced composition point. Move the disclosure flag and its synchronous transition into a small private state module. Delegate from Launcher::update, and pass the single authoritative flag into the existing Home view. Preserve the entry-point signatures. Keep the existing headless Launcher regression covering the public update path and exercise default and repeated transitions directly in the state module.

**Tests and acceptance.** Both transition paths pass without a display. Review imports to confirm state and message types compile without Iced imports of their own. Launch the shell, open/close details by both inputs and check resize, startup status and version. Run the shared Cargo checks.

**Risk and exclusion.** A second flag in the adapter would drift out of sync. Use one value. No generic reducer framework, effect enum, job registry, service trait or future screen scaffold.

### 3. Verify and document the resulting shell

**Completed within the available-platform scope.** The baseline architecture, source map and Phase 02 design documents now describe the implemented modules. Source review found no further implementation work.

**Objective and prerequisite.** Reconcile the baseline documentation with accepted code and record platform evidence. Requires accepted Stage 2 and a Stage 3 assignment including permission to update the two existing documents.

**Expected files.** Update `docs/ARCHITECTURE.md` and `docs/PROJECT-STRUCTURE.md`. The final assignment also authorized reconciling the four Phase 02 documents with the implementation.

**Decision and tasks.** Update the event path and tree to match the verified implementation. Preserve historical facts and identify pending checks. Run the shared checks and inspect native interactions on Linux and Windows. Use the existing CI matrix when a separately authorized repository action runs it.

**Tests and acceptance.** Trace F1 and button from input to state to Home. Verify opening, dismissal, repeated F1 suppression, status and version text, scroll/resize and clean close. Record revision, platform, checks and results. The original acceptance criteria called for automated and desktop checks on both targets. At closeout the owner authorized completion with unavailable Windows checks recorded separately.

**Risk and exclusion.** A hosted Windows build cannot establish interactive desktop behavior. No changes to CI, source, licensing, dependencies or performance claims in this stage.

## Verification shared by implementation stages

Use the repository commands on supported targets.

```sh
cargo fmt --check
cargo test --locked
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

Review the diff against the exact file list at each stage. Keep builds locked. Confirm the initial dark Home screen, title, size constraints, status, app version and input behavior still match Phase 01. Existing platform constraints and limits on accessibility validation remain documented in [UI design](UI-DESIGN.md).

## Verification record

Verified on 2026-11-18 against the uncommitted Phase 02 working tree based on `4166310`. That commit alone identifies the baseline and does not contain the two stage implementations.

| Check | Evidence |
|---|---|
| Linux Cargo suite | Formatting, locked check, all seven unit tests, Clippy with warnings denied, and the locked release build passed. Git diff whitespace check passed. |
| Linux release window | The dark Home screen displayed the startup status and version wordmark. In an isolated X11 window under Xwayland, F1 opened and closed details. The button opened details, the text remained accessible after resizing to 360 × 300 and scrolling, and the button closed details after enlargement to 820 × 520. A normal window close request exited with status 0. |
| Windows automated checks | The workflow checks out the triggering revision and runs the same Cargo checks on windows-2022. The hosted Windows job passed for the earlier Phase 01 revision `e290b13`. The current source is local and uncommitted. No hosted result covers these edits, and only the Linux Rust target was installed for local verification. [Hosted run](https://github.com/MrGodzilla38/averion-launcher/actions/runs/44169697622) |
| Windows desktop | Interactive startup, input, resize and shutdown remain unverified on Windows. |
| Resource requirement | The 2 GB total-system-RAM benchmark remains deferred until representative launcher workloads exist. |

## Scope and exit criteria

Planning is complete when the four proposals exist, their local references resolve, each stage is reviewable independently and recommendations are visibly separate from approvals. The recorded platform limitations carry forward for validation on Windows. Product capabilities have their own design gates. For example, introduce routing with a real second screen, task ownership with a real operation, and a persistence adapter with an approved schema and writer policy.

Excluded from Phase 02 shell implementation are new navigation UI, configuration persistence, logging sinks, database work, auth or credential selection, game content, process management, updater, dependency or renderer changes, performance-client features and license selection. The 2 GB total physical RAM requirement guides design. Test it on both operating systems once a functional launcher can exercise representative workloads.
