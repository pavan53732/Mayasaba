import { useEffect, useState } from "react";

import { RailField, StatusChip, type Tone } from "./StatusChip";
import { Separator } from "../ui/separator";
import { ScrollArea } from "../ui/scroll-area";
import {
  getAgentStatus,
  getCommunicationHealth,
  getEventCursor,
  type AgentPerformanceReport,
  type CommunicationHealth,
  type EventCursor,
} from "../../intake/bridge";
import type { CommandError, ProjectView } from "../../intake/state";

/*
 * The right context rail.
 *
 * CONTROL-ROOM-DESIGN.md, "Layout": the rail shows the user-selected workspace/task scope, active agents,
 * tasks, blockers, task-applicable build/test or artifact-validation health, communication health, epoch,
 * context snapshot, lease expiry, barriers, retries and dead letters.
 *
 * Three of those are readable today - communication health, the durable event cursor and agent reports - and
 * they are read here. The rest are named with the operation that would supply them and rendered as not
 * reported, because the alternative is a rail that shows a plausible zero for a queue depth nobody measured.
 *
 * Nothing in this component is stored: each value is read from Rust on mount and on project change, and the
 * rail keeps no second copy that could disagree with the authority (AGENTS.md section 12).
 */

/** The consumer whose cursor the rail reads. The Control Room is one consumer of the project's event stream. */
const CONTROL_ROOM_CONSUMER = "control-room";

type Loaded<T> =
  /** No project is open, so there is nothing to read. Distinct from `loading`, which means a read is in flight. */
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "loaded"; value: T }
  | { kind: "failed"; error: CommandError };

export function ContextRail({ project }: { project: ProjectView | null }) {
  const [health, setHealth] = useState<Loaded<CommunicationHealth>>({ kind: "idle" });
  const [cursor, setCursor] = useState<Loaded<EventCursor>>({ kind: "idle" });
  const [agents, setAgents] = useState<Loaded<AgentPerformanceReport[]>>({ kind: "idle" });

  useEffect(() => {
    if (!project) {
      // Every read below is per project, so with none open the rail reports that rather than staying in a
      // loading state that would read as a read still in flight.
      setHealth({ kind: "idle" });
      setCursor({ kind: "idle" });
      setAgents({ kind: "idle" });
      return;
    }
    let cancelled = false;
    setHealth({ kind: "loading" });
    setCursor({ kind: "loading" });
    setAgents({ kind: "loading" });

    void (async () => {
      const [h, c, a] = await Promise.all([
        getCommunicationHealth(project.project_id),
        getEventCursor(project.project_id, CONTROL_ROOM_CONSUMER),
        getAgentStatus(project.project_id),
      ]);
      if (cancelled) return;
      setHealth(isError(h) ? { kind: "failed", error: h } : { kind: "loaded", value: h });
      setCursor(isError(c) ? { kind: "failed", error: c } : { kind: "loaded", value: c });
      setAgents(isError(a) ? { kind: "failed", error: a } : { kind: "loaded", value: a });
    })();

    return () => {
      cancelled = true;
    };
  }, [project]);

  return (
    <aside
      aria-label="Context"
      className="flex h-full min-h-0 flex-col border-l border-border bg-surface-raised"
    >
      <div className="border-b border-border px-3 py-2">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted">Context</h2>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-4 p-3">
          <Scope project={project} />
          <Separator />
          <Communication health={health} />
          <Separator />
          <Cursor cursor={cursor} />
          <Separator />
          <Agents agents={agents} />
          <Separator />
          <NotReported />
        </div>
      </ScrollArea>
    </aside>
  );
}

function Scope({ project }: { project: ProjectView | null }) {
  return (
    <section aria-label="Scope">
      <h3 className="mb-2 text-[11px] font-medium uppercase tracking-wide text-ink-subtle">Scope</h3>
      {project ? (
        <dl className="flex flex-col gap-2">
          <RailField label="Workspace" value={project.local_path} mono />
          <RailField label="Phase" value={project.phase} />
          <RailField label="Status" value={project.status} />
          <RailField label="Epoch" value={project.current_epoch} />
          <RailField label="Brief" value={project.brief_id} mono />
          <RailField label="Brief version" value={project.brief_version} />
        </dl>
      ) : (
        <p className="text-xs text-ink-muted">
          No project open, so there is no authorized scope to show. The whole PC is never an implicit scan
          scope.
        </p>
      )}
    </section>
  );
}

function Communication({ health }: { health: Loaded<CommunicationHealth> }) {
  return (
    <section aria-label="Communication health">
      <h3 className="mb-2 text-[11px] font-medium uppercase tracking-wide text-ink-subtle">Communication</h3>
      {health.kind === "idle" ? (
        <p className="text-xs text-ink-muted">No project open, so there is nothing to read.</p>
      ) : health.kind === "loading" ? (
        <p className="text-xs text-ink-subtle">Reading…</p>
      ) : health.kind === "failed" ? (
        <Failure error={health.error} />
      ) : (
        <dl className="flex flex-col gap-2">
          <RailField label="Backlog" value={health.value.backlog} />
          <RailField label="Enqueue limit" value={health.value.enqueue_limit} />
          <RailField label="Queued" value={health.value.counts.queued} />
          <RailField label="Retrying" value={health.value.counts.retrying} />
          <RailField label="Expired" value={health.value.counts.expired} />
          <RailField label="Dead-lettered" value={health.value.counts.dead_lettered} />
          <RailField label="Transport" value={health.value.transport_status} />
          <div className="flex flex-col gap-0.5">
            <dt className="text-[11px] uppercase tracking-wide text-ink-subtle">Open gaps</dt>
            <dd className="text-xs">
              {health.value.open_gaps.length === 0 ? (
                "none"
              ) : (
                <ul className="flex flex-col gap-1">
                  {health.value.open_gaps.map((gap, index) => (
                    <li key={`${gap.session_id}#${gap.channel}#${index}`} className="font-mono text-[11px]">
                      {gap.code} {gap.channel} expected {gap.expected}, found {gap.found}
                    </li>
                  ))}
                </ul>
              )}
            </dd>
          </div>
        </dl>
      )}
    </section>
  );
}

function Cursor({ cursor }: { cursor: Loaded<EventCursor> }) {
  const tone: Tone = cursor.kind === "loaded" && cursor.value.gap_detected ? "warn" : "neutral";
  return (
    <section aria-label="Event cursor">
      <div className="mb-2 flex items-center justify-between gap-2">
        <h3 className="text-[11px] font-medium uppercase tracking-wide text-ink-subtle">Event cursor</h3>
        {cursor.kind === "loaded" ? (
          <StatusChip
            tone={tone}
            label={cursor.value.gap_detected ? "Gap detected" : "No gap"}
          />
        ) : null}
      </div>
      {cursor.kind === "idle" ? (
        <p className="text-xs text-ink-muted">No project open, so there is nothing to read.</p>
      ) : cursor.kind === "loading" ? (
        <p className="text-xs text-ink-subtle">Reading…</p>
      ) : cursor.kind === "failed" ? (
        <Failure error={cursor.error} />
      ) : (
        <dl className="flex flex-col gap-2">
          <RailField label="Consumer" value={cursor.value.consumer_id} mono />
          <RailField label="Last sequence" value={cursor.value.last_sequence} />
          <RailField label="Next sequence" value={cursor.value.next_sequence} />
          <RailField label="Resync from" value={cursor.value.resync_from} />
        </dl>
      )}
    </section>
  );
}

function Agents({ agents }: { agents: Loaded<AgentPerformanceReport[]> }) {
  return (
    <section aria-label="Agents">
      <h3 className="mb-2 text-[11px] font-medium uppercase tracking-wide text-ink-subtle">Agents</h3>
      {agents.kind === "idle" ? (
        <p className="text-xs text-ink-muted">No project open, so there is nothing to read.</p>
      ) : agents.kind === "loading" ? (
        <p className="text-xs text-ink-subtle">Reading…</p>
      ) : agents.kind === "failed" ? (
        <Failure error={agents.error} />
      ) : agents.value.length === 0 ? (
        <p className="text-xs text-ink-muted">No agent attempts recorded for this project.</p>
      ) : (
        <ul className="flex flex-col gap-3">
          {agents.value.map((report) => (
            <li key={report.agent_id} className="flex flex-col gap-1.5">
              <span className="font-mono text-[11px] break-all">{report.agent_id}</span>
              <dl className="flex flex-col gap-1.5">
                <RailField label="Attempts" value={report.sample_size} />
                <RailField label="With failure" value={report.attempts_with_recorded_failure} />
                {/* A withheld rate is rendered as withheld with its reason. Substituting zero would turn the
                    absence of a measurement into a measurement. */}
                <RailField
                  label="Failure rate"
                  value={
                    report.failure_rate === null
                      ? `withheld — ${report.failure_rate_suppressed_reason ?? "reason not reported"}`
                      : report.failure_rate
                  }
                />
                <RailField label="Validation survival" value={report.validation_survival} />
              </dl>
              {report.by_state.length > 0 ? (
                <div className="flex flex-wrap gap-1">
                  {report.by_state.map((count) => (
                    <StatusChip key={count.state} label={count.state} detail={String(count.count)} />
                  ))}
                </div>
              ) : null}
              {report.informational_only ? (
                <p className="text-[11px] leading-relaxed text-ink-subtle">
                  Descriptive only. This report does not affect routing, thresholds or authority.
                </p>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * The rail fields the contract declares and no handler supplies.
 *
 * Named with the operation that would answer them, so the gap is legible as unfinished work rather than as a
 * value that happens to be missing. `get_action_admissibility` is the one that matters most: it is what the
 * design document says the Control Room reads before enabling a mutating action, and without it every
 * mutating control in this shell is advisory only.
 */
function NotReported() {
  const fields: Array<[string, string]> = [
    ["Tasks", "get_task_graph"],
    ["Blockers", "get_task_graph"],
    ["Lease expiry", "get_task_graph"],
    ["Barriers", "get_council_round"],
    ["Retries", "get_repairs"],
    ["Context snapshot", "get_context_status"],
    ["Build health", "get_build_status"],
    ["Test health", "get_test_runs"],
    ["Action admissibility", "get_action_admissibility"],
  ];
  return (
    <section aria-label="Not reported">
      <h3 className="mb-2 text-[11px] font-medium uppercase tracking-wide text-ink-subtle">Not reported</h3>
      <p className="mb-2 text-[11px] leading-relaxed text-ink-muted">
        These are declared by the bridge contract and have no registered handler, so the rail cannot read
        them. They are named rather than shown as zero.
      </p>
      <ul className="flex flex-col gap-1">
        {fields.map(([label, operation]) => (
          <li key={label} className="flex items-baseline justify-between gap-2 text-[11px]">
            <span className="text-ink-muted">{label}</span>
            <code className="font-mono text-ink-subtle">{operation}</code>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Failure({ error }: { error: CommandError }) {
  return (
    <p role="alert" className="text-xs text-state-danger">
      <strong>{error.code}</strong> — {error.message}
    </p>
  );
}

/**
 * True when the service answered with a refusal rather than a value.
 *
 * A `CommandError` always carries a string `code` and no view does, so this distinguishes the two by the shape
 * the contract gives each rather than by which fields happen to be present.
 */
function isError<T>(value: T | CommandError): value is CommandError {
  return typeof (value as CommandError).code === "string";
}
