# Phase 03 · Design decisions for review

## Approved direction

| Area | User decision |
|---|---|
| Platform | One Rust/Cargo desktop app, Iced 0.14.0, tiny-skia, Linux and Windows, Vanilla Minecraft Java Edition first. Keep native window decorations and resize behavior. |
| Identity | Dark, minimal, visually rich gaming interface. Blue/cyan identity. Lunar atmosphere, Feather simplicity and Salwyrr's Minecraft focus inform an original Averion composition. |
| Navigation | Floating, translucent, frost-inspired top navigation. Home, Versions, Mods, Accounts, Settings, News and Social / Friends. Clear selection, pointer and keyboard access. |
| Home | Balanced density. Version-driven scene, one large centered preview linking to Versions, where selection takes place, prominent central Launch, compact upper profile card, target/status and small announcement area. |
| Artwork | Bundled and offline, deterministically selected by version, with a fallback, blur and darkening. Official promotional artwork is the preferred source, conditional on demonstrable redistribution rights. |
| Content and motion | Separate official Minecraft and Averion announcements. Subtle purposeful transitions, accessible operation and responsive reorganization. Planned functions must be identifiable. |
| Phase boundary | The research assignment produced the initial proposal. The subsequent Phase 03 assignment authorizes the seven-page UI foundation, labelled local samples, original bundled artwork and minimal Iced rendering features. Game services and persistence remain deferred. |

## Implemented details

The implementation assignment authorizes the seven destinations and sample data. Phase 03 uses three local release samples, a 1120 × 720 initial window, a 640 × 440 minimum, in-process reduced motion, a compact inline menu and original terrain/images under assets/. These are implementation choices within the approved direction. Official key art remains uncleared. A bundled fallback scene covers unknown version IDs. Iced's optional image-without-codecs feature supplies raw RGBA handles without adding a direct crate. Original icons avoid a third-party asset license obligation. [Current interaction record](UI-DESIGN.md)

## Research recommendations

Use a slate scene with a sharp landscape card and a version number over the artwork. Blue fill is reserved for Launch, cyan marks selection and keyboard focus. Try 1120 × 720 initial and 640 × 440 minimum logical client size. Keep seven destinations discoverable using a labelled More menu. Use the system sans-serif, one outline icon family, a keyboard-accessible version preview, original rights-cleared family scenery and a static fallback. Prefer preprocessing over runtime blur, with a short fade only if measured performance allows it. The exact tokens, budgets, dimensions, labels and motion durations are proposals. [Design system](DESIGN-SYSTEM.md) · [Screen specification](UI-UX-SPECIFICATION.md)

## Open decisions, evidence and release gates

| Topic | Status and practical next decision |
|---|---|
| Asset rights | No verified grant for bundling official key art. Obtain asset-specific terms covering derivatives and downstream distribution, before replacing the delivered original terrain. Commissioned art would need its own rights record. |
| Asset rendering | The authorized image feature is enabled without codecs. SVG, Canvas and advanced widgets remain disabled. Review compressed formats if the asset collection grows. |
| Minimum size and animation | Verify effective work area, large text, translation, reachable Launch and focus at the proposed minimum. The delivered fade uses temporary frame subscriptions. Cross-platform resource and reduced-motion validation remains open. |
| Accessibility | Keyboard control requires explicit focus modeling for buttons. Screen-reader exposure and OS reduced-motion detection are separate platform questions. Include users of assistive technology in later validation. |
| Content scope | Versions requires a trusted real catalog and separately approved snapshot support. Accounts, News and Social require approved data sources and policies. Mods stays future-facing under the Vanilla scope. |
| Existing open choices | Authentication, secure storage, database, configuration format, project license and release packaging retain their existing decision owners. No selection is made here. [Existing decisions required](DECISIONS-REQUIRED.md) |

The mint startup screen was the Phase 02 baseline. The [current UI](UI-DESIGN.md) implements seven destinations. Phase 02's architectural growth map identifies possible internal responsibilities, including installations and downloads; it does not require seven new backend modules or extra top-level tabs. [Architecture proposal](APP-ARCHITECTURE.md)

**Review package:** [Reference observations and rights](DESIGN-RESEARCH.md) · [System and contrast](DESIGN-SYSTEM.md) · [Screens and interaction](UI-UX-SPECIFICATION.md) · [Iced feasibility and verification plan](ICED-DESIGN-FEASIBILITY.md).
