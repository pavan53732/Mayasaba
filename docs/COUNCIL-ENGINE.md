# Mayasaba Council Engine

## Purpose

The Council Engine coordinates the four independent agents as a virtual council for user-authorized local work—including software engineering and other file-based tasks—without turning them into a single shared mind.

## Deliberation model

1. user submits a task brief and selects a local workspace
2. independent analysis by available agents
3. proposals collected
4. cross-critique
5. rebuttal
6. revision
7. disagreement resolution
8. targeted user interview
9. task/product convergence (UX where relevant)
10. solution, tool and architecture debate where relevant
11. acceptance-criteria and validation-plan review
12. decision/lock

Software-engineering tasks retain the full requirements, product/UX, technology and architecture deliberation. Other local artifact tasks use the same independent analysis, critique, user-question and decision principles, but deliberate on only the scope, approach, risks and checks relevant to their acceptance criteria.

## Council messages

IDEA, PROPOSAL, QUESTION, CRITIQUE, COUNTERARGUMENT, REBUTTAL, REVISION, AGREE, DISAGREE, BLOCK, ACCEPT, REJECT, ABSTAIN, DECISION, LOCK

## Council round state

~~~text
OPEN → RESPONSES_COLLECTING → CRITIQUE → REBUTTAL → REVISION → DISAGREEMENT_REVIEW → CLOSING → SEALED
~~~

USER_INPUT_REQUIRED and explicit timeout/non-participation outcomes are supported.

Silence is not agreement.

## Independent-first rule

Agents should produce their initial analysis before exposure to other agents' proposals when the phase is designed for independent reasoning. This reduces anchoring.

## Disagreement handling

Material disagreement is retained as first-class state. Mayasaba does not use simple majority voting as the sole architecture authority.

A material unresolved issue can:

- be resolved through evidence
- be revised by the council
- require a user decision
- block architecture lock

## Question engine

Agent question candidates are:

1. normalized
2. clustered/deduplicated
3. checked against scoped workspace/evidence and, for user-requested research, read-only public sources
4. ranked by impact on the requested artifact, acceptance criteria, safety and delivery
5. batched for the user

User answers are persisted and redistributed to affected agents.

## Decision locks

Decisions contain:

- ID
- subject
- decision
- rationale
- evidence
- alternatives
- authority/status
- affected scope
- supersession chain

HARD_LOCK requires explicit change procedure.

## No private chain of thought

The Control Room displays model-provided messages, conclusions, rationales, tool actions, artifacts and evidence. It does not expose private chain-of-thought.

## Temporary chair

One participating agent may act as temporary council chair/synthesizer for a round. This does not create a fifth intelligence or central reasoning model.
