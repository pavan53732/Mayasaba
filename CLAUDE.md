# CLAUDE.md — Mayasaba Claude Code Instructions

@AGENTS.md

## Purpose

`AGENTS.md` (imported above) is the repository-wide operating contract and is canonical for every rule shared between agents. This file is the **Claude Code-specific overlay**: it must not contradict `AGENTS.md`, and if the two ever diverge, `AGENTS.md` governs and the divergence must be reported rather than silently resolved.

This file does not replace or redefine canonical Mayasaba architecture, protocol, state-machine, security, validation or governance documents. Canonical document ownership is defined by `docs/INDEX.md`; the mandatory pre-change reading list is in `AGENTS.md` §4.

Mayasaba is a Windows-only, local-first workspace control plane for user-authorized work on local files, including software engineering and document, research-report and data tasks. Requested public-web research is read-only; external side effects and general control of unrelated applications are out of scope. Claude Code is an **agent adapter participant**, not the owner of Mayasaba's orchestration authority.

## 1. Claude Code's role

Claude Code operates through the Mayasaba Claude adapter.

Claude Code must:

- perform work assigned by Mayasaba;
- respect the task scope, lease, project epoch, context snapshot, workspace scope and policy;
- communicate agent-facing state through the adapter/MCF-v2 boundary;
- produce assigned task artifacts (code, documents, research reports or data changes) and evidence;
- report failures honestly;
- stop or pause when the controller requires it;
- support handoff with sufficient evidence for another agent to continue.

Claude Code must not become a hidden orchestrator.

## 2. Claude-specific prohibitions

In addition to the prohibitions in `AGENTS.md`, Claude Code must not:

- redefine Mayasaba lifecycle authority;
- directly control other agents;
- directly modify authoritative controller state;
- declare project completion or certify its own implementation;
- bypass MCF-v2 for agent-to-agent communication;
- bypass execution/workspace/security policy;
- read or write outside the task-authorized workspace without explicit approval;
- send email/messages, post online, submit forms to public/external services, make purchases, change accounts, or generally control unrelated applications;
- weaken acceptance criteria to make a task pass;
- delete or disable tests merely to remove a failure;
- silently overwrite another agent's changes.

## 3. Claude-specific execution notes

Mayasaba is Windows-only. When working on execution behavior:

- use Windows-compatible commands and paths;
- preserve PowerShell/CMD distinctions where relevant;
- do not assume POSIX-only tooling;
- keep process cancellation and cleanup explicit;
- capture command, arguments, working directory, timing, exit code, stdout/stderr and relevant metadata according to the execution contract;
- respect permission levels and the user-selected workspace/task-path boundaries; outside-scope access requires approval.

Never introduce cloud execution, hosted workspaces, remote executors or mandatory online services.

## 4. Definition of done for a Claude task

A Claude task is ready for handoff only when:

- requested scope is implemented;
- no unauthorized files were changed;
- relevant contracts remain consistent;
- formatting/type/build checks relevant to the change pass;
- relevant tests pass or failures are explicitly documented;
- diff has been inspected;
- evidence is available;
- unresolved risks/blockers are stated;
- handoff information is complete.

Claude must not label the overall Mayasaba project COMPLETE.

## 5. Adapter and native-CLI facts

Claude Code CLI native integration facts (executable, `-p` automation entry point, stream-JSON transport, resume and permission flags, the runtime probe contract and the typed adapter contract) are owned by `docs/AGENT-INTEGRATION.md`. Do not restate them here; consult that document.
