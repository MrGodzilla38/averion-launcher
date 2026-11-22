# Phase 03 · Desktop interaction specification

**Scope and delivery.** The product specification below describes the target experience, including future service-backed flows. Phase 03 delivers seven navigable UI foundations, the native frame and top bar, a single selected-version preview, bundled offline covers, disabled Launch, profile entry, both news empty states, session-only reduced motion and F1 disclosure. On narrow windows a labelled menu expands inline. The Home artwork and top Versions item both open the same nine-family gallery. Version discovery, authentication, switching, game operations, news feeds, Mods and Friends services remain deferred. The [implemented UI](UI-DESIGN.md) records exact behavior. [Implemented shell](APP-ARCHITECTURE.md) · [Tokens and component states](DESIGN-SYSTEM.md)

## Opening the window

Keep the OS frame, title, drag region, system menu, resizing and window controls. The top navigation begins inside the client area with a margin on every side. The small gap above the bar lets the scene show through and clearly separates application navigation from window chrome.

Phase 03 opens at **1120 × 720 logical client pixels** and requests a **640 × 440** minimum. Phase 02 used 700 × 420 and 360 × 300. The larger opening fits a 56-high navigation row, 24-pixel gutters, a prominent version preview, Launch and status, and two short announcements. The compact card occupies 320 × 160 logical pixels. Remaining width covers labels and enlarged controls. A 440-high window can show compact navigation, artwork, Launch and its immediate status at normal type size, with news below the fold. The window manager determines placement. High-DPI and constrained work-area testing remains open. If native tiling yields less space, allow further vertical reflow.

## Home composition

| Reading order | Placement and content |
|---|---|
| 1 · Top bar | Averion wordmark at left. Home, Versions, Mods, Settings, News, Social / Friends across the available center. The Profile control at right opens the account screen. Each destination pairs a labelled outline icon with text, with underlined current location. Friends is an acceptable short label for Social / Friends. |
| 2 · Scene and version stage | The version's terrain fills the client background, cropped to cover with a per-image focal point. Place the horizon in the upper third, outside caption and button regions. A sharp, smaller crop inside the selected card gives the blurred background context. No additional hero slogan or duplicate title. |
| 3 · Launch and status | One centered Launch control directly below selection, 16 gap. Beneath it, one line for the operation and target, expanding for a genuine error. Place any recovery action here. Keep Startup details (F1) as a quiet disclosure. |
| 4 · Announcements | A short strip beneath the action area. One Minecraft item and one Averion item, each identified in text. Put additional items on News. Never rotate automatically. |

**Availability.** In the Phase 02 baseline only Home and its disclosure worked. In a future shell preview, show planned destinations by name with a visible planned explanation or list them under More with an explicit status. An available destination may contain a disabled action if its prerequisite is missing. Example: Versions can browse a real catalog while Launch is unavailable. Reserve “Ready” for validated launch prerequisites. The implementation brief permits labelled, bundled sample versions on the Versions page. Account identities and news headlines require genuine data.

## Version selection

Order a validated catalog by release timestamp descending with a stable version-ID tie break. Initially show supported Vanilla releases. Snapshots gain a text type label and an explicit Versions filter only if support is approved. Match exact IDs including punctuation. Selecting an item updates the focused Home card, artwork lookup and launch target together. Selection alone never starts a game. An already running job remains bound to its original target. Preserve a valid existing selection on catalog refresh. If it disappears, clear Launch eligibility and ask the user to select an available version.

The Home preview is a single button with artwork and the version ID over a dark gradient that fades upward from the bottom edge. Click anywhere inside it or use Tab and Enter/Space. Both this card and the top Versions link open the gallery with the current family focused. Activate a family card to open its release picker. Select a specific release to update the preview and return Home. Escape closes the picker and restores focus to the family card. Returning without choosing preserves the previous ID. Arrow keys on Home leave selection unchanged.

## Launch workflow

Keep the same footprint and location through each state. A single activation captures selected version and account reference and admits one attempt. Show progress beside the button where it can wrap without moving the target. Future messages must refer to that attempt. A late completion from a prior attempt cannot enable Launch.

| State | Button and supporting information |
|---|---|
| Backend unavailable / no selection | Disabled “Launch”. Explain “Game launching is planned” in a preview, or “Select a version” when the catalog works. |
| Account or runtime prerequisite missing | Disabled “Launch” and the specific reason. Offer a working Accounts or runtime remedy only when available. Unknown ownership never enables Launch. |
| Ready | Blue “Launch”, triangle icon, status identifies selected Java Edition version. Hover lightens, press darkens, keyboard ring stays distinct. |
| Preparing | Immediately show “Preparing…” and target. Disable duplicate activation while resolving or verifying prerequisites. |
| Downloading | “Downloading…” plus bytes or percentage when total is trusted. Unknown total shows bytes received or stage text. Expose cancellation only if cleanup is supported. |
| Starting / running | “Launching…” while starting, “Running” only after successful process start. Show the bound game version. A normal exit returns to eligibility evaluation. |
| Failed | Show stage-specific, safe reason. If recoverable, “Retry launch” rechecks prerequisites. For missing entitlement or invalid content, explain the blocking condition and show the relevant remedy. Keep version selection visible. |

## Profile and announcements

**Profile.** At upper right use a 32-square portrait, username and small session line. Cap the trigger around 160 wide. If privacy or missing data precludes an avatar, show a neutral icon or initials. A first-time profile reads “No account”; a previously known expired session reads “Signed out” or “Sign-in required” only when established. For a known valid session show its verified display name and status. Once accounts work, the popover lists eligible accounts, puts a check beside the active one and offers Manage accounts. Up/Down or Tab traverses choices, Enter selects, Escape restores trigger focus. Switch only after success. Failure retains the prior active account and gives a safe reason. During a bound launch, defer a conflicting switch or explain the lock. Account type and full name fit in the expanded view. No credentials or authentication protocol appear in the design.

**Announcements.** Keep the last available headline per source, ordered by its actual publication date. Include the written source label even when only one item exists. News navigation can open with that source selected. Each source resolves independently: waiting label while loading, “No Minecraft news available” or “No Averion announcements” for known emptiness, and a short source-specific unavailable message on failure. Previously verified content may remain with its date and a stale notice. If no feed exists, explain the planned integration once. Headlines should wrap to two lines on Home, with full text on News. Avoid identical empty cards, invented lead stories, stock timestamps or a spinner that implies retrieval has begun.

## Destination behavior

| Destination | Layout and interaction | Availability gate |
|---|---|---|
| Home | Composition above. Preserve selection and in-flight status when returning. | Local sample presentation and startup disclosure work. Game workflow is planned. |
| Versions | Heading, search by version ID, approved type filters, then rows with version/type and selected mark. Keep selected item visible on return. Tab between sample rows. Click, Enter or Space selects the focused row and returns to Home. Distinguish catalog entries from verified local installations. Include clear-filter, empty, error and retry states. | The foundation uses a labelled local card gallery with nine family cards and a dialog for each family’s stable releases. Discovery, filters and installation status need a real catalog. |
| Mods | Future page would group compatible installed items by version, with item details and real availability status. Until scope exists, muted planned entry with explanation that the first release targets Vanilla. | No mod loader, installation, marketplace or toggle promised. |
| Accounts | A list of real public account identities, active marker and session status. Details panel for the selected row, stacked below on compact windows. A signed-out user has a single next action once sign-in exists. No credentials in UI state or diagnostics. | Authentication and vault decisions remain open. |
| Settings | Single-column labelled groups for Appearance/accessibility, Game/runtime and About, showing only supported values. About can surface existing startup facts. Distinguish drafts from committed choices and show write errors next to the affected setting. | Categories are concepts. Controls require a real backing setting. Storage format remains open. |
| News | Source filters All / Minecraft / Averion. Dated text list in descending publication order, labelled source per row, explicit “Open original” for verified external links. Preserve source filter on return. Each source retains its own loading, stale, empty and error state. | Sources, terms, publication process and retrieval are unverified integrations. |
| Social / Friends | Future list grouped by actual presence when known, name and status in text; profile details in place. Show a single planned explanation until a service exists. | Identity, consent, privacy, presence and service have no approved design. Never show fake online counts. |

## Resizing and navigation overflow

| Available logical width | Layout |
|---|---|
| ≥1280 | Center content on a maximum 1280-wide stage. 32–48 outer gutters. Full labels and profile when measured widths fit. Keep the 440 × 220 preview centered. Both source entries occupy one strip. |
| 960–1279 | 24 gutters. Use the regular or compact card according to the shell layout. Keep Home and Versions visible. Move lowest-priority destinations to labelled More before any label is clipped, beginning with planned items. Profile can collapse its second line first. |
| 640–959 | 16 gutters, compact wordmark, Home, Versions, More and a 36-wide avatar trigger at right with explicit focus help. Move additional destinations into More in their canonical order. Use a centered 320 × 160 card. Stack source entries below Launch. |
| Short height / enlarged text | Single vertical content scroll below navigation. Reduce stage spacing first. Let news move below the fold. Retain normal type and control hit sizes. With less effective width, keep Home/current route, More and profile, collapse wordmark to “A”. Allow full labels in More and return focus there if an active item moves inside it. |

Use intrinsic content width at actual text scale to trigger overflow earlier than nominal bands. Mark More as containing the active route when appropriate. On width change preserve selection, job and reading position. Fit popovers inside the visible area and keep focused controls scrolled into view. Do not horizontally scroll the entire window. No control may rely solely on hover, drag or a hidden shortcut.

## Artwork delivery proposal

**Delivered assets.** Each of the nine update families provides a 320 × 180 cover and a 256 × 144 preblurred, darkened variant. Stable shared Iced handles reuse both images across patch releases. The selected release uses its family cover on Home and its blurred variant across all pages. [Artwork sources](../assets/version-covers/README.md)

Future bundle layout: assets/artwork/backgrounds/<family>-bg.jpg for pretreated scenery, assets/artwork/cards/<family>-card.jpg for sharp crops, and a cleared fallback pair under the same folders. Use stable lowercase ASCII slugs independent of remote display strings. Keep a small reviewed mapping with exact ID overrides, explicit version-ID-to-family associations and one universal fallback. Resolve in that order. A patch can share its family's artwork. Unknown releases, future numbering schemes and unassigned snapshots get the fallback. Never interpolate external version IDs as paths or infer a snapshot family from string order.

For each cleared master, propose a 1600 × 900 landscape export and a 640 × 360 card crop. Use JPEG for opaque painted scenes, PNG for small transparency-dependent assets if a decoder is approved. Crop and soften at production time (start with 8–12 image pixels blur at 1600 width), then apply dark tint. Keep the center band quiet; protect critical text with opaque controls. Store focal point per image for wide/narrow crops. Rights must cover these derivatives. Review a crisp and a softened export side by side before selection.

Start with a compressed budget of 350 KiB per background and 90 KiB per card. Five families plus fallback would contribute about 2.58 MiB before packaging overhead at those targets. Strip metadata and visually inspect gradients and banding. Use fewer shared families before lowering text readability or image quality. Load the fallback or solid surface immediately, request only the current family and the visible thumbnails, keep at most two full decoded backgrounds during a transition, and release superseded work. Missing, corrupt, unlicensed or oversized images yield the checked fallback, then solid background. All resolution is local. Artwork rights, packaging and decoder feature approval remain gates. [Provenance](DESIGN-RESEARCH.md#asset-provenance-and-release-decision) · [Image cost and renderer limits](ICED-DESIGN-FEASIBILITY.md)

## Motion and review

| Interaction | Proposed maximum normal duration and fallback |
|---|---|
| Hover, press, nav | Hover tint 80 ms, press immediate with 60 ms release, active underline 100 ms. No change to bounds. |
| Background | 180 ms fade of the new prepared image over the previous one when both are ready and the frame budget allows. Same family incurs no transition. Static swap is the fallback. |
| Page | 100 ms local header/selection emphasis. Change content immediately; avoid fading two whole complex pages. |
| Reduced motion | All durations zero. Text, outlines, progress and selection remain sufficient. Stop frame requests at rest, on interruption and when the view closes. |

Later acceptance walkthrough: resize with the profile open, navigate Home → Versions → Home by keyboard, change version rapidly across an artwork boundary, lose a selected catalog entry on refresh, and test one failed news source while the other succeeds. Confirm Launch and its reason remain reachable at minimum size and enlarged text, and that unavailable features issue no work. These are review criteria for future implementation.
