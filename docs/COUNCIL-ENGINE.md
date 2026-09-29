# Mayasaba Council Engine

## Purpose

The Council Engine coordinates the four independent agents as a virtual engineering council without turning them into a single shared mind.

## Deliberation model

1. user submits project idea
2. independent analysis by available agents
3. proposals collected
4. cross-critique
5. rebuttal
6. revision
7. disagreement resolution
8. targeted user interview
9. product/UX convergence
10. technology/architecture debate
11. architecture review
12. decision/lock

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
3. checked against repository/evidence
4. ranked by implementation impact
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
