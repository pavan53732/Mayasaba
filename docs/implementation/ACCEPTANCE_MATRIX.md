# Mayasaba Acceptance Evidence Matrix (Milestone M13)

This document is the authoritative implementation-stage `AcceptanceEvidenceMatrix` for Mayasaba, fulfilling **AGENTS.md § 13 and § 14** and **Milestone M13** of [PLAN.md](./PLAN.md).

> [!NOTE]
> **Audit & Supersession Record (2026-10-10):**  
> This matrix explicitly supersedes prior unverified drafts and provisional digests. During the audit, the overall completion verdict was withdrawn until every blocking criterion was proven with retained logs and verified runtimes. Previously recorded SHA-256 digests from intermediate builds and unverified ASan runs (affected by toolchain dynamic library search paths) are marked as incorrect or unverified and are superseded by the recomputed digests and captured logs recorded below.

---

## 1. Environment & Provenance Record

| Property | Value |
| --- | --- |
| **Operating System** | Windows 10/11 x64 (Build 26100 SDK) |
| **Compiler & Toolchain** | MSVC 19.50.35724 (Visual Studio 2026 Enterprise), toolset v145 |
| **Build Systems** | CMake 4.2.3, MSBuild 18.3.0 |
| **Packaging Engine** | WiX Toolset 6.0.2+b3f3403 (`wix` dotnet tool) |
| **Testing Harness** | CTest / GoogleTest 1.15.2 (14 test targets, 100% passing across Debug, Release, and ASan) |
| **Runtime Target** | Native x64 Windows desktop, WinUI 3 (Windows App SDK 1.7 self-contained unpackaged) |
| **Database Engine** | SQLite 3.54.0 (amalgamation, WAL mode, serialized transactions) |
| **Release Manifest** | [packaging/release/RELEASE_MANIFEST.json](../../packaging/release/RELEASE_MANIFEST.json) |

---

## 2. Maintained Release Artifact Digests

All hashes recomputed from the exact release build outputs and verified against the release manifest. Alternate and intermediate artifacts are retained in local staging for historical comparison.

| Artifact | Path | Scope / Type | SHA-256 Digest | Status |
| --- | --- | --- | --- | --- |
| **Maintained Release Installer** | [packaging/release/Mayasaba.msi](../../packaging/release/Mayasaba.msi) | `perUser` MSI (20,612,679 bytes) | `037C8B692072E087205A8ADE5FFB1B70AB84414A50897FC6007A008DDCA19EC0` | **VERIFIED** |
| *Superseded Intermediate MSI* | `packaging/release/Mayasaba_draft.msi` | Intermediate prototype | `B8A48CA29BB9625053C5A560EAF091D4A21D4E6FD30E8511BA817DFFE7EC4B0B` | *SUPERSEDED* |
| **Control Room App (Release)** | `app/Mayasaba.App/x64/Release/Mayasaba.App/Mayasaba.App.exe` | WinUI 3 Executable (2,792,448 bytes) | `647F3AA312163F10A93E7B53A6500346CF2946A57C22B03442395A0BA7F86FFE` | **VERIFIED** |
| **Application Services Lib (Release)** | `build/vs2026/app/Release/mayasaba_app.lib` | Static Library (Layer 2) | `2C8C163B2E1344BD50EAA33359497C063D793BCDAFB85C0A6608016FFB24F76D` | **VERIFIED** |
| **Core Engine Lib (Release)** | `build/vs2026/core/Release/mayasaba_core.lib` | Static Library (Layers 3-13) | `6C61D5A48A6F768C3388E1C9E2AE0F04E015CB4E3A88FA63B3F70957B9B999AB` | **VERIFIED** |
| **CLI Adapters Lib (Release)** | `build/vs2026/adapters/Release/mayasaba_adapters.lib` | Static Library (Layer 4) | `A019E24A2B580528689E5A9399A4FE18B680C40D11FA3A176B00EBB8CA1DB793` | **VERIFIED** |

---

## 3. Required Pre-Implementation Prototypes (AGENTS.md § 14)

| # | Prototype Gate | Required Behavior | Evidence & Discriminative Oracle | Captured Log / Metric | Verdict |
| --- | --- | --- | --- | --- | --- |
| **P1** | **Responsive WinUI Control Room under sustained streaming** | Timeline updates remain smooth, blocking I/O stays off UI thread, Chat is the sole persistent workspace. Max query latency during continuous 30-message burst must remain <500ms. | [controller_tests.cpp](../../tests/e2e/controller_tests.cpp) (`ControllerE2E.SustainedStreamingMaintainsResponsivenessAndInteractivity` & `ControllerE2E.VerticalSliceFromFolderToPublishedChange`). Verifies 30 streamed notifications with concurrent `QueryTimeline`, `QueryProjectState`, `QueryAgents`, `DismissCard`. | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log)<br>[ctest_vs2026_asan.log](./logs/ctest_vs2026_asan.log)<br>(Latency: 139ms max under ASan, <3ms in Release; 0 missed notices) | **PASS** |
| **P2** | **Process launch, cancellation, and crash cleanup** | Win32 suspended process creation assigned to controller-owned Job Object, handle inheritance restricted, clean teardown of entire process tree (child and grandchild) on exit/cancel. | [kernel_tests.cpp](../../tests/fault/kernel_tests.cpp) (`Kernel.CancellationTerminatesWholeProcessTree`, `Kernel.JobAccountingShowsSingleActiveProcessAfterExit`, `Kernel.LaunchesCapturesAndObservesExit`). Verifies grandchild PID does not survive job termination. | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log)<br>[ctest_vs2026_asan.log](./logs/ctest_vs2026_asan.log)<br>(Grandchild PID confirmed exited within 5s bound) | **PASS** |
| **P3** | **Enforceable workspace access & stale-write rejection** | Isolated staging worktree/directory per task; Job Objects alone do not establish filesystem containment; Workspace Manager enforces path boundaries, conflict detection, publication journal before mutation, stale-lease fencing rejection, and never rolls back newer user edits. | [workspace_tests.cpp](../../tests/unit/workspace_tests.cpp) (`WorkspaceStaging.CopiesAllowedSubsetAndLeavesRootUnchanged`, `WorkspacePublish.ConflictDoesNotOverwriteDivergedRootFile`, `WorkspacePublish.JournalIsPersistedBeforeMutationAndRecordsFailure`, `WorkspacePublish.ReconcileReportsDivergedFileWithoutRollback`); [tasks_tests.cpp](../../tests/unit/tasks_tests.cpp) (`TasksLease.ValidateAndRevokeFencing`, `TasksAttempt.StartRequiresLeaseVersionMatchAndPersistsHistory`). | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log)<br>[ctest_vs2026_asan.log](./logs/ctest_vs2026_asan.log)<br>(Stale leases fail with `ErrorCode::Stale`; user edits preserved on conflict) | **PASS** |

---

## 4. Full Acceptance Evidence Matrix

| Criterion ID | Requirement / Scope | Observable Expectation | Oracle Class | Exact Check Command & Working Dir | Blocking | Captured Log Reference | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **CRIT-01** | AGENTS.md § 2, 3: Toolchain & Platform | Native Windows C++20 with MSVC v145, SQLite, and WinUI 3; no cloud backend. | `compile` | `cmake --preset vs2026` then `cmake --build --preset vs2026-debug` from repository root | YES | [ctest_vs2026_debug.log](./logs/ctest_vs2026_debug.log) | **PASS** |
| **CRIT-02** | AGENTS.md § 4.1: Layer Boundaries | 13 distinct functional layers; raw CLI protocols isolated in adapters; app services in `mayasaba_app`. | `static` | CMake target hierarchy check: `mayasaba_core` &rarr; `mayasaba_adapters` &rarr; `mayasaba_app` | YES | `build/vs2026/CMakeCache.txt` target graph | **PASS** |
| **CRIT-03** | AGENTS.md § 4.2: Record Ownership | Single authoritative owner per record; SQLite alone executes SQL. | `unit` | `ctest --preset vs2026-release -R store_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-04** | AGENTS.md § 4.4, 13: Validation Fail-Closed | Empty validation result or inconclusive evidence must fail task; never complete. | `unit` | `ctest --preset vs2026-release -R orchestration_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-05** | AGENTS.md § 5: Chat-Only UX | Native Chat is the sole top-level route; Open Folder at lower left; exception-only CLI warnings. | `runtime` | Inspect [ChatPage.xaml](../../app/Mayasaba.App/Chat/ChatPage.xaml) & execute `ctest --preset vs2026-release -R controller_tests` | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-06** | AGENTS.md § 6: Three External CLIs | Exactly Hermes, Kilo Code, and Claude Code CLIs coordinated headlessly; no fourth model; probe-gated launch. | `unit` | `ctest --preset vs2026-release -R adapter_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-07** | AGENTS.md § 7: FULL Council Engine | 5-round cap, sequential rounds, 6 directed peer critiques, non-chair synthesis review, deterministic chair rotation. | `unit` | `ctest --preset vs2026-release -R council_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-08** | AGENTS.md § 8: Workspace Staging | Isolated staging for implementation tasks; guarded publication journal; user folder untouched during execution. | `unit` | `ctest --preset vs2026-release -R workspace_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-09** | AGENTS.md § 9: Process Security | Local Execution Kernel exclusive launch path; Win32 Job Objects; secrets redacted from context and events. | `unit` | `ctest --preset vs2026-release -R kernel_tests` & `-R base_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-10** | AGENTS.md § 10: MCF-v2 Bus | Envelopes, inbox/outbox persistence, sequence tracking, dedup, dead-letter, verified ACKs. | `unit` | `ctest --preset vs2026-release -R bus_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-11** | AGENTS.md § 11: SQLite Persistence | Append-only events chain; SHA-256 tamper detection; idempotency ledger; WAL mode. | `unit` | `ctest --preset vs2026-release -R store_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-12** | AGENTS.md § 12: End-to-End Vertical Slice | Folder bound &rarr; contribution persisted &rarr; staged task &rarr; execution &rarr; publication &rarr; validation timeline. | `e2e` | `ctest --preset vs2026-release -R controller_tests` from repository root | YES | [ctest_vs2026_release.log](./logs/ctest_vs2026_release.log) | **PASS** |
| **CRIT-13** | AGENTS.md § 13: WiX MSI Installer | Standalone `.msi` authored with WiX v6; unpackaged self-contained WinUI 3 payload; verified installation and runtime execution. | `package` | Pipeline: `packaging\build-msi.bat`. Scope: `perUser` to `[LocalAppDataFolder]\Mayasaba` (no `perMachine` mandate in spec; avoids non-elevated error 1925). Test: `powershell -ExecutionPolicy Bypass -File scripts\verify-msi-install.ps1` | YES | [msi_install_verification.log](./logs/msi_install_verification.log)<br>[RELEASE_MANIFEST.json](../../packaging/release/RELEASE_MANIFEST.json) | **PASS** |
| **CRIT-14** | AGENTS.md § 3: Linux-only Packaging | Product packaging, WinUI execution, and installer testing must be on Windows. | `policy` | Windows-only desktop product specification. Linux deployment is explicitly out of scope. | NO | Spec § 3 ("Build and validate the product on Windows") | **NOT_APPLICABLE** |

---

## 5. Retained Execution Logs & Test Results

All test runs executed natively on Windows and captured with full configuration, timestamps, exit codes, and durations.

### 5.1 Release Build Suite (`ctest --preset vs2026-release`)
- **Log File:** [logs/ctest_vs2026_release.log](./logs/ctest_vs2026_release.log)
- **Exit Code:** `0`
- **Duration:** `6,233 ms` (Real test time: 6.19 sec)
- **Timestamp (UTC):** `2026-10-10T03:20:54.8633676Z`
- **Result:** 14/14 test targets passed (100%).

### 5.2 Debug Build Suite (`ctest --preset vs2026-debug`)
- **Log File:** [logs/ctest_vs2026_debug.log](./logs/ctest_vs2026_debug.log)
- **Exit Code:** `0`
- **Duration:** `7,828 ms` (Real test time: 7.75 sec)
- **Timestamp (UTC):** `2026-10-10T03:20:48.6147823Z`
- **Result:** 14/14 test targets passed (100%).

### 5.3 AddressSanitizer Suite (`ctest --test-dir build/vs2026-asan -C Debug`)
- **Log File:** [logs/ctest_vs2026_asan.log](./logs/ctest_vs2026_asan.log)
- **Exit Code:** `0`
- **Duration:** `27,078 ms` (Real test time: 27.03 sec)
- **Timestamp (UTC):** `2026-10-10T03:21:21.9464615Z`
- **Toolchain Runtime:** Repaired via automatic `PATH` and DLL deployment of `clang_rt.asan_dbg_dynamic-x86_64.dll` from MSVC bin directory (`14.50.35717`). All binaries start and execute under ASan.
- **Sanitizer Reports:** **0 errors / 0 reports**. No heap buffer overflows, use-after-free, or memory violations.
- **Result:** 14/14 test targets passed (100%).

### 5.4 MSI Live Installation & Execution Test
- **Log File:** [logs/msi_install_verification.log](./logs/msi_install_verification.log)
- **Exit Code:** `0`
- **Duration:** `16,850 ms`
- **Timestamp (UTC):** `2026-10-10T03:23:32.0636422Z`
- **Package:** `packaging/release/Mayasaba.msi` (SHA-256: `037C8B692072E087205A8ADE5FFB1B70AB84414A50897FC6007A008DDCA19EC0`)
- **Observed Actions:**
  1. Passive installation completed with ExitCode `0`.
  2. Verified installed executable presence: `C:\Users\pavan\AppData\Local\Mayasaba\Mayasaba.App.exe`.
  3. Verified payload file count: 285 files (matches manifest layout).
  4. Executed installed binary: verified active running process (`PID 12200`).
  5. Process terminated gracefully and uninstalled cleanly via ProductCode `{9750A5BF-D2AC-4C2D-B4E5-6FE45D8C2D3E}` with ExitCode `0`.

---

## 6. Definition of Done Verdict

- **13 of 13 blocking acceptance criteria are PASS** with retained, reproducible evidence.
- Criterion **CRIT-14** is validly **NOT_APPLICABLE** with verified specification rationale.
- **Zero blocking criteria are FAIL, INCONCLUSIVE, or BLOCKED.**
- All pre-implementation prototypes **P1, P2, and P3 are PASS** with discriminative runtime oracles.
- The maintained packaging pipeline produces a verified, deployable MSI linked to an authoritative release manifest.

**Mayasaba implementation milestones M0 through M13 are COMPLETE.**
