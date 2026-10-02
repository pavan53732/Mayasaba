# Mayasaba Council Engine

## Purpose

The Council Engine coordinates the three agents as a virtual council for user-authorized local work—including software engineering and other file-based tasks—without turning them into a single shared mind.

The three sessions are architecturally separate, but they are not three independent implementations: Kilo Code CLI is a fork of OpenCode CLI. Council deliberation must not treat agreement between Kilo and OpenCode as independent corroboration. Model/provider diversity across the three remains real and is the diversity the council relies on. See `ARCHITECTURE.md` §2.

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

IDEA, PROPOSAL, QUESTION, CRITIQUE, COUNTERARGUMENT, REBUTTAL, REVISION, AGREE, DISAGREE, BLOCK, ACCEPT, REJECT, ABSTAIN, DECISION, LOCK, SYNTHESIS

### Position and review-target semantics

Council contributions travel in the existing MCF-v2 envelope on the `council` channel. The envelope's `sender`, `recipients`, `message_type`, `round_id`, `phase`, project epoch and context references identify who sent the contribution, who should receive it, what action it represents and which round/context it belongs to. The payload remains the existing `CouncilRound` aggregate validated by `schemas/mcf-v2/council-round.schema.json`; this section defines the meaning of its currently generic `positions` and `participants` objects. It adds no MCF-v2 message type, envelope field or payload-schema field.

Each persisted agent position is immutable and carries these semantic fields:

- `position_id`: stable UUID for this contribution;
- `author_agent_id` and `source_message_id`: must match the MCF sender and source envelope;
- `position_type`: must match the MCF `message_type`;
- `body`: concise, user-visible conclusion and rationale, not private chain-of-thought;
- `responds_to_position_ids`: IDs of earlier positions this contribution addresses; this is empty for an initial IDEA/PROPOSAL and required for CRITIQUE, COUNTERARGUMENT, REBUTTAL, REVISION, AGREE, DISAGREE, BLOCK, ACCEPT, REJECT, ABSTAIN, SYNTHESIS and agent DECISION/LOCK candidates; a QUESTION retains its source position or task reference;
- `claims`: claim ID and statement, with `evidence_refs` plus relevant requirement/contract references; an unsupported claim must be labeled as an assumption rather than presented as verified evidence;
- `supersedes_position_id`: optional reference when a revision replaces an earlier position.

Message-type rules:

| MCF type | Council meaning and required linkage |
|---|---|
| `IDEA`, `PROPOSAL` | Opens a position; `responds_to_position_ids` is empty. |
| `CRITIQUE` | Evidence-based review of a controller-assigned proposal; cites its assigned target ID(s). |
| `COUNTERARGUMENT`, `REBUTTAL` | Challenges or answers prior reasoning; cites the position(s) it addresses. |
| `REVISION` | Presents an updated position; cites the positions it answers and sets `supersedes_position_id` when replacing one. |
| `AGREE`, `DISAGREE`, `BLOCK`, `ACCEPT`, `REJECT`, `ABSTAIN` | Records a reasoned stance on one or more referenced positions; it is not itself a controller decision. |
| `QUESTION` | Creates a CouncilService question candidate linked to its source position; it is not an automatic user prompt. |
| `DECISION`, `LOCK` | Agent-submitted candidates only; neither creates an authoritative decision nor a HARD_LOCK. |
| `SYNTHESIS` | A merge of two or more referenced positions, submitted only by the round's temporary chair/synthesizer. Cites every contributing position in `responds_to_position_ids`. It is a proposed merge, not an authoritative decision, and it does not itself resolve a disagreement. |

Transport ACK/NACK reports delivery state, not agreement or a council stance.

Before a critique/rebuttal phase, CouncilService freezes the positions to review and assigns `review_target_position_ids` to each participant in that round's participant context. MCF `recipients` routes the round to the assigned agent; the agent's response must cite the assigned position IDs in `responds_to_position_ids`. By default, every proposal receives review from at least one other available agent; high-risk or contract-sensitive proposals are sent to all other available participants. Self-review does not count. Missing participants are recorded as non-participation, never fabricated as agreement.

A QUESTION creates a question candidate for CouncilService to assess and cluster; it is not automatically shown to the user. Agent-sent DECISION or LOCK messages are candidates only: only the authorized controller/user decision process can persist an authoritative decision or HARD_LOCK.

The CouncilService validates these semantics before persisting a position. The generic nested MCF schema is transport compatibility, not permission to accept arbitrary or untraceable council content.

## Council round state

~~~text
OPEN → RESPONSES_COLLECTING → CRITIQUE → REBUTTAL → REVISION → DISAGREEMENT_REVIEW → CLOSING → SEALED
~~~

USER_INPUT_REQUIRED and explicit timeout/non-participation outcomes are supported.

Silence is not agreement.

### Termination

A round ends for exactly one reason, recorded as the round's `outcome_type`:

| Outcome | Meaning |
|---|---|
| `CONVERGED` | The position set reached a fixpoint: a full round produced no new or changed position. |
| `SYNTHESIZED` | The round closed on a chair-submitted `SYNTHESIS` that merges the surviving positions. |
| `CAP_REACHED` | `round_index` reached `max_rounds` before convergence. |
| `ESCALATED` | Material disagreement survived review and requires a user decision. |
| `SEALED_WITH_OPEN_QUESTION` | The round sealed with a question still open; the project shows WAITING_FOR_USER. |

Termination uses convergence first and the round cap as a backstop. Convergence is a fixpoint test over the position set, not a count of agreeing agents: this is what keeps termination consistent with the rule below that simple majority voting is never the sole architecture authority. The cap (`max_rounds`, default 5) guarantees termination when the fixpoint test never fires; reaching the cap is not by itself evidence of disagreement, so `CAP_REACHED` is distinct from `ESCALATED`.

### Escalation

When a round ends `ESCALATED`, CouncilService emits `COUNCIL_USER_INPUT_REQUIRED` and persists a structured escalation packet validated by `schemas/council-v1/escalation.schema.json`. The packet carries the competing positions with their strongest arguments, a conflict matrix naming the requirement IDs actually in dispute and each side's stance, a non-authoritative default recommendation, and a timeout behavior fixed at `PAUSE`.

The packet's recommendation is advisory: it is not a decision, and it never carries authority. `timeout_behavior` is constrained to `PAUSE` by the schema so that no answer, timeout or silence can be converted into assent.

A round that ends `SEALED_WITH_OPEN_QUESTION` links its question to the packet through the question's `escalation_ref`.

## Independent-first rule

Agents should produce their initial analysis before exposure to other agents' proposals when the phase is designed for independent reasoning. This reduces anchoring.

## Disagreement handling

Material disagreement is retained as first-class state. Mayasaba does not use simple majority voting as the sole architecture authority.

A material unresolved issue can:

- be resolved through evidence
- be revised by the council
- require a user decision
- block architecture lock

## Discovery and user-interview gates

Mayasaba has two distinct user-facing gates. They do not overlap.

DISCOVERY is pre-analysis and brief-oriented. It establishes the ProjectBrief baseline, performs objective local discovery (workspace path, repository characteristics, project type, existing files, available toolchain, detected constraints) and determines whether the brief is complete enough to begin independent analysis. It may surface gaps in the user's own brief, but it is not a council interview and does not run agent-generated questioning.

USER_INTERVIEW is post-deliberation and question-oriented. It resolves material questions produced by the council, and it is driven entirely by the question engine below. Only questions that survive normalization, deduplication, evidence-checking and impact ranking reach the user.

A user-facing clarification therefore reaches the council at exactly one of two points: before independent analysis (DISCOVERY, about the brief) or after deliberation (USER_INTERVIEW, about a council question). Free-text user input submitted outside these gates is recorded as a `UserContribution` and never bypasses this flow; see `MEMORY-CONTEXT.md`.

## Question engine

An agent QUESTION is a candidate, not an automatic user interruption. CouncilService:

1. normalizes it and preserves its source agent/position;
2. clusters and deduplicates equivalent questions;
3. checks scoped workspace/evidence and, for user-requested research, read-only public sources;
4. ranks impact on the requested artifact, acceptance criteria, safety and delivery;
5. asks the user only when the material point remains unresolved, in a batched Control Room prompt.

## User-answer and context handoff

The Control Room submits a response through the existing `answer_user_question` command. CouncilService verifies that the question is open and belongs to the active project/round, persists an immutable `UserAnswer` using `schemas/council-v1/answer.schema.json`, and changes the question to ANSWERED. A response is not automatically a requirement, decision, HARD_LOCK or permission grant; the owning service records any project-truth change through its normal command.

`redistribution_scope`, when supplied, must be within the question's affected agents and project participants. When omitted, CouncilService uses the question's `affected_agents`; it must not silently broadcast to unrelated project agents. If the answer changes material project truth, the owning service updates it and the project epoch advances under the context rules. ContextService then creates a new immutable ContextPack and digest for the affected agents. Even when the epoch does not change, a changed pack gets a new snapshot/digest. Earlier snapshots remain historical and cannot authorize writes after becoming stale.

The answer itself is not a new MCF-v2 message type. Mayasaba distributes the updated snapshot with the existing `CONTEXT_UPDATE` message; when an answer is recorded as a formal decision, the existing `DECISION` message may accompany it. Agents resume only against the new snapshot and digest. ACK is receipt only; stale-context and lease gates remain in force.

A round that requires user input closes/seals with its question still open; the project is displayed as WAITING_FOR_USER. After the answer is persisted and redistributed, CouncilService opens a linked revision/continuation round. No answer, timeout or silence is converted into assent.

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

The chair may submit a `SYNTHESIS` merging positions from other participants. A synthesis is a proposed merge: it cites every contributing position, it is not an authoritative decision, and it does not by itself close a material disagreement. The chair holds no vote, tie-break or override authority, and a synthesis by the chair of its own position alone is not a synthesis.
