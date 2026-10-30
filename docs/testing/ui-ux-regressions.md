# UI/UX regression coverage and review checklist

Playwright UI specs run against the existing Vite preview. They inject synthetic Pinia state only; no serial hardware, flash, erase, or authorization action is performed. Defect-target assertions are ordinary assertions of the required behavior; the original failing baseline is preserved below for comparison with repairs.

## Automated coverage map

| ID     | Automated test                                                                   | Current target                                                                                                                                                                                                                                                                                                                                             |
| ------ | -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| UI-01  | `serial-debug-layout.spec.ts` — English 1024×680 controls, no port and mock port | Connection fields and action controls have no intersecting bounding rectangles.                                                                                                                                                                                                                                                                            |
| UI-02  | `serial-debug-layout.spec.ts` — firmware Browse discoverability                  | Default viewport hit-tests to Browse and clicking opens the browser file chooser. If product chooses a visible horizontal-scroll cue instead, replace the direct-visibility assertion with a test for the cue and successful scroll-to-Browse; record that acceptance decision here. Do not report the present default view as passing before that change. |
| UI-03  | `serial-debug-layout.spec.ts` — modal focus                                      | Independent tests cover focus entering, Tab wrap, Shift+Tab wrap, Escape close/restore, and close-button restore.                                                                                                                                                                                                                                          |
| UI-05  | `serial-debug-layout.spec.ts` — erase confirmation | Cancel receives initial focus; Tab wraps and Escape restores the opener. |
| UI-06  | `serial-debug-layout.spec.ts` — batch workflow | Start and Port Filter remain visible at 1024×680; filter focus and field names work; missing firmware blocks the configured flash mode, and supplying it enables Start. |
| UI-07  | `serial-debug-layout.spec.ts` — invalid Hex draft | Sending invalid Hex preserves the input and shows an inline error. Store/component tests cover failed sends, pending duplicates, edited drafts and composition Enter. |
| UI-08  | `serial-debug-layout.spec.ts` — operation prerequisites | Disabled reasons appear beside operation buttons; malformed credentials have associated inline errors; correcting values enables authorization. |
| UI-09  | `serial-debug-layout.spec.ts` — primary text contrast | Light/dark enabled flash buttons have at least 4.5:1 text contrast at both gradient endpoints, including the hover brightness filter. |
| LOG-01 | `serial-log-regressions.spec.ts` — virtual row geometry                          | Alternating real blank/nonblank store lines, two desktop sizes, all timestamp/badge combinations, font sizes 10/12/18; actual `appendChunk` newline splitting; probe/row alignment, bottom gap, middle scroll, tail return, normal-line control, and cached-page return.                                                                                   |
| LOG-02 | `serial-log-regressions.spec.ts` — display-toggle state                          | `aria-pressed` follows timestamp/direction toggles independently.                                                                                                                                                                                                                                                                                          |
| LOG-03 | `serial-log-regressions.spec.ts` — proposal test                                 | Prefix remains in pane while horizontally scrolling. This is a proposed experience improvement, not an approved design requirement; confirm the fixed-prefix interaction with product/design before treating this assertion as a release gate.                                                                                                             |
| LOG-04 | `serial-log-regressions.spec.ts` — search behavior guard                         | Query keeps prev/next and whole-session search available; persistent filter-chip creation remains distinguishable in the DOM. Label wording is evaluated manually below.                                                                                                                                                                                   |
| LOG-05 | Manual checklist below                                                           | Timestamp and direction marker readability at real display scale; metadata minimum sizes preserve the existing LOG-01 row geometry. |
| UI-04  | Manual checklist below                                                           | Update Center copy usefulness. No exact sentence lock-in.                                                                                                                                                                                                                                                                                                  |

Default locator/expect polling timeouts are at most 3 seconds; each spec test has a 15-second ceiling. Specs use `.spec.ts` so Vitest does not collect them.

## Manual acceptance items

### LOG-04 — temporary find versus persistent filter labels

1. Open Serial Debug at 1024×680 and 1280×800 in Chinese and English.
2. Identify the toolbar action that opens transient find, the search input and match navigation, the “search whole session” action, and the `+` action that creates a persistent filter tab.
3. Ask a first-time user to predict which action persists after closing the search bar. **Pass:** labels and grouping clearly distinguish temporary find from a saved filter; “whole session” is discoverable after entering a query. No claim is made that current search misses session data.
4. Existing interaction guard is automated as LOG-04; final wording remains a product decision.

### LOG-05 — timestamp/direction text readability

1. Use representative RX, TX, and SYS lines at 1024×680 and 1280×800, with light and dark themes.
2. Check default 12px body text, then use the existing 10–18px font controls. Judge whether timestamps and RX/TX/SYS markers can be read quickly without competing with payload text.
3. The October 30 review approved minimum metadata sizes: timestamps use at least 11px and direction markers at least 10px at the default root size, while scaling with larger log text. Record OS/display scaling when reviewing readability. Synthetic browser checks do not replace real-display or screen-reader acceptance.

### LOG-03 — inspecting and copying long log lines (manual proposal review)

1. Inject or capture a long JSON/protocol line whose identifying fields are near the end, at 1024×680 and 1280×800.
2. Verify a user can reach and read the final fields, then copy the complete line and confirm the copied text includes those fields.
3. Horizontal scrolling that exposes the full line or a details interaction that presents/copies the full line are both acceptable approaches. The implementation is not prescribed; review readability, discoverability and copy completeness before choosing one.

### UI-04 — Update Center explanation

1. Open Settings in English and read the Update Center subtitle without reading surrounding implementation notes.
2. Ask a user what actions/settings this section offers. **Pass:** they can identify checking for updates, viewing release notes and configuring background-check cadence from the copy.
3. Review Chinese localization for equivalent user-facing meaning. Do not pin an exact sentence; copy remains editorially adjustable.

## Existing behavior protected by current coverage/source contracts

- Log rendering uses fixed-height virtual rows and overscan. Do not enable wrapping unless the virtualizer is redesigned for measured variable-height rows.
- Font size adjustment, timestamp/direction toggles, substring find, prev/next navigation, whole-session filter action, per-tab pause/follow, selected-text actions, and Hex rendering already exist. These are not new feature requests.
- Native text selection covers mounted virtual rows only; selecting across an unmounted viewport is a documented implementation boundary. Tests must not imply that browser-native selection spans the complete virtual log.
- ESP-IDF/Tuya E/W severity text coloring exists in the line renderer; do not add a duplicate coloring test as if no severity treatment exists.

## Local setup and commands

```sh
pnpm install --frozen-lockfile
pnpm exec playwright install chromium
pnpm run typecheck:ui
pnpm test:ui
```

`pnpm test:ui` runs all tests, including `@proposal`. Use `pnpm test:ui:confirmed` to exclude the optional sticky-prefix interaction proposal from required repair verification. The UI workflow runs confirmed-defect assertions for pull requests changing UI code, tests, or their configuration, and supports manual dispatch. Repository branch-protection settings determine whether the check is required to merge.

Playwright writes JSON to `test-results/ui-results.json`, HTML to `playwright-report/`, and failure screenshots/traces under `test-results/ui/`. The original red baseline below is historical: its nonzero exit recorded genuine assertion failures, rather than a runner/setup failure.

LOG-01 currently asserts fixed-row geometry (real line heights within 1px of the measured probe) because that is the active fixed-height virtualizer contract and currently confirmed defect. If the product later adopts a variable-height virtualizer, replace that implementation-specific assertion with equivalent end-to-end scroll geometry coverage (no cumulative blank space, stable middle anchor, correct tail/re-entry across size and viewport changes); do not relax this test while the fixed-height renderer remains.

## Additional observation outside the original nine findings

`LOG-01 paused middle reading position is preserved after cached-page navigation` is a separate follow-up control: after the test scrolls to the middle, observes the paused-follow control, navigates to Toolbox, waits for Toolbox heading and verifies the old pane is detached, then returns, the first mounted line changed from ID 171 to 334. This was traced to DOM scrollTop resetting during KeepAlive deactivation and temporary activation scroll events overwriting the saved reading position. The repair saves the model offset and resumes scroll handling only after restoring it; the strict test remains in the confirmed suite.

## Latest completed baseline (2026-10-29)

`pnpm run typecheck:ui` passed. A complete `pnpm test:ui` execution ran 40 tests and returned nonzero: **4 passed, 36 failed, 0 skipped, 0 flaky**. The nonzero status reflects the ordinary regression assertions below failing against the current UI. Failure groups:

- **Confirmed original findings — 34 failures:** UI-01 2 cases (overlap area 1703px² without port and 1612px² with mock port); UI-02 1 (Browse center hit-test false); UI-03 5 (initial focus, Tab/Shift+Tab wrap, Escape restore, close restore); LOG-01 fixed-row-height matrix 24 cases plus independent blank-band test (measured gap 251.5px against 25px allowed); LOG-02 1 (missing `aria-pressed`).
- **Optional proposal — 1 failure:** LOG-03 sticky-prefix `@proposal`; excluded from `pnpm test:ui:confirmed`, not an approved release gate.
- **Separate additional observation — 1 failure:** paused-middle cached navigation changes the first mounted line from 171 to 334, even after waiting for Toolbox heading and pane detachment. This is outside the original nine findings and remains for separate investigation.
- **Passed controls — 4:** non-empty virtual rows, repeated-newline appendChunk split, follow-tail cached return, and LOG-04 search navigation (match count, next/previous return, and whole-session action present).

Playwright overwrites its local reports on subsequent runs, including filtered runs and test listing. Preserve reports from a completed full run when comparing this baseline with a later repair. The counts above describe a completed execution, not a test listing.

## Earlier repair verification (2026-10-30)

- The confirmed suite now contains 40 cases, including an additional regression for closing a settings dropdown with Escape before closing its parent modal. The original 40-case red baseline above remains historical.
- Empty and non-empty ASCII rows share a font-relative minimum height, preserving the fixed-row virtualizer at all tested sizes without dropping blank lines. Cached-page navigation restores the saved model offset before enabling scroll handling again.
- Narrow connection fields/actions wrap, and firmware segments select their layout from the operation card width so Browse remains visible. Settings focus enters, cycles in both directions, and returns to its opener; handled dropdown Escape events do not also dismiss the modal.
- Display toggles expose their pressed state. Their browser locators now use exact accessible names to distinguish the direction toggle from the renamed add-filter-tab button. No state assertions were relaxed.
- English and Chinese copy distinguishes transient Find from persistent filter tabs and explains Update Center actions directly.
- At this earlier checkpoint, typography remained under review. The optional sticky-prefix proposal remains separate from confirmed-defect verification; no variable-height wrapping or new long-line detail interaction is introduced.
- Verification uses synthetic serial data in Chromium. Native Tauri dialogs and physical serial devices are outside this validation.

Confirmed verification: `pnpm test:ui:confirmed` completed with **40 passed, 0 failed, 0 skipped, 0 flaky**. The full Vitest coverage suite passed **1070 tests**; lint, UI/script type checks, Tauri package parity, and production build also passed. Build and browser execution were run separately to prevent Vite reloads from generated HTML.

## UI/UX follow-up verification (2026-10-30)

The user approved the follow-up review changes. Eight new browser cases failed before implementation; a ninth integration case covers the batch mode/file correction path. The final confirmed run completed with **49 passed, 0 failed, 0 skipped, 0 flaky**. Vitest coverage completed with **1122 tests across 76 files** and passed the configured coverage gates.

- Send input clears after success only; a newer draft survives completion or rejection of an older request. Failures remain in the log, and pending submissions are guarded.
- Batch mode follows explicit switches. Firmware, Excel and command-prefix prerequisites apply before start/retry. Retry All waits for active work to finish without discarding diagnostics; a single-port retry may run beside another active port. Read and used/in-progress credential recovery remain supported.
- Shared modal focus behavior covers confirmations, port filtering, serial settings, updates, log viewing and the nested log opener. Tests cover handled dropdown Escape, background inert restoration and full-tree unwind.
- Metadata minimum sizes were checked against the existing 24-case virtual-row matrix without relaxing the 1px tolerance. Prefix line height prevents metadata from enlarging the probe at the 10px body setting.

Lint, frontend/UI/script type checks, Tauri package parity, production build, Rust formatting, all crate Clippy checks, Rust tests (including optional core features), and generated binding drift checks passed. The production preview was also inspected in Chinese/light at 1280×800 and English/dark at 1024×680 across Flash, Serial Debug, Batch Flash & Auth, and Settings. A production interaction check verified the sticky batch toolbar, port-filter focus restoration and serial settings dropdown Escape handling.

Validation uses synthetic state in Chromium. Native Tauri, physical serial devices, native input methods and screen-reader acceptance remain outside this run.

## Detail repair verification (2026-10-30)

The [39-item detail review](./ui-ux-detail-review-2026-10-30.md#本次修复与验收--2026-10-30) records each disposition, the original reproductions and current evidence. The confirmed browser suite completed with **72 passed, 0 failed, 0 skipped, 0 flaky**; 23 new cases live in `ui-detail-regressions.spec.ts`. Vitest coverage passed **1164 tests across 85 files**, including delayed success/error regressions for Excel validation, log reads, update checks and sending, plus filter request cleanup and capped-log follow behavior.

- Popup placement covers bottom triggers, resize, long filter bars, and batch menus at four viewport corners. The height calculation includes borders. Blocked-port dialogs cover 100 entries in English/Chinese and light/dark themes.
- Read-only Hex, modal focus, field-specific address errors in both layouts, deletion focus, theme/tab navigation, clipboard rejection, long focused history and direct update-task navigation have browser coverage. Existing Browse hit-testing, virtual-row geometry, paused reading and modal assertions remain intact.
- Batch recovery/configuration guards, persistent Archive controls, cumulative-reset confirmation, custom baud validation and enumeration failures have component/store tests. Refresh retains log search/position and previously loaded text on failure. Newer drafts and history browsing survive an older send completing.
- The selected-text popup converts displayed UTF-8 text; native selection covers mounted rows only. This does not introduce a byte-level cross-viewport selection contract. The `@proposal` sticky-prefix case remains excluded.

Lint, frontend/UI/script type checks, six Tauri version pairs, production build, Rust formatting, all crate Clippy/tests, optional core feature checks and generated binding drift checks passed. Build and browser execution were separate. The [verification summary](./ui-ux-detail-review-2026-10-30/repair-verification.json) and [full UI result](./ui-ux-detail-review-2026-10-30/repair-ui-results.json) preserve this completed run; earlier counts above describe their own checkpoints.

Validation uses synthetic state and simulated desktop IPC. Native Tauri, hardware operation and long-duration serial stress, actual update download/install/relaunch, clipboard permissions, native input methods and screen readers remain unverified.
