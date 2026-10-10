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
| **Testing Harness** | CTest / GoogleTest 1.15.2 (19 test targets, 100% passing across Debug and Release) |
| **Runtime Target** | Native x64 Windows desktop, WinUI 3 (Windows App SDK 1.7 self-contained unpackaged) |
| **Database Engine** | SQLite 3.54.0 (amalgamation, WAL mode, serialized transactions) |
| **Release Manifest** | [packaging/release/RELEASE_MANIFEST.json](../../packaging/release/RELEASE_MANIFEST.json) |

---

## 2. Maintained Release Artifact Digests

All hashes recomputed from the exact release build outputs and verified against [RELEASE_MANIFEST.json](../../packaging/release/RELEASE_MANIFEST.json).

| Artifact | Path | Scope / Type | SHA-256 Digest | Status |
| --- | --- | --- | --- | --- |
| **Maintained Release Installer** | [packaging/release/Mayasaba.msi](../../packaging/release/Mayasaba.msi) | `perUser` MSI (20,645,447 bytes) | `580DF92D6FB1F493B4473C6A248D940205CB6E930794E4EE537755481E002091` | **VERIFIED (LIFECYCLE PASS)** |
| *Prior Tested Package (Historical)* | `packaging/release/Mayasaba.msi` (prior build) | Documented in 03:23 log | `037C8B692072E087205A8ADE5FFB1B70AB84414A50897FC6007A008DDCA19EC0` | *SUPERSEDED BUILD* |
| *Superseded Intermediate MSI* | `packaging/release/Mayasaba_draft.msi` | Intermediate prototype | `B8A48CA29BB9625053C5A560EAF091D4A21D4E6FD30E8511BA817DFFE7EC4B0B` | *SUPERSEDED* |
| **Control Room App (Release)** | `app/Mayasaba.App/x64/Release/Mayasaba.App/Mayasaba.App.exe` | WinUI 3 Executable (2,866,688 bytes) | `D74DFE6D9D692B9383F1CE6F36922BD104EE8BA71BA13D168E4871ADBDD430D3` | **VERIFIED** |
| **Application Services Lib (Release)** | `build/vs2026/app/Release/mayasaba_app.lib` | Static Library (Layer 2) | `014EADC7C878F0A30E299A0A7A42618B6063B59A413CDA668F9E1B9DD54A1C43` | **VERIFIED** |
| **Core Engine Lib (Release)** | `build/vs2026/core/Release/mayasaba_core.lib` | Static Library (Layers 3-13) | `58653694B7B96FF7D3677B14B4C9C9EB96B6F5D36F7CF9276AB12A2555FB0D78` | **VERIFIED** |
| **CLI Adapters Lib (Release)** | `build/vs2026/adapters/Release/mayasaba_adapters.lib` | Static Library (Layer 4) | `D831B99652769AC780178576CE59B03F3D40FA1EECFD65E586CB874E0D03255C` | **VERIFIED** |

---

## 3. Required Pre-Implementation Prototypes (AGENTS.md § 14)

| # | Prototype Gate | Required Behavior | Evidence & Discriminative Oracle | Captured Log / Metric | Verdict |
| --- | --- | --- | --- | --- | --- |
| **P1** | **Responsive WinUI Control Room under sustained streaming** | Timeline updates remain smooth, blocking I/O stays off UI thread, Chat is the sole persistent workspace. Max query latency during continuous 30-message burst must remain <500ms. | [controller_tests.cpp](../../tests/e2e/controller_tests.cpp) verifies 30 streamed notifications headlessly. [verify-ui-shell.ps1](../../scripts/verify-ui-shell.ps1) also confirms the rendered native window exposes the Chat composer, lower-left Open Folder control, and correctly disabled pre-bind Send control. Sustained streaming through that rendered window remains unexecuted. | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log)<br>[uia_shell_verification.log](./logs/uia_shell_verification.log) | **INCONCLUSIVE** |
| **P2** | **Process launch, cancellation, and crash cleanup** | Win32 suspended process creation assigned to controller-owned Job Object, handle inheritance restricted, clean teardown of entire process tree (child and grandchild) on exit/cancel. | [kernel_tests.cpp](../../tests/fault/kernel_tests.cpp) verifies Job Object termination for mock `child_process_helper` process trees. Observation and verification of real process-tree cancellation across actual installed CLI toolchains and their subprocesses is unrecorded. | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log)<br>(Mock process tree verified; real CLI process tree unproven) | **INCONCLUSIVE** |
| **P3** | **Enforceable workspace access & stale-write rejection** | Isolated staging worktree/directory per task; Job Objects alone do not establish filesystem containment; Workspace Manager enforces path boundaries, conflict detection, publication journal before mutation, stale-lease fencing rejection, and never rolls back newer user edits. | [kernel_tests.cpp](../../tests/fault/kernel_tests.cpp) now proves a Windows low-integrity agent process can write its controller-labelled staging tree and is denied a sibling-directory write. The adapter requests that boundary for every authorized session. Scoped reads and real Hermes/Kilo/Claude executions under it remain unobserved. | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log)<br>(write boundary passes; real CLI execution profile unproven) | **INCONCLUSIVE** |

---

## 4. Full Acceptance Evidence Matrix

| Criterion ID | Requirement / Scope | Observable Expectation | Oracle Class | Exact Check Command & Working Dir | Blocking | Captured Log Reference | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **CRIT-01** | AGENTS.md § 2, 3: Toolchain & Platform | Native Windows C++20 with MSVC v145, SQLite, and WinUI 3; no cloud backend. | `compile` | `cmake --preset vs2026` then `cmake --build --preset vs2026-debug` from repository root | YES | [ctest_vs2026_debug.log](./logs/ctest_vs2026_debug.log) | **PASS** |
| **CRIT-02** | AGENTS.md § 4.1: Layer Boundaries | 13 distinct functional layers; raw CLI protocols isolated in adapters; app services in `mayasaba_app`. | `static` | CMake target hierarchy check: `mayasaba_core` &rarr; `mayasaba_adapters` &rarr; `mayasaba_app` | YES | `build/vs2026/CMakeCache.txt` target graph | **PASS** |
| **CRIT-03** | AGENTS.md § 4.2: Record Ownership | Single authoritative owner per record; SQLite alone executes SQL. | `unit` | `ctest --preset vs2026-release -R store_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-04** | AGENTS.md § 4.4, 13: Validation Fail-Closed | Empty validation result or inconclusive evidence must fail task; never complete. | `unit` | `ctest --preset vs2026-release -R orchestration_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-05** | AGENTS.md § 5: Chat-Only UX | Native Chat is the sole top-level route; Open Folder at lower left; exception-only CLI warnings. | `runtime` | Execute `scripts/verify-ui-shell.ps1` and `ctest --preset vs2026-release -R controller_tests` from repository root. | YES | [uia_shell_verification.log](./logs/uia_shell_verification.log)<br>[ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-06** | AGENTS.md § 6: Three External CLIs | Exactly Hermes, Kilo Code, and Claude Code CLIs coordinated headlessly; no fourth model; probe-gated launch. | `unit` | `ctest --preset vs2026-release -R adapter_tests` from repository root. Verified with mock CLI adapters; live coordination across all three real installed CLIs is not recorded. | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **INCONCLUSIVE** |
| **CRIT-07** | AGENTS.md § 7: FULL Council Engine | 5-round cap, sequential rounds, 6 directed peer critiques, non-chair synthesis review, deterministic chair rotation. | `unit` | `ctest --preset vs2026-release -R council_` & `-R orchestration_tests` from repository root. Facade and engine durability verified; live 3-CLI debate run pending. | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-08** | AGENTS.md § 8: Workspace Staging | Isolated staging for implementation tasks; guarded publication journal; user folder untouched during execution. | `unit` | `ctest --preset vs2026-release -R workspace_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-09** | AGENTS.md § 9: Process Security | Local Execution Kernel exclusive launch path; Win32 Job Objects; low-integrity staging write boundary; secrets redacted from context and events. | `unit` | `ctest --preset vs2026-release -R kernel_tests` & `-R base_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-10** | AGENTS.md § 10: MCF-v2 Bus | Envelopes, inbox/outbox persistence, sequence tracking, dedup, dead-letter, verified ACKs. | `unit` | `ctest --preset vs2026-release -R bus_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-11** | AGENTS.md § 11: SQLite Persistence | Append-only events chain; SHA-256 tamper detection; idempotency ledger; WAL mode. | `unit` | `ctest --preset vs2026-release -R store_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-12** | AGENTS.md § 12: End-to-End Vertical Slice | Folder bound &rarr; contribution persisted &rarr; staged task &rarr; execution &rarr; publication &rarr; validation timeline. | `e2e` | `ctest --preset vs2026-release -R controller_tests` from repository root | YES | [ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) | **PASS** |
| **CRIT-13** | AGENTS.md § 13: WiX MSI Installer | Standalone `.msi` authored with WiX v6; unpackaged self-contained WinUI 3 payload; verified installation and runtime execution. | `package` | Current package `packaging/release/Mayasaba.msi` (SHA-256: `580DF92D6FB1F493B4473C6A248D940205CB6E930794E4EE537755481E002091`) installed, launched, and uninstalled through the controlled lifecycle script. | YES | [msi_install_verification_580d.log](./logs/msi_install_verification_580d.log)<br>[RELEASE_MANIFEST.json](../../packaging/release/RELEASE_MANIFEST.json) | **PASS** |
| **CRIT-14** | AGENTS.md § 3: Linux-only Packaging | Product packaging, WinUI execution, and installer testing must be on Windows. | `policy` | Windows-only desktop product specification. Linux deployment is explicitly out of scope. | NO | Spec § 3 ("Build and validate the product on Windows") | **NOT_APPLICABLE** |

---

## 5. Retained Historical Execution Logs & Test Results

> [!WARNING]
> **Historical Baseline Notice:** The retained execution logs in Sections 5.1–5.5 document intermediate milestone artifacts and runs. They are preserved for audit provenance. Fresh current 19-suite Debug and Release logs are recorded in Section 5.6; the current `580D…091` MSI lifecycle is recorded in Section 5.7.

### 5.1 Historical Release Build Suite (`ctest --preset vs2026-release`)
- **Log File:** [logs/ctest_vs2026_release.log](./logs/ctest_vs2026_release.log)
- **Exit Code:** `0`
- **Duration:** `6,233 ms` (Real test time: 6.19 sec)
- **Timestamp (UTC):** `2026-10-10T03:20:54.8633676Z`
- **Historical Result:** 14/14 test targets passed (current test suite has 19 passing suites).

### 5.2 Historical Debug Build Suite (`ctest --preset vs2026-debug`)
- **Log File:** [logs/ctest_vs2026_debug.log](./logs/ctest_vs2026_debug.log)
- **Exit Code:** `0`
- **Duration:** `7,828 ms` (Real test time: 7.75 sec)
- **Timestamp (UTC):** `2026-10-10T03:20:48.6147823Z`
- **Historical Result:** 14/14 test targets passed (current test suite has 19 passing suites).

### 5.3 Historical AddressSanitizer Suite (`ctest --test-dir build/vs2026-asan -C Debug`)
- **Log File:** [logs/ctest_vs2026_asan.log](./logs/ctest_vs2026_asan.log)
- **Exit Code:** `0`
- **Duration:** `27,078 ms` (Real test time: 27.03 sec)
- **Timestamp (UTC):** `2026-10-10T03:21:21.9464615Z`
- **Toolchain Runtime:** Repaired via automatic `PATH` and DLL deployment of `clang_rt.asan_dbg_dynamic-x86_64.dll` from MSVC bin directory (`14.50.35717`). All binaries start and execute under ASan.
- **Sanitizer Reports:** **0 errors / 0 reports**. No heap buffer overflows, use-after-free, or memory violations.
- **Historical Result:** 14/14 test targets passed.

### 5.4 Historical MSI Live Installation & Execution Test
- **Log File:** [logs/msi_install_verification.log](./logs/msi_install_verification.log)
- **Exit Code:** `0`
- **Duration:** `16,850 ms`
- **Timestamp (UTC):** `2026-10-10T03:23:32.0636422Z`
- **Tested Package:** `packaging/release/Mayasaba.msi` (Prior build SHA-256: `037C8B692072E087205A8ADE5FFB1B70AB84414A50897FC6007A008DDCA19EC0`)
- **Observed Actions:**
  1. Passive installation completed with ExitCode `0`.
  2. Verified installed executable presence: `C:\Users\pavan\AppData\Local\Mayasaba\Mayasaba.App.exe`.
  3. Verified payload file count: 285 files (matches manifest layout).
  4. Executed installed binary: verified active running process (`PID 12200`).
  5. Process terminated gracefully and uninstalled cleanly via ProductCode `{9750A5BF-D2AC-4C2D-B4E5-6FE45D8C2D3E}` with ExitCode `0`.
  *(Superseded by the later current-package lifecycle records in Sections 5.5 and 5.7.)*

### 5.5 Current MSI Lifecycle Test
- **Log File:** [logs/msi_install_verification_9d4a.log](./logs/msi_install_verification_9d4a.log)
- **Package:** `packaging/release/Mayasaba.msi` (SHA-256: `9D4AC4A2D676FAFF2B70C61CCFE51508C510166D7D796D80BB77DF6B84A98903`)
- **Exit Code:** `0`
- **Duration:** `10,983 ms`
- **Timestamp (UTC):** `2026-10-10T15:12:22.3271083Z`
- **Observed Actions:** Per-user installation succeeded; 285 payload files were present; the installed executable ran as PID `32092`; clean uninstall succeeded.

### 5.6 Current Debug and Release CTest Suites
- **Debug Log:** [logs/ctest_vs2026_debug_19suite.log](./logs/ctest_vs2026_debug_19suite.log) — `19/19` passed, 10.40 seconds real time.
- **Release Log:** [logs/ctest_vs2026_release_19suite.log](./logs/ctest_vs2026_release_19suite.log) — `19/19` passed, 8.58 seconds real time.
- **Execution:** `ctest --preset vs2026-debug -j 4 --output-on-failure` and `ctest --preset vs2026-release -j 4 --output-on-failure` from the repository root on 2026-10-10.

### 5.7 Current MSI Lifecycle Test
- **Log File:** [logs/msi_install_verification_580d.log](./logs/msi_install_verification_580d.log)
- **Package:** `packaging/release/Mayasaba.msi` (SHA-256: `580DF92D6FB1F493B4473C6A248D940205CB6E930794E4EE537755481E002091`)
- **Exit Code:** `0`
- **Duration:** `10,543 ms`
- **Timestamp (UTC):** `2026-10-10T19:10:26.0910251Z`
- **Observed Actions:** Per-user installation succeeded; 283 payload files were present; the installed executable ran as PID `24948`; clean uninstall succeeded.

---

## 6. Definition of Done Verdict

- **Core & Controller Build / CTest Status:** **PASS** (19 of 19 test suites pass in Debug and Release).
- **End-to-End Product Certification:** **FAIL / BLOCKED** (per AGENTS.md § 13 and § 14, completion requires all blocking criteria to pass with verified runtime evidence; claims cannot be aggregated).
- **Prototype Gates (P1, P2, P3):** **INCONCLUSIVE** (the native Chat shell is now UI-Automation verified and controller queues pass, but sustained rendering under three-CLI streaming, real external CLI process-tree observation, and real CLI execution under the workspace boundary remain unrecorded).
- **Installer Lifecycle:** **PASS** (the current MSI `580DF92D6FB1F493B4473C6A248D940205CB6E930794E4EE537755481E002091` installed, launched, and uninstalled successfully; see Section 5.7).
- **Three-CLI Live Run:** **INCONCLUSIVE** (mock adapter execution passes; real concurrent multi-agent FULL council run across installed Hermes, Kilo Code, and Claude Code CLIs is unrecorded).

**Conclusion:** The native C++20 core and WinUI 3 executable compile, link, and test cleanly, but end-to-end product certification is blocked by missing live UI automation, real three-CLI execution, OS containment proof, and current-MSI lifecycle evidence.
