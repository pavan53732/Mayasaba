# Mayasaba UI/UX Specification

## Document status

This document translates the user-interface requirements in [Mayasaba — Complete System Description (for AI agents).md](./Mayasaba%20%E2%80%94%20Complete%20System%20Description%20%28for%20AI%20agents%29.md) into an implementation-facing reference for product, design, WinUI, accessibility, and desktop-test work. The complete system description remains authoritative.

Mayasaba's interface is the native WinUI 3 **Control Room**. It is not a terminal multiplexer, a browser UI, a setup wizard, or a passive chat wrapper. Its job is to make authority, work, uncertainty, evidence, and recovery understandable without exposing private reasoning or raw protocol noise.

## 1. Experience goals

The Control Room must help the user:

1. open or restore a local Windows project without waiting for agents;
2. communicate through one continuous Chat from first idea through delivery;
3. understand what Mayasaba recorded, interpreted, authorized, ran, observed, and verified;
4. inspect agent contributions without confusing them with controller truth;
5. answer only questions that cannot be resolved from authorized evidence;
6. interrupt work at any time;
7. recover from missing agents, crashes, stale work, and partial publication; and
8. trust a completion claim because criterion-linked evidence is visible.

The interface should feel calm, native and direct. It is a conversation, not a dashboard: one strong vertical reading flow, generous whitespace for ordinary messages, and structured cards only when the controller has something consequential to report. Use Fluent styling and system materials only where they preserve readability and accessibility.

## 2. Product-wide interaction principles

### 2.1 Chat is always available

- Every application launch opens Chat immediately.
- CLI discovery runs quietly in the background and never blocks the shell.
- Local history and project recovery controls remain available when zero agents are usable.
- A draft may be typed before a folder is selected; Send remains disabled until an authorized project root is bound.

### 2.2 One composer, one contribution path

- The first and every later message use the same composer, Send action, and `SubmitUserContribution` command.
- Conversation, requirements, questions, change requests, attachments, approvals, and control responses stay in the same Chat.
- There is no goal-intake wizard, special initial prompt, separate Explore composer, or mode switch that changes the meaning of Send.
- Each enabled Send persists exactly one immutable `UserContribution` before interpretation.

### 2.3 Gate operations, not the interface

- Missing agents may block a FULL council, complete three-agent exploration/research, or the task assigned to that agent.
- Missing agents do not block Chat, Open Folder, project creation, project switching, history, drafts, existing evidence, or recovery.
- A persisted message is not automatically shown as interpreted, approved, running, answered, or completed.

### 2.4 Show provenance and uncertainty

- Distinguish user text, agent response, controller result, observed command outcome, validation result, and certification.
- Acknowledge receipt separately from successful execution.
- Label provisional streaming text as provisional until a recorded outcome exists.
- Say `unknown`, `partial`, `stale`, `inconclusive`, or `blocked` when that is the observed state.
- Do not invent progress percentages, token usage, capabilities, or successful cancellation.

### 2.5 Keep control visible

- Pause and Stop remain visible while work is active.
- Pending questions, approvals, and recovery actions link to their exact owning operation.
- No timeout or silence becomes approval.

## 3. Chat-only information architecture

Chat is the only persistent route and fills the application workspace. There is no side rail, tab bar, dashboard, alternate project screen or permanent details pane. The shell has three persistent regions:

1. **Compact Chat header** — project identity, inspectable canonical path, lifecycle phase, operational condition, Pause/Stop and a small overflow menu.
2. **Conversation timeline** — user messages, agent responses and controller-owned contextual cards in chronological order.
3. **Chat footer** — lower-left Open Folder/current-project control plus the composer, pending attachments and Send.

Council, requirements, decisions, tasks, files/evidence, validation/repair, delivery and agent diagnostics run in the background. They become visible only when relevant as a compact timeline card, an inline expansion of that card, or a temporary details sheet over Chat. The details sheet is not a route and cannot be pinned as a replacement workspace.

### Desktop layout

```text
+--------------------------------------------------------------------------------------+
| Mayasaba | Project + path | phase + condition                  [Pause] [Stop] [•••]   |
+--------------------------------------------------------------------------------------+
|                                                                                      |
|                 User message                                                         |
|                                                                                      |
|                 Agent response                                                       |
|                                                                                      |
|                 ┌ Controller work card ───────────────────────────┐                  |
|                 │ Build failed · 1 blocking criterion             │                  |
|                 │ Clear summary and next action       [Show details]                 |
|                 └─────────────────────────────────────────────────┘                  |
|                                                                                      |
|                 Conversation continues                                               |
|                                                                                      |
+--------------------------------------------------------------------------------------+
| [Open Folder / Project ▾]  [Attach files] [ Message Mayasaba...              ] [Send] |
+--------------------------------------------------------------------------------------+
```

Open Folder/current project remains outside the text-entry field and must never look like Attach files. The conversation column is centered with a readable line length; structured controller cards may be wider when a matrix, file list or criterion table requires it.

### Contextual disclosure

| Presentation | Use |
| --- | --- |
| Inline summary card | A material milestone, blocker, choice, failure, recovery event or delivery result |
| Expanded card | Moderate detail such as task attempts, criteria, changed files, council rationale or source links |
| Temporary details sheet | Dense trees, comparison matrices, logs, per-file coverage, diagnostics or long evidence lists |
| Direct Chat query | The user asks “show current tasks,” “why is this blocked?” or another inspection question; Mayasaba returns a current source-linked card |

Routine heartbeats, queue operations, protocol envelopes and low-value progress events stay hidden. Presentation filtering changes only what is shown, never the underlying authoritative records.

### Responsive behavior

- Wide: keep one centered conversation column; a temporary details sheet may use available width while Chat remains visible or dimmed behind it.
- Medium: expand details inline when readable; otherwise use a modal sheet.
- Narrow: cards become single-column and the details sheet becomes full-width but remains dismissible back to the same Chat position.
- Keep Pause/Stop reachable while work is active.
- Restore scroll position, selected card and keyboard/screen-reader focus after closing details.

## 4. Visual direction

The visual idea is a **quiet conversation with an evidence spine**. Ordinary messages remain light and spacious. Controller cards carry a narrow semantic edge and precise status text, so authority and proof are recognizable without turning Chat into a grid of equal boxes.

### Default light-theme tokens

| Token | Value | Role |
| --- | --- | --- |
| `Canvas` | `#F7F8FA` | Application and timeline background |
| `Surface` | `#FFFFFF` | Composer, cards and temporary sheets |
| `Ink` | `#202124` | Primary text |
| `Muted` | `#62666D` | Secondary metadata |
| `Authority` | `#4057C8` | Controller provenance and active controls |
| `Attention` | `#A65D00` | Waiting, warning and unresolved state |
| `Critical` | `#B4232C` | Blocking failure and destructive-stop emphasis |

Use Windows theme resources and high-contrast overrides in implementation; these values define the intended default-light relationships, not hard-coded inaccessible colors. Certification uses a distinct success semantic token but never color alone.

### Typography and shape

- Use **Segoe UI Variable** for interface and conversation text.
- Use **Cascadia Mono** only for code, paths, hashes and command fragments.
- Keep prose lines below roughly 80 characters where window width allows.
- Prefer small-radius message/card containers and quiet one-pixel separators over repeated shadows.
- Use the evidence spine, not decorative gradients, as the memorable visual motif.
- Use motion only for user-triggered expansion, collapse, insertion and confirmed state change; respect reduced motion.

## 5. Startup and folder selection

### 5.1 Initial launch

Render immediately:

- the native application shell;
- Chat welcome/empty state;
- an editable draft composer;
- disabled Send with a clear "Open a folder to send" explanation; and
- lower-left Open Folder in the Chat shell.

Start bounded, non-mutating CLI checks in the background. A transient checking indicator may appear without rendering three status rows.

Do not show:

- a blocking agent-setup page;
- an authentication or provider form;
- a model picker;
- a requirement intake wizard;
- a permanent three-agent readiness panel; or
- a spinner that prevents folder selection.

### 5.2 Open Folder

Open Folder launches the native Windows directory picker. The selected path is a candidate until Workspace Manager and Policy Engine validate locality, existence, permissions, aliases, and scope. Project service then creates or restores the project identity.

Success behavior:

- update the persistent header;
- replace Open Folder with the folder/project name and an inspectable canonical path;
- enable Send;
- restore Chat plus the authoritative background requirements, decisions, tasks and evidence needed to reconstruct its cards; and
- retain any pre-selection draft.

Rejection behavior:

- explain the exact path or authorization problem;
- preserve the draft;
- keep Open Folder available; and
- do not create a partial project identity or imply access was granted.

Opening a folder must not visibly or invisibly:

- launch working agent sessions;
- scan or index the repository;
- consume model-provider usage;
- initialize Git;
- alter files;
- approve a request; or
- grant attachment access outside the selected root.

### 5.3 Project switching

The current-project control offers Open Folder or Switch Folder. Switching uses Project service and must:

- preserve each project's independent Chat and state;
- never retarget an active CLI session to the new root;
- never merge projects or rebind tasks;
- require controlled pause/recovery when active work makes switching unsafe; and
- restore the selected project's projections after binding succeeds.

## 6. Agent readiness UX

### 6.1 Exception-only notices

Normal UI warns only about agents needing attention:

- `MISSING`: executable not found;
- `UNSUPPORTED`: observed behavior or safety requirements cannot be supported; or
- `PROBE_FAILED`: a bounded readiness check failed for a concrete reason.

`READY` agents are omitted from warning lists. When every agent is ready, no setup banner appears. `CHECKING` may use a small nonblocking indicator.

Each problem row includes:

- exact CLI name;
- exact observed status;
- concise cause;
- effect on current operations;
- guidance to install, authenticate, configure, or repair it externally;
- **Locate executable** where appropriate; and
- **Recheck**.

Do not label an installed but incompatible CLI as missing. Do not disguise a later provider-login or task-tool failure as an installation problem.

### 6.2 Technical diagnostics

An explicitly opened temporary diagnostics sheet may show all three adapters, probe time, executable identity, supported observed capabilities, session status, failure details, and recovery history. It opens over Chat, is read-only except for scoped controls such as Locate and Recheck, and returns focus to its invoking control when dismissed.

It must not expose credentials, tokens, private prompts, or model configuration controls.

### 6.3 Operation-specific status

Active work shows attributed session state in contextual Chat cards and their details even when installations are healthy. Distinguish:

- ready;
- starting;
- synchronizing;
- running;
- idle;
- waiting;
- blocked;
- cancelling;
- failed;
- recovering; and
- stopped.

Installation health and task/session activity are different concepts and should not share a misleading badge.

## 7. Chat experience

### 7.1 Conversation timeline

The timeline contains visually distinct entries for:

- user contributions;
- agent-attributed responses;
- controller interpretations and outcomes;
- clarifying question batches;
- approval/choice requests;
- task and decision summaries;
- observed execution/validation updates;
- recovery notices; and
- final certification.

Every submitted user message displays author, timestamp, persistence state, interpretation/route state, and links to relevant `UserContribution`, request, question, decision, task, or evidence records.

Agent attribution always names Hermes, Kilo Code, or OpenCode. Controller-owned summaries are not styled as a fourth agent.

Controller cards use a consistent anatomy: plain-language title, source-linked status, one-sentence consequence, timestamp, affected operation, primary next action when one exists, and **Show details**. A narrow evidence spine distinguishes controller authority from conversational messages. Do not emit a new card for every heartbeat or low-level event; update the existing source-linked card when that preserves truthful chronology, and append a new card when the authoritative outcome materially changes.

### 7.2 Composer

The single native composer provides:

- multiline text;
- configurable keyboard send behavior;
- Attach files;
- drag-and-drop;
- removable attachment chips/cards;
- one visible Send action; and
- context for a linked reply when answering a question or choice packet.

The composer never writes requirements, decisions, `ProjectIntent`, or project epochs directly.

Draft rules:

- preserve text and valid attachments after a rejected submission;
- selecting files does not submit the draft;
- attachment rejection does not discard valid selections;
- project switching must not accidentally send a draft to another project; and
- application restart restores a draft only according to an explicit local draft policy, never as a submitted contribution.

### 7.3 Message and request states

Folder state and message state remain separate.

Chat Send states:

- draft;
- sending;
- persisted; and
- rejected.

Derived operation states shown beneath or beside the contribution may include:

- commentary or answer;
- interpretation pending;
- clarification needed;
- pending authorization;
- authorized;
- waiting for an agent/capability;
- running;
- rejected; and
- completed with evidence.

A persisted contribution can legitimately have no completed interpretation yet. The UI must not merge those states into one checkmark.

### 7.4 Streaming and long histories

- Virtualize long Chat histories and dense event/evidence lists.
- Apply bounded live-update batches on the UI dispatcher.
- Preserve the user's reading position while new content streams.
- Show a clear Jump to latest action when the user is not at the end.
- Do not force-scroll during reading, selection, or assistive-technology navigation.
- Keep streaming content provisional and distinguish it from persisted controller outcomes.

## 8. Attachments

### 8.1 Selection and preview

Users may select one or more local files with the native picker or drag them onto the composer. Each pending file displays:

- file name;
- detected type;
- size;
- preparation state;
- removal control;
- image thumbnail when supported;
- read-only text/document preview when supported; or
- explicit preview-unavailable fallback.

Declare and enforce count, size, type, and parser limits. Place an actionable error beside the rejected file.

### 8.2 Attachment lifecycle

Visually distinguish:

1. selected;
2. preparing;
3. ready;
4. rejected;
5. submitted/persisted; and
6. included in a particular context snapshot.

A file displayed in Chat is not automatically reported as read, synchronized to every agent, accepted as a requirement, or verified as evidence.

Submitted attachments appear with the originating contribution. Expanding the attachment card shows provenance, digest, supported preview, and context-inclusion status without opening a Files page.

### 8.3 Outside-root files

Selecting a file outside the project root requires explicit authorization for that file and its managed copy. It does not authorize the parent directory or neighboring content. The UI must explain this boundary before the copy is accepted.

Attachment processing never executes the file or treats embedded text as controller authority.

## 9. Conversation, exploration, and work-request UX

### 9.1 Ordinary conversation

Questions and brainstorming can receive normal Chat responses without creating tasks, locking requirements, or opening a FULL council. The UI should avoid adding heavy workflow chrome to a simple answer.

### 9.2 Repository exploration

When the user asks to inspect the entire project, show a bounded exploration card within Chat:

- requested canonical root;
- inventory/manifest status;
- three named independent assignments;
- per-agent status;
- source verification status;
- inventory, content-inspected, analyzed, excluded, unreadable, unsupported, too-large, and stale counts;
- verified facts, disagreements, unknowns, and limitations; and
- per-file coverage/citation drill-down.

Do not flood the main conversation with a raw file listing. If any agent is unavailable or any readable file lacks current inspection evidence, label the report partial or blocked rather than complete.

### 9.3 Engineering request

For requested implementation, show the source-linked request's progression:

- recorded;
- clarification needed;
- pending authorization;
- authorized;
- planning/council as applicable;
- queued/leased;
- active;
- integration/validation;
- repair/recovery; and
- evidence-backed completion.

Ask only for details needed by the affected operation. A blocked request must not freeze unrelated Chat or authorized work.

### 9.4 Mid-project changes

A feature addition, removal, or direction change remains a normal Chat contribution. If material, display:

- a source-linked pending change proposal;
- affected requirements, decisions, tasks, and artifacts;
- the FULL council decision point;
- required user approval;
- epoch/plan invalidation after authorization; and
- unaffected work that may continue.

Never display the proposed change as approved scope before the owning services commit it.

## 10. Questions, choices, and approvals

### 10.1 Clarification batch

Agents' unresolved questions are normalized, deduplicated, checked against available evidence, and presented in one batch. Each question includes enough context to answer and identifies the affected operation.

### 10.2 Choice packet

An `escalated` council result presents:

- competing options;
- applicable hard constraints;
- user preferences already on record;
- strongest supporting and contradicting evidence;
- measured costs, risks, reversibility, and downstream impact;
- unresolved uncertainty;
- a conflict matrix tied to requirement IDs; and
- an advisory recommendation clearly labeled advisory.

### 10.3 Open factual question

A `sealed with an open question` result requests missing facts or clarification, not a preference. Its response contract and labeling must differ from a choice packet.

### 10.4 Approval semantics

- Approval controls name the exact proposed effect.
- A response is linked to the exact question/operation/decision point.
- Silence, timeout, an unrelated "yes," or an agent recommendation is never approval.
- After a material answer changes project truth, show that a new context/epoch is being prepared and affected agents must resynchronize.

## 11. Council cards and details

Council remains a background subsystem. Insert a concise council card only when a decision begins, materially changes, needs the user, blocks work or reaches an outcome. **Show details** prioritizes structured inspection over conversational theater.

For each decision point, show:

- stable identity and triggering event/gate;
- precise question and affected records;
- current epoch/context digest;
- round budget and current round;
- all three required participants;
- deterministic chair and rotating duties;
- independent proposals;
- the six directed critique assignments and completion state;
- rebuttals and revisions with predecessor links;
- evidence grades and source links;
- surviving material disagreements;
- synthesis plus non-chair review and coverage result;
- round outcome and continuation reason; and
- linked user question, choice packet, or binding decision.

Do not expose private chain of thought. Concise rationale, claims, evidence, alternatives, and objections are sufficient.

Visually communicate that:

- chair is a synthesis role, not a superior agent;
- majority is not authority;
- repeated citations to one observation are one evidence source;
- an absent critique is incomplete, not an abstention; and
- stable agreement is deliberative stability, not proof of universal optimality.

## 12. Requirements and decision cards

### 12.1 Requirements

An on-demand requirements card, generated by a material change or direct Chat request, shows:

- current source-linked `ProjectIntent` projection;
- approved requirements and versions;
- amendments and pending changes;
- approval state;
- originating contributions;
- linked decisions/tasks/acceptance criteria; and
- affected epoch.

Do not present `ProjectIntent` as a user-authored upfront brief or a global completion checkpoint.

### 12.2 Decisions

A decision card and its expanded details show:

- decision question and resolution;
- disposition: `HARD_LOCK`, `SOFT_DECISION`, `ASSUMPTION`, or `OPEN`;
- alternatives and shared evaluation criteria;
- supporting/contradicting evidence;
- authoritative preferences and hard constraints;
- residual risks and uncertainty;
- affected records and artifacts;
- originating council rounds and positions;
- predecessor/successor records; and
- explicit reopening action.

When evidence challenges a decision, show the scoped validity concern and affected downstream work. Do not silently portray the historical lock as reversed.

## 13. Task cards and details

Tasks run in the background. Chat receives a compact task card for meaningful assignment, blocker, retry, repair, integration or completion changes. Expanded card details or a temporary sheet may show graph and list representations appropriate to scale. Every task projection exposes:

- objective and expected artifacts;
- source contribution/work request;
- dependencies;
- assigned CLI;
- allowed read/write scope in concise form;
- attempt history;
- active lease/fencing version where technically useful;
- progress derived from observed records;
- blockers and retry/repair status;
- acceptance criteria and required oracles; and
- changed paths and evidence after execution.

Retry and reassignment are controller-authorized actions, not direct agent controls. Avoid false numeric percentages when the underlying work is not measurable.

## 14. Files and evidence cards

Provide linked Chat cards and on-demand detail sheets for:

- selected project files;
- managed submitted attachments;
- generated artifacts;
- source citations;
- repository exploration coverage;
- observed command/process output references;
- hashes and environment identity;
- producing task/attempt/command; and
- acceptance criteria that use the evidence.

The internal evidence/blob store is not presented as a second editable project tree. A missing or corrupted managed blob shows an integrity/availability failure and cannot satisfy a criterion.

Diagnostic records may expose protocol or database details only in a temporary read-only, redacted technical sheet over Chat. Inspecting evidence never expands filesystem authority.

## 15. Validation, repair, and delivery cards

### 15.1 Criterion accountability

For every criterion display:

- stable ID and source requirement;
- observable expectation;
- oracle or review procedure;
- blocking/nonblocking designation;
- artifact/hash and environment evaluated;
- staged versus published-location result;
- one verdict: `PASS`, `FAIL`, `INCONCLUSIVE`, `BLOCKED`, or `NOT_APPLICABLE`;
- oracle-adequacy status and known gaps; and
- evidence links.

Text and iconography must communicate outcomes in addition to color.

### 15.2 Separate validation domains

Do not collapse these into one green state:

- contract/schema validation;
- formatting/static analysis;
- compilation/build;
- unit/integration/regression tests;
- process launch;
- runtime behavior;
- E2E behavior;
- accessibility/UX review;
- cross-agent review;
- publication/reconciliation;
- packaging/install lifecycle; and
- final certification.

### 15.3 Repair

Show the failure class, failing criterion, observed signature, current hypothesis, bounded repair scope, responsible task/agent, attempt history, targeted rerun, and regression outcome.

When the same failure repeats without progress, explain that the strategy stopped and new evidence or a new hypothesis is required. Toolchain, provider, policy, and external-environment failures must not look like application-code defects.

### 15.4 Publication and recovery

During integration show:

- staged candidate status;
- conflict/freshness checks;
- pre-publication validation;
- publication journal status;
- files pending/applied/conflicted;
- restart reconciliation;
- post-publication checks; and
- whether the original folder is restored, modified, certified, or awaiting user action.

Never show partial multi-file publication as completed. Never hide preserved user-edit conflicts.

### 15.5 Delivery

Completion appears only after all applicable blocking criteria have fresh evidence and the controller certifies the exact deliverables in the selected root. The final delivery card lists final paths, hashes where useful, certification evidence, nonblocking warnings, and any explicitly accepted limitations.

## 16. Feedback and visual semantics

Use a consistent semantic vocabulary across messages, cards and temporary sheets:

| Semantic state | Meaning | UI treatment |
| --- | --- | --- |
| Neutral | Recorded or available; no outcome implied | Plain label and timestamp |
| Provisional | Streaming or proposed, not authoritative | Explicit provisional styling/text |
| Active | Observed work is currently progressing | Activity indicator plus descriptive verb |
| Waiting | Awaiting a declared dependency, user input, or agent | Name the dependency and available action |
| Blocked | Cannot proceed safely or with current authority/capability | Strong text label, reason, and recovery path |
| Warning | Nonblocking risk or uncovered case | Text and icon; preserve at delivery |
| Failed | A valid operation or oracle observed failure | Exact failed item and next action |
| Unknown | Physical outcome cannot yet be established | Never style as success or ordinary retry |
| Recovered | Reconciliation established a safe current state | Describe what was recovered and revalidated |
| Certified | Controller proved all applicable completion gates | Reserved final-success treatment |

Avoid using the same green check for persistence, ACK, process exit, passing one test, and final certification.

## 17. Empty, loading, error, and disconnected states

Every card type and temporary sheet defines:

- first-use empty state;
- no matching records state;
- bounded loading/checking state;
- partial-data state;
- unavailable dependency state;
- recoverable error state;
- corrupted/integrity-failure state; and
- restart reconciliation state.

Each state explains what happened, what remains trustworthy, and the next available user action. Preserve existing readable state during transient failures instead of blanking the interface.

After restart, render persisted records as persisted and mark interrupted physical outcomes as reconciling or unknown until observed. Never assume interrupted work completed.

## 18. Accessibility

Accessibility is a release requirement, not polish.

### Keyboard

- Every interactive element is reachable in a logical order.
- Visible focus is always present.
- Chat, cards, inline details, question batches, tables, temporary sheets and dialogs have predictable focus entry/return.
- Pause and Stop remain keyboard reachable during streaming.
- Configurable Send behavior avoids trapping multiline entry users.

### Screen readers and semantics

- Give every control an accessible name, role, state, and useful description.
- Announce meaningful state changes without reading every streamed token or event.
- Identify agent attribution and controller attribution in accessible text.
- Tables, trees, graphs, and evidence relationships have linear alternatives.
- Do not encode status solely through position, color, animation, or icon.

### Visual accessibility

- Support system text scaling, DPI scaling, high contrast, and color themes.
- Maintain readable contrast over system materials.
- Respect reduced-motion preferences.
- Keep focus, selection, error, warning, and certification visually distinct.
- Avoid dense fixed-size panels that clip translated or scaled text.

### Performance accessibility

- Keep the UI thread free from agent I/O, parsing, database work, and long validation operations.
- Virtualize long lists.
- Batch frequent updates.
- Preserve assistive-technology focus during refresh.
- Do not replace stable controls unnecessarily as status changes.

## 19. Privacy and information boundaries

The ordinary interface may show authorized source files, submitted attachments, generated artifacts, relevant sanitized output, and evidence. It must not show:

- provider credentials or authentication tokens;
- private chain of thought;
- unrelated environment variables or filesystem content;
- raw secrets found in source or web content;
- unrestricted CLI logs;
- sensitive internal database/protocol records without redaction; or
- data from another project or session.

When a provider or research tool may receive project context, the user-facing flow must preserve the governing authorization and provider policy. A UI preview or evidence link never widens access.

## 20. Native implementation boundaries

- Use WinUI 3 controls, XAML layouts, and C++/WinRT view models.
- View models submit typed commands/queries and render immutable projections.
- Implement one persistent Chat route only. Council, requirements, decisions, tasks, files/evidence, validation/repair, delivery and diagnostics are card/detail projections, not pages or navigation destinations.
- Dispatch UI updates explicitly and in bounded batches.
- UI code does not write SQLite, launch executables, manage worktrees, parse raw CLI protocols, authorize material actions, or compute certification.
- Keep view-only formatting and selection state separate from durable domain state.
- Use Windows UI Automation properties and stable automation IDs for critical flows.

## 21. Desktop acceptance scenarios

At minimum, native automated and manual acceptance must cover:

### Startup and readiness

1. First launch with zero, one, two, and three usable CLIs always shows Chat and Open Folder.
2. Only `MISSING`, `UNSUPPORTED`, and `PROBE_FAILED` agents appear in the normal warning UI.
3. All agents ready produces no setup banner.
4. Recheck updates observed status without installing, updating, or changing a model/provider.
5. A later provider/task-tool failure is attributed to the affected operation, not mislabeled as missing.

### Folder and project identity

6. Draft before folder selection; Send is disabled for root absence only.
7. Valid empty and populated folders create/restore projects.
8. Invalid, denied, aliased, and reparse-escape roots fail with exact feedback.
9. Opening a folder launches no working agent and performs no implicit scan.
10. Switching projects preserves identity and never retargets a live session.

### Chat and requests

11. First and later messages persist through the identical contribution path.
12. Missing agents do not prevent contribution persistence.
13. Normal conversation does not create implementation tasks or a council.
14. A multi-intent message yields distinct linked operation statuses.
15. Work-specific clarification blocks only affected work.
16. A material mid-project change remains pending until council and approvals finish.
17. Rejected submission preserves the draft and valid attachments.

### Attachments

18. Select, remove, drag/drop, preview, reject, submit, and inspect supported/unsupported files.
19. Selecting an attachment never sends the draft.
20. Outside-root attachment authorization remains file-scoped.
21. Missing/corrupt managed bytes display an integrity failure and cannot appear included/verified.

### Exploration and council

22. Three-agent exploration shows independent assignment and honest coverage counts.
23. Unreadable, stale, oversized, and excluded files prevent false full-coverage claims.
24. Council renders three proposals and six real critiques; absence cannot look complete.
25. First-round stability without synthesis shows continuation, not convergence.
26. Choice and factual-question outcomes use distinct packets and response handling.

### Work, evidence, and recovery

27. One-agent task starts only its assigned CLI; FULL workflows start all three headlessly.
28. Start, stream, cancel, timeout, crash, and recover without visible terminal windows.
29. Build, launch, runtime, E2E, and certification remain separate results.
30. Every criterion shows one of the five allowed verdicts and linked evidence.
31. Partial publication and concurrent user edits show recovery/conflict, never success.
32. Restart reconciles interrupted process and publication outcomes before positive status.
33. Final completion appears only with fresh criterion-linked published-artifact evidence.

### Accessibility and responsiveness

34. Complete critical flows using keyboard only.
35. Verify screen-reader names, roles, states, reading order, and useful announcements.
36. Verify high contrast, text/DPI scaling, reduced motion, and non-color status cues.
37. Sustain large Chat, task, council, and evidence streams without UI-thread stalls or reading-position loss.
38. Verify there is no project-navigation rail, tab bar, dashboard or standalone project page at any window size.
39. Verify every background domain is reachable through a relevant Chat card, direct Chat query or temporary details sheet without creating another route.
40. Verify card expansion and sheet dismissal restore timeline position, keyboard focus and screen-reader context.

## 22. UX anti-patterns

Do not introduce:

- a blocking startup/setup wizard;
- a model/provider/API-key selector;
- a permanent READY list for all three agents;
- a project-navigation rail, project tab bar, dashboard or standalone Council/Requirements/Decisions/Tasks/Files/Validation/Delivery/Settings page;
- a separate goal intake or Explore input mode;
- agent terminals the user must launch or manage;
- a folder picker inside the attachment control;
- a global progress percentage inferred from agent text;
- a generic success check that conflates receipt, execution, test, and certification;
- a two-agent fallback council;
- a majority vote display as the decision rule;
- hidden automatic approval or timeout-to-consent;
- editable internal evidence/database state as a second project truth;
- raw protocol traffic in ordinary Chat;
- private chain-of-thought display;
- false token counts or a credential/token display; or
- a visual claim of full repository analysis without current per-file evidence.

The best Mayasaba interface is not the one that makes automation look effortless. It is the one that makes the exact boundary between request, authority, execution, evidence, and completion easy to see.
