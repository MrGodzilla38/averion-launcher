# Phase 03 · Design system

**Design target and delivery notes.** Phase 03 implements the central palette, system sans-serif, outline glyphs, single version preview, button states and keyboard focus. The profile links directly to Accounts. The compact menu expands inline. The disabled Launch uses a blue outline and subdued fill. The active card has a continuous cyan focus frame, blue rim at rest, a white pixel-font version ID over the artwork. Paired navigation symbols are drawn from Averion’s original glyph strip. Popovers, live status components and backend-dependent actions in the design contract below remain prospective. The visual anchor is a wide terrain scene framed by a single selected-version preview. Blue means “act”, cyan marks the current choice or keyboard focus. Reserve saturated fills for Launch. The current behavior is documented in [UI design](UI-DESIGN.md). Dimensions below are logical pixels at application scale 1.

## Palette and surfaces

| Semantic role | HEX | Use |
|---|---|---|
| Window / secondary background | #0D141D / #101923 | Base behind artwork and page canvas respectively |
| Card / elevated surface | #182634 / #243447 | Content panels and news strip / popovers and raised controls |
| Navigation tint | #243447 at 45% to #101923 at 55% opacity | Over the prepared scene or secondary page background. A one-pixel 23% secondary-text rim separates the glass from its surroundings |
| Hero scrim | #0D141D at 42% starting opacity | Applied once in artwork preparation. Tune by image. The version ID has a dark glyph shadow |
| Primary action / hover / pressed | #579DFF / #78B3FF / #4289E3 | Launch fill, with #101923 label in all three states |
| Cyan / selected surface | #67DEF2 / #18354B | Selected underline, version corner ticks, focus ring / selected navigation fill |
| Secondary hover / pressed | #243447 / #18354B | Quiet button feedback at fixed size |
| Divider / strong boundary | #34475A / #8094AC | Decorative separation / necessary outlines, control boundaries |
| Primary / secondary / muted text | #F1F6FA / #BFCEDB / #A4B6C8 | Titles, body copy, metadata respectively |
| Disabled fill / label | #26303D / #A4B6C8 | Unavailable action plus nearby written reason |
| Success / warning / error | #82DEA9 / #F2C475 / #FF9B9B | Status symbol and short text on dark surface |

**Contrast check.** Calculated from linearized sRGB relative luminance, using (lighter + 0.05) / (darker + 0.05). On the lightest neutral surface #243447, primary, secondary and muted text measure **11.65, 7.88 and 6.10** to 1. Success, warning and error measure **7.83, 7.80 and 6.28**. Dark label against default, hover and pressed blue measures **6.47, 8.18 and 4.99**. Disabled label on disabled fill is **6.42**. Cyan on selected fill is **8.07**. Strong boundary on elevated is **4.07**. Divider is decorative. A cyan ring needs a dark spacer around a bright blue button. For navigation tint, cap the brightest possible composite or use an opaque underlay and measure the actual result. Recheck native renders at smallest text size and worst crop. These calculations cover solid pairs only. [WCAG contrast rationale](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)

## Type, geometry and icons

Use the platform sans-serif resolved by Iced's system font stack. Segoe UI on Windows and a locally installed Noto Sans on Linux are candidate faces if explicit selection becomes necessary. Fall back to generic sans-serif, then the system's glyph fallback. Use monospace sparingly for diagnostic identifiers. No font download. Test long localized names, glyph coverage, weight substitution and enlarged text on each OS.

| Role | Size / line height | Weight |
|---|---|---|
| Selected release or page heading | 30 / 36, compact 24 / 30 | Bold (700) |
| Section heading | 20 / 26 | Semibold (600) |
| Launch label | 18 / 24 | Bold |
| Body / secondary button | 16 / 24 | Regular (400) / semibold |
| Navigation / username | 15 / 20 | Medium (500) / semibold |
| Metadata, source, type and session labels | 14 / 20 | Regular or medium |

Use sentence case. Preserve the exact casing of version IDs and usernames. Space scale **4, 8, 12, 16, 24, 32, 48**. Use 4 for icon/indicator detail, 8 between related labels, 12 between cards, 16 inside panels, 24 between related sections and 32–48 at wide window edges. Radii **4** for small markers, **8** for controls, **12** for cards and **16** for the floating bar. Keep the selected card's two short cyan corner marks square, an Averion cue derived from the scene's stepped horizon. Reserve the stronger outline for state or focus.

Use one 24-unit outline icon family with approximately 2-unit strokes. Render navigation symbols at 18–20 logical pixels beside text. Suggested Lucide names in navigation order: House, Layers, Puzzle, CircleUserRound, Settings, Newspaper, UsersRound. Triangle Play accompanies Launch. Icons communicate function through paired text or an explicit action name. Check final subset and notices before use. Keep the generic avatar square with rounded corners so it reads as a portrait at small sizes. [Icon rights](DESIGN-RESEARCH.md#asset-provenance-and-release-decision)

## Components and interaction contract

| Component | Geometry, state and behavior |
|---|---|
| Top navigation | Floating inset bar, 56 high at standard scale, 8 internal spacing, thin upper edge on tinted surface. Wordmark left, destinations middle, profile right. Increase height for enlarged type. Compact overflow rules in the [screen spec](UI-UX-SPECIFICATION.md). |
| Navigation item | At least 36 high, 8 horizontal padding. Default secondary text. Hover fills a local rounded rect. Press darkens it. Selected uses cyan underline plus selected surface and primary label. Focus adds a separate 2-pixel ring. Planned state pairs muted label with “Planned” in the item or overflow list. |
| Profile card | Quiet secondary control, 32-square portrait, 8 gap, two lines for username and session. Keep full username in popover. Signed-out uses a generic silhouette or initials and written status. |
| Primary Launch | 240 × 56 standard, 200 × 48 compact, minimum sizes that can grow for labels. Centred icon/label, 8 radius, stable hit bounds. Dark text on state-specific blue. Busy states use disabled fill and adjacent status. |
| Secondary button | Content width, 36 minimum height, 12 horizontal padding, strong outline when boundary conveys action. Use for Retry, Details or Back. Same focus and disabled treatment as Launch. |
| Home version preview | One centered landscape button. The artwork and version ID share one hit area. Place the white ID directly over the landscape with a two-pixel dark glyph shadow. Use Craftron Gaming’s Minecraft typeface only for this ID. Hover brightens the rim. A cyan border marks keyboard focus. Activation opens Versions. |
| Announcement strip | One shared surface, two labelled entries, one per source. Source in 14 medium, headline in 16, date optional. One News link when there is a working destination. Use whitespace or one divider between sources. |
| Status indicator | One short phrase plus shape: check for ready, clock for waiting, triangle for warning, exclamation for failure. Color reinforces meaning. Keep status in place as it changes. |
| Tooltip | Secondary help on hover after about 400 ms or immediately on focus. Opaque elevated fill, 8 padding, max 280 wide, wrapped text. Escape dismisses. Necessary explanations stay visible without hovering. |
| Popover | 240–320 wide or available width minus gutters. Align to trigger edge, clamp to window, scroll contents if height is limited. Focus first eligible row, Escape or outside click closes, restore trigger focus. Consume outside click before it reaches controls below. |
| Empty state | In the affected region, one sentence identifying what is absent. Add one available recovery action only if meaningful. Maintain the slot for the version stage or announcement row. |
| Loading state | Retain previous valid content if relevant, show operation and target in writing. Use determinate progress only with a known total. A static waiting label works for unknown totals and reduced motion. |
| Error state | Put brief cause and actionable recovery next to the failed operation. Preserve unaffected regions and user selection. Give detailed diagnostics a secondary disclosure with safe text. |

**Input rules.** Use logical reading order, Tab and Shift+Tab in both directions, Enter/Space on actionable controls and Escape for transient UI. A pointer press cancels when released outside. Prevent double activation while work is pending. Keep labels visible in compact layouts or expose them in the keyboard-operable More list. Disabled controls do not dispatch actions or take a tab stop. Show the reason in normal reading order. Selected, pressed and focused are independent states. Errors do not steal focus on background completion. After explicit navigation, focus the new page heading or first relevant control. Preserve F1 on Home.

**Review conditions.** Test focus visibility over the brightest scene, traversal with More and the profile popover open, long labels, 125–200% scale, distinct text cues with color removed, and zero-motion operation. Verify actual assistive technology behavior on Linux and Windows. Stock button focus and the missing semantic tree are explicit risks in the [feasibility record](ICED-DESIGN-FEASIBILITY.md).

The version label uses the user-supplied Minecraft Medium TrueType face by Craftron Gaming. The supplied archive contains no separate license text. [Font listing](https://www.dafont.com/minecraft.font)
