# Mayasaba Control Room — UI design system (implementation record)

Status: implementation record for the WinUI 3 Control Room shell under `app/Mayasaba.App`.
It is derived from `UI-UX.md` (authoritative) and `AGENTS.md` section 5; it records how the
design system is implemented and does not redefine any requirement. Where this document and
`UI-UX.md` differ, `UI-UX.md` wins and the difference is a defect here.

Scope: design tokens (colour, type, spacing, shape, motion), the timeline card style set, the
original Mayasaba mark, the icon pipeline, and the accessibility matrix. Runtime behaviour
(view-models, controller bridge, focus/reading-position rules) is documented in the source
headers next to the code.

## File map

| Path | Contents |
| --- | --- |
| `Theme/Tokens.xaml` | Colour brushes in `ThemeDictionaries` (Light / Dark / HighContrast), typography families and sizes, spacing, shape, motion durations. Single source of truth for tokens. |
| `Theme/Styles.xaml` | Type-ramp styles, the 13-kind card style set, containers, buttons, composer and sheet styles, and the mark geometry (`MayasabaMarkLines`, `MayasabaMarkNode`). |
| `Controls/LogoMark.xaml(.h/.cpp)` | The original mark as a `UserControl` (Viewbox over the 64×64 geometry). |
| `Chat/ChatPage.xaml(.h/.cpp)` | Shell layout: header, attention strip, timeline `DataTemplate`, composer, empty/loading/error states, details sheet. Thin code-behind. |
| `Chat/ChatViewModel.h`, `Chat/TimelineItemViewModel.h`, `Chat/ComposerViewModel.h` | Plain C++ view-models and projection rules (no XAML, no SQL, no process launch). |
| `Chat/ControllerBridge.h`, `Chat/ControllerSession.h/.cpp` | The only place the UI touches `mayasaba::control::Controller` (typed commands/queries; notice callback marshalled to the UI thread). |
| `Chat/BindableItems.h/.cpp` | Factories from plain view-models to the bindable WinRT items (`TimelineCard`, `AgentNotice`, `AttachmentChip`, `CardActionView`). |
| `Chat/DetailsSheetBuilder.h/.cpp` | Builds the details-sheet body from an already-authoritative `DetailsViewModel`. |
| `assets/mayasaba-mark.svg`, `assets/mayasaba-appicon.svg` | Canonical vector sources of the mark and the application icon. |
| `assets/generate-icon.ps1` | Rasterises the same geometry with `System.Drawing` into `assets/mayasaba.ico` + reference PNGs. |
| `assets/mayasaba.ico`, `assets/icons/mayasaba-<size>.png` | Generated icon artifacts (checked in). |

## 1. Information architecture (as implemented)

- Chat is the only persistent workspace and the only top-level route (UI-UX.md 3). `MainWindow`
  hosts `ChatPage` directly — no navigation rail, tabs, dashboard or standalone Council /
  Requirements / Decisions / Tasks / Files / Validation / Delivery / Settings / Agents pages.
- `Open Folder` sits in the lower-left of the Chat shell (`ChatFooter`) and is visually distinct
  from `Attach files`, which lives in the composer.
- Council, requirements, decisions, tasks, files/evidence, validation/repair, delivery and
  diagnostics surface only as timeline cards. Every card with extra information carries a
  **Show details** action that opens a temporary dismissible sheet over Chat (never a route);
  closing restores Chat focus and reading position. Diagnostics is an overflow-menu action that
  opens the same sheet.
- Agent CLI problems appear only as exception-only attention rows (missing / unusable CLIs).
  Healthy READY agents never occupy a row (UI-UX.md 6.1; AGENTS.md section 5). The
  agent-executable-path setting is a contextual action (`SetAgentExecutablePath`) on that
  warning row — there is no settings page.
- Send is gated only on project binding (`ProjectState.bound`). After binding, every Send
  persists exactly one contribution even with no CLI available. First and later messages use
  the same composer and send path.

## 2. Layout metrics

| Token | Value | Use |
| --- | --- | --- |
| `ConversationMaxWidth` | 760 DIP | Reading measure of the conversation column and composer. |
| `CardMaxWidth` | 900 DIP | Controller cards may exceed the measure for structured content. |
| `ComposerMaxWidth` | 760 DIP | Composer column. |
| initial window | 1120 × 800 DIP | `MainWindow.xaml.cpp` (`AppWindow().Resize`). |
| details sheet | max 880 × 640 DIP, bottom-centred | `SheetSurfaceStyle`. |

All sizes are device-independent pixels; XAML scales them with the display DPI.

## 3. Colour tokens

Defined in `Theme/Tokens.xaml` as `SolidColorBrush` resources inside `ThemeDictionaries`
(`Light` is the default theme; `Dark` and `HighContrast` are first-class). Every consumer uses
`{ThemeResource}` so theme switches repaint automatically.

### Semantic families

| Family | Meaning | Light | Dark |
| --- | --- | --- | --- |
| Authority / AuthoritySoft | Controller provenance; records the controller owns (decisions, council, requirements) | `#4057C8` / `#E8ECFB` | `#8FA2FF` / `#2A3358` |
| Attention / AttentionSoft | Waiting on the user; degraded-but-working | `#A65D00` / `#FBF1E2` | `#E0A24A` / `#3B2E17` |
| Critical / CriticalSoft | Blocking failure | `#B4232C` / `#FBE9EA` | `#FF8A8A` / `#3F1F22` |
| Success / SuccessSoft | Certification / delivery only — never "the agent said it is done" | `#1F6F4A` / `#E7F4ED` | `#6FD3A0` / `#17301F` |
| Provisional / ProvisionalSoft | Streaming or proposed content explicitly labelled as not authoritative | `#6A4E9C` / `#F0ECF8` | `#C3A9F0` / `#2A2338` |

### Surfaces and ink

| Token | Light | Dark |
| --- | --- | --- |
| `CanvasBrush` | `#F7F8FA` | `#1B1D21` |
| `SurfaceBrush` | `#FFFFFF` | `#26282D` |
| `SurfaceAltBrush` | `#F1F3F6` | `#2E3137` |
| `InkBrush` | `#202124` | `#F2F3F5` |
| `MutedBrush` | `#62666D` | `#A9ADB4` |
| `BorderBrush` | `#E2E5EA` | `#3A3D44` |
| `SeparatorBrush` | `#EDEFF2` | `#33363C` |
| `FocusBrush` | `#4057C8` | `#8FA2FF` |
| `ScrimBrush` | `#66000000` | `#99000000` |

### High contrast

`HighContrast` maps hue onto system colours (`SystemColorWindowColor`,
`SystemColorWindowTextColor`, `SystemColorGrayTextColor`, `SystemColorHighlightColor`).
Nothing depends on the indigo/green/red hues being distinguishable: every card also carries a
kind label, a decorative glyph, a state word and body text (see section 7 and the
accessibility matrix in section 10).

## 4. Typography

| Token | Value | Use |
| --- | --- | --- |
| `FontUIFamily` | Segoe UI Variable Text | Interface and conversation text. |
| `FontDisplayFamily` | Segoe UI Variable Display | Empty-state and sheet display text. |
| `FontMonoFamily` | Cascadia Mono | Code, paths, hashes, command fragments, machine-readable record. |

| Size token | Value | Line height |
| --- | --- | --- |
| `FontSizeCaption` | 12 | `LineHeightTight` 16 |
| `FontSizeBody` | 14 | `LineHeightBody` 20 |
| `FontSizeBodyLarge` | 16 | 22 |
| `FontSizeSubtitle` | 20 | — |
| `FontSizeTitle` | 28 | — |

Style ramp (`Theme/Styles.xaml`): `TextDisplay`, `TextSubtitle`, `TextBody`, `TextBodyLarge`,
`TextMeta`, `TextKindLabel` (small-caps label, character spacing 60, Authority ink),
`TextCardTitle`, `TextMono`, `TextSectionHeader`. Body and mono styles enable text selection.

## 5. Spacing and shape

- 4 px base scale: `SpaceXXS` 2, `SpaceXS` 4, `SpaceS` 8, `SpaceM` 12, `SpaceL` 16,
  `SpaceXL` 24, `SpaceXXL` 32.
- Pads: `PadCard` 16, `PadCardCompact` 12, `PadShell` 16,10,16,10, `PadComposer` 12.
- Radii: `RadiusS` 4, `RadiusM` 8, `RadiusL` 12, `RadiusPill` 999.
- Strokes: `SpineWidth` 3 (evidence spine), `HairlineWidth` 1 (borders),
  `FocusStrokeWidth` 2 (focus visuals).

## 6. Motion

| Token | Value (ms) | Use |
| --- | --- | --- |
| `MotionFastMs` | 120 | Card insertion/removal (ListView item transitions). |
| `MotionMediumMs` | 200 | Details-sheet fade-in. |
| `MotionSlowMs` | 320 | Reserved for larger surface transitions. |

Reduced motion: `ChatPage::ApplyReducedMotionPreference` reads
`UISettings.AnimationsEnabled` once at page load. When the user has reduced motion enabled,
the timeline's item-container transitions are cleared and the sheet appears directly at its
final state (the storyboard is skipped). Motion is never required to understand state
(UI-UX.md 18).

## 7. Timeline card style set

The card kinds are the stable vocabulary of `control::TimelineItem.kind` (UI-UX.md 7.1).
`mayasaba::app::StyleKeyForKind` maps kind → style key in `Theme/Styles.xaml`; an unknown kind
degrades to `Card_Default` — records are never dropped.

| Kind | Style key | Surface / border family | Kind label | Glyph (decorative) |
| --- | --- | --- | --- | --- |
| `user_message` | `Card_UserMessage` | Authority soft, right-aligned bubble | "You" | — |
| `agent_message` | `Card_AgentMessage` | Surface, left | "Agent" | — |
| `status` | `Card_Status` | Surface | "Status" | Segoe Fluent |
| `warning` | `Card_Warning` | Attention | "Attention" | Segoe Fluent |
| `question` | `Card_Question` | Authority | "Question" | Segoe Fluent |
| `progress` | `Card_Progress` | Surface | "In progress" | Segoe Fluent |
| `council_summary` | `Card_CouncilSummary` | Authority | "Council" | Segoe Fluent |
| `requirements_summary` | `Card_RequirementsSummary` | Authority | "Requirements" | Segoe Fluent |
| `decision` | `Card_Decision` | Authority | "Decision" | Segoe Fluent |
| `task` | `Card_Task` | Surface | "Task" | Segoe Fluent |
| `validation` | `Card_Validation` | Surface | "Validation" | Segoe Fluent |
| `delivery` | `Card_Delivery` | Success | "Delivery" | Segoe Fluent |
| `error` | `Card_Error` | Critical | "Error" | Segoe Fluent |
| any other | `Card_Default` | Surface (quiet neutral) | "Update" | — |

Two container bases: `BubbleBase` for conversational entries (max 640, radius L) and
`CardBase` for controller cards (3 px evidence spine, kind-label row, action footer).

Severity spine: `severity ∈ {info, attention, error, success}` selects which of the four spine
elements is visible (the `Spine*Visibility` properties computed in `BindableItems.cpp`).
Severity is independent of kind — e.g. a validation card can carry success or error.

Rules:

- Colour, glyph and spine are redundant reinforcement. The kind label, state word and body
  text always carry the meaning on their own (required for High Contrast).
- `is_provisional` cards show an explicit "not yet authoritative" state word; provisional
  styling never replaces the label.
- Action buttons in the footer come from the controller (`control::CardAction`); a card that
  has more information always offers **Show details**.

## 8. Shell components

- Header: project name and canonical path (with a copy action), lifecycle phase and condition
  text; overflow menu (Open Folder, Copy path, Enter-sends toggle, Technical diagnostics).
- Attention strip: exception-only CLI warning rows with `Locate executable` and `Recheck`
  actions (mapped to `SetAgentExecutablePath` / `RetryAgentProbe`).
- Timeline: `ListView` with the 13-kind template, bounded in-memory window, jump-to-latest
  pill when the user has scrolled away from the newest item.
- Composer: multiline `TextBox` (max height 168), `Attach`, `Send` (accent), `Stop` while an
  operation is active, Enter-sends toggle, attachment chips with remove.
- Empty state: centred explanation plus `Open Folder` call to action; loading state with a
  progress ring; shell error card (title + exact reason) for controller failures.
- Details sheet: scrim + bottom-centred surface (max 880 × 640), title / card-id / subtitle,
  scrollable sections, collapsed machine-readable record, `Close` and Escape; on close, focus
  returns to the invoking control and the previously selected card is restored.

## 9. The original mark — "Three Sightlines, One Node"

Concept: Mayasaba owns one project reality — the filled node — observed by three independent
coding CLIs from three directions (three unequal, gapped sightlines); the node stands on the
plumb baseline of the published project root. Deliberately not a sparkle, robot, chat bubble,
hexagon or symmetric network hub.

Geometry on a 64 × 64 design grid — identical numbers in the XAML geometry, the SVGs and the
icon script:

| Element | Coordinates |
| --- | --- |
| Sightline (upper-left observer) | (9, 10) → (26.5, 24.5) |
| Sightline (upper-right observer) | (55, 10) → (37.5, 24.5) |
| Sightline (lower observer) | (32, 58) → (32, 37.5) |
| Plumb baseline (published root) | (22, 58) → (42, 58) |
| Node (verified project reality) | circle centre (32, 30), r 5.2 |

Stroke width 4.5, round caps and joins.

Rendering surfaces:

- In-app: `MayasabaMarkLines` (`PathGeometry`) and `MayasabaMarkNode` (`EllipseGeometry`) in
  `Theme/Styles.xaml`; `Controls/LogoMark.xaml` wraps them in a `Viewbox`, so the mark is
  resolution-independent and crisp from 16 px to 256 px.
- Title bar: the window extends content into the title bar (`ExtendsContentIntoTitleBar`) and
  places the mark in the title area.
- SVG sources: `assets/mayasaba-mark.svg` (mark on transparent) and
  `assets/mayasaba-appicon.svg` (mark reversed out of the Authority-indigo rounded plate).
- Application icon: `assets/generate-icon.ps1` rasterises the same geometry with
  `System.Drawing` (no network, no external tools, no NuGet) into a multi-resolution
  `mayasaba.ico` — frames 16/20/24/32/40/48/64/128/256, PNG-compressed — plus one reference
  PNG per size under `assets/icons/`. The `.ico` is embedded by `app/Mayasaba.App/app.rc` and
  copied next to the executable; `MainWindow.xaml.cpp` calls `AppWindow.SetIcon` with it, so
  taskbar, Alt-Tab and Explorer identities match the in-app mark.
- The mark is decorative (`AutomationProperties.AccessibilityView="Raw"`); the adjacent
  wordmark carries the meaning for assistive technology.

## 10. Accessibility matrix

| Area | Commitment | Where implemented |
| --- | --- | --- |
| Keyboard | Every interactive element is reachable; Enter sends (per the Enter-sends toggle), Shift+Enter inserts a newline, Escape closes the details sheet. | `ChatPage.xaml` keyboard wiring, `OnComposerKeyDown`, `OnRootKeyDown`. |
| Visible focus | System focus visuals on all custom buttons; 2 DIP focus stroke token. | `UseSystemFocusVisuals` on button styles; `FocusStrokeWidth`. |
| Screen readers | `AutomationProperties.Name` on interactive elements, cards and the timeline (names come from controller-derived data); decorative glyphs and the mark are `AccessibilityView="Raw"`. | `ChatPage.xaml` (44 automation properties), `BindableItems.cpp` (`AccessibilityName`). |
| High contrast | Dedicated `HighContrast` theme dictionary using `SystemColor*` brushes; no meaning depends on hue. | `Theme/Tokens.xaml`. |
| DPI scaling | All sizes are DIPs; the mark is vector; icon frames cover common DPI steps. | Tokens + Viewbox + icon frames. |
| Reduced motion | Honoured at page load; transitions cleared, sheet appears at final state. | `ApplyReducedMotionPreference`, `OpenDetailsSheet`. |
| Colour independence | Kind label + glyph + state word + text always accompany the spine colour. | Card templates + `KindLabelForKind`/`GlyphForKind`. |
| Privacy | The UI renders only controller-query results; it never shows chain-of-thought, credentials, tokens or unrelated process output. The diagnostics sheet states what it does not show. | `ChatPage.xaml.cpp` (`ShowDiagnosticsSheet`), view-model projections. |

## 11. Verification of this document

As of 2026-10-10, in this staging tree (`app/Mayasaba.App`, Debug x64):

- `cmake --preset vs2026` and `cmake --build --preset vs2026-debug` (repository root): exit 0;
  `mayasaba_core.lib`, `mayasaba_adapters.lib`, `mayasaba_sqlite3.lib` built.
- MSBuild build of `Mayasaba.App.vcxproj`: all C++ sources, XAML compilation
  (`Theme/Tokens.xbf`, `Theme/Styles.xbf`, `Controls/LogoMark.xbf`, `Chat/ChatPage.xbf`) and
  resources compile with zero errors. The link resolves every symbol except
  `mayasaba::control::Controller::Create` (the controller facade is implemented in the
  canonical repository, not in this staging copy).
- `assets/generate-icon.ps1` produced `assets/mayasaba.ico` (9 frames) and
  `assets/icons/mayasaba-{16,20,24,32,40,48,64,128,256}.png`.
