import { useCallback, useEffect, useState } from "react";

import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { StatusChip } from "../shell/StatusChip";
import {
  getCommunicationHealth,
  getEventCursor,
  replayDeadLetter,
  type CommunicationHealth,
  type EventCursor,
  type ReplayOutcome,
} from "../../intake/bridge";
import type { CommandError, ProjectView } from "../../intake/state";

/*
 * The Logs section: diagnostics, communication health, the durable event cursor and dead-letter replay.
 *
 * CONTROL-ROOM-DESIGN.md, "Event cursor": UI subscriptions use event cursors, and a sequence gap triggers
 * recovery/resync rather than silent local reconstruction. The cursor is read here and a detected gap is
 * surfaced as a gap - the UI never reconstructs the missing range locally.
 *
 * `request_event_resync` is the operation that would act on a gap, and it is declared with no handler
 * (DEC-069 keeps it declared and unimplemented on purpose), so the gap is reported with the operation named
 * rather than with a control that cannot act.
 *
 * `replay_dead_letter` is live. Its refusal is rendered rather than swallowed: a material-action message is
 * refused with `AUTHORIZATION_NOT_IMPLEMENTED` and nothing is enqueued, so a caller must not assume the
 * replay happened.
 */

const CONTROL_ROOM_CONSUMER = "control-room";

export function LogsSection({ project }: { project: ProjectView | null }) {
  const [health, setHealth] = useState<CommunicationHealth | null>(null);
  const [cursor, setCursor] = useState<EventCursor | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const [loading, setLoading] = useState(false);

  const projectId = project?.project_id ?? null;

  const read = useCallback(async () => {
    if (!projectId) return;
    setLoading(true);
    const [h, c] = await Promise.all([
      getCommunicationHealth(projectId),
      getEventCursor(projectId, CONTROL_ROOM_CONSUMER),
    ]);
    if (isError(h)) {
      setError(h);
      setHealth(null);
    } else {
      setHealth(h);
    }
    if (isError(c)) {
      setError(c);
      setCursor(null);
    } else {
      setCursor(c);
    }
    setLoading(false);
  }, [projectId]);

  useEffect(() => {
    void read();
  }, [read]);

  if (!project) {
    return (
      <Card>
        <CardContent className="p-4">
          <p className="text-xs text-ink-muted">
            No project open. Communication health and the event cursor are per project, so there is nothing to
            read until one is open.
          </p>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" onClick={() => void read()} disabled={loading}>
          {loading ? "Reading…" : "Re-read"}
        </Button>
        <span className="text-xs text-ink-muted">
          Read-only. Neither query advances the cursor or changes a queue.
        </span>
      </div>

      {error ? (
        <p role="alert" className="text-xs text-state-danger">
          <strong>{error.code}</strong> — {error.message}
        </p>
      ) : null}

      <Card>
        <CardHeader>
          <CardTitle>Communication health</CardTitle>
          <p className="text-xs text-ink-muted">
            `backlog` and `enqueue_limit` are global while the counts and gaps are per project, because the
            capacity bound the bus enforces is global (DEC-068).
          </p>
        </CardHeader>
        <CardContent>
          {health === null ? (
            <p className="text-xs text-ink-subtle">{loading ? "Reading…" : "Not reported."}</p>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <StatusChip label="Backlog" detail={String(health.backlog)} />
                <StatusChip label="Enqueue limit" detail={String(health.enqueue_limit)} />
                <StatusChip label="Queued" detail={String(health.counts.queued)} />
                <StatusChip label="Retrying" detail={String(health.counts.retrying)} />
                <StatusChip label="Expired" detail={String(health.counts.expired)} />
                <StatusChip
                  tone={health.counts.dead_lettered > 0 ? "warn" : "neutral"}
                  label="Dead-lettered"
                  detail={String(health.counts.dead_lettered)}
                />
                <StatusChip label="Transport" detail={health.transport_status} />
              </div>
              <div>
                <h4 className="mb-1 text-xs font-semibold">Open ordering gaps</h4>
                {health.open_gaps.length === 0 ? (
                  <p className="text-xs text-ink-muted">None reported.</p>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {health.open_gaps.map((gap, index) => (
                      <li key={`${gap.session_id}#${gap.channel}#${index}`} className="font-mono text-xs">
                        {gap.code} · {gap.channel} · expected {gap.expected}, found {gap.found}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Event cursor</CardTitle>
          <p className="text-xs text-ink-muted">
            The durable position of the <code className="font-mono">{CONTROL_ROOM_CONSUMER}</code> consumer.
            A gap is reported as a gap; the UI never reconstructs the missing range locally.
          </p>
        </CardHeader>
        <CardContent>
          {cursor === null ? (
            <p className="text-xs text-ink-subtle">{loading ? "Reading…" : "Not reported."}</p>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <StatusChip
                  tone={cursor.gap_detected ? "warn" : "ok"}
                  label={cursor.gap_detected ? "Gap detected" : "No gap"}
                />
                <StatusChip label="Last sequence" detail={String(cursor.last_sequence)} />
                <StatusChip
                  label="Next sequence"
                  detail={cursor.next_sequence === null ? "not reported" : String(cursor.next_sequence)}
                />
                <StatusChip
                  label="Resync from"
                  detail={cursor.resync_from === null ? "not reported" : String(cursor.resync_from)}
                />
              </div>
              {cursor.gap_detected ? (
                <p className="rounded-md border border-state-warn/40 bg-state-warn-surface px-3 py-2 text-xs text-state-warn">
                  A sequence gap is open. Acting on it requires <code className="font-mono">
                    request_event_resync
                  </code>
                  , which the contract declares and the shell does not register (DEC-069), so this surface
                  reports the gap and offers no control that could not act.
                </p>
              ) : null}
            </div>
          )}
        </CardContent>
      </Card>

      <ReplayCard />
    </div>
  );
}

/**
 * Dead-letter replay.
 *
 * The refusal is the interesting outcome, not the success: a material-action message is refused with
 * `AUTHORIZATION_NOT_IMPLEMENTED` and nothing is enqueued. Rendering the refusal is what stops the surface
 * from implying a replay that did not happen.
 */
function ReplayCard() {
  const [messageId, setMessageId] = useState("");
  const [outcome, setOutcome] = useState<ReplayOutcome | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const [busy, setBusy] = useState(false);

  const onReplay = useCallback(async () => {
    setBusy(true);
    setOutcome(null);
    setError(null);
    const result = await replayDeadLetter(messageId.trim());
    if (isError(result)) {
      setError(result);
    } else {
      setOutcome(result);
    }
    setBusy(false);
  }, [messageId]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Replay a dead-lettered message</CardTitle>
        <p className="text-xs text-ink-muted">
          Re-enqueues the original envelope as a new message. A material-action message is refused and nothing
          is enqueued, so the refusal is shown rather than treated as a replay.
        </p>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="dead-letter-id">Message ID</Label>
          <div className="flex gap-2">
            <Input
              id="dead-letter-id"
              value={messageId}
              onChange={(event) => setMessageId(event.target.value)}
              placeholder="the dead-lettered message's id"
              className="font-mono"
            />
            <Button
              variant="outline"
              className="whitespace-nowrap"
              onClick={() => void onReplay()}
              disabled={busy || messageId.trim() === ""}
            >
              {busy ? "Replaying…" : "Replay"}
            </Button>
          </div>
        </div>

        {outcome ? (
          <div
            role="status"
            className="rounded-md border border-state-ok/40 bg-state-ok-surface px-3 py-2.5 text-xs text-state-ok"
          >
            <strong>Replayed.</strong>
            <dl className="mt-1.5 grid grid-cols-[150px_1fr] gap-x-3 gap-y-1">
              <dt className="text-ink-muted">Replayed message</dt>
              <dd className="font-mono break-all">{outcome.replayed_message_id}</dd>
              <dt className="text-ink-muted">Source message</dt>
              <dd className="font-mono break-all">{outcome.source_message_id}</dd>
              <dt className="text-ink-muted">Context snapshot</dt>
              <dd className="font-mono break-all">{outcome.context_snapshot_id ?? "—"}</dd>
              <dt className="text-ink-muted">State digest</dt>
              <dd className="font-mono break-all">{outcome.state_digest ?? "—"}</dd>
              <dt className="text-ink-muted">Context refreshed</dt>
              <dd>{outcome.context_refreshed ? "yes" : "no — the original envelope's context is carried"}</dd>
              <dt className="text-ink-muted">Deduplicated</dt>
              <dd>
                {outcome.deduplicated
                  ? "yes — an earlier replay was found and no new message was produced"
                  : "no"}
              </dd>
            </dl>
          </div>
        ) : null}

        {error ? (
          <div
            role="alert"
            className="rounded-md border border-state-warn/40 bg-state-warn-surface px-3 py-2.5 text-xs text-state-warn"
          >
            <strong>Not replayed.</strong>
            <div className="mt-1">
              <code className="font-mono">{error.code}</code> — {error.message}
            </div>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

function isError<T>(value: T | CommandError): value is CommandError {
  return typeof (value as CommandError).code === "string";
}
