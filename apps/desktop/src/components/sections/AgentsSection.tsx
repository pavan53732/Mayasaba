import { useCallback, useEffect, useState } from "react";

import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { StatusChip } from "../shell/StatusChip";
import { getAgentStatus, type AgentPerformanceReport } from "../../intake/bridge";
import type { CommandError, ProjectView } from "../../intake/state";

/*
 * The Agents section.
 *
 * CONTROL-ROOM-DESIGN.md, "Agent/council views": the agent view shows runtime identity, session, capabilities,
 * health, active task, lease and recent evidence.
 *
 * `get_agent_status` is live and supplies the attempt history. The rest of that list - session, capabilities,
 * active task, lease, recent evidence - is supplied by operations the contract declares and the shell does not
 * register, so they are named by `SectionView` rather than rendered as blanks here.
 *
 * The report is descriptive by contract: `informational_only` is stated on every report because the guarantee
 * is the point of the report. The Control Room renders it as an observation and never as a routing input or a
 * health verdict, and it renders a withheld rate as withheld rather than as zero.
 */

export function AgentsSection({ project }: { project: ProjectView | null }) {
  const [reports, setReports] = useState<AgentPerformanceReport[] | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const [loading, setLoading] = useState(false);

  const projectId = project?.project_id ?? null;

  const read = useCallback(async () => {
    if (!projectId) return;
    setLoading(true);
    const result = await getAgentStatus(projectId);
    if (isError(result)) {
      setError(result);
      setReports(null);
    } else {
      setError(null);
      setReports(result);
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
            No project open. Agent attempts are recorded per project, so there is nothing to read until one is
            open.
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
        <span className="text-xs text-ink-muted">Read-only. This report never affects routing or authority.</span>
      </div>

      {error ? (
        <p role="alert" className="text-xs text-state-danger">
          <strong>{error.code}</strong> — {error.message}
        </p>
      ) : null}

      {reports === null ? (
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-ink-subtle">{loading ? "Reading…" : "Not reported."}</p>
          </CardContent>
        </Card>
      ) : reports.length === 0 ? (
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-ink-muted">
              No agent attempts recorded for this project. This is a normal state before any agent has run.
            </p>
          </CardContent>
        </Card>
      ) : (
        reports.map((report) => <AgentCard key={report.agent_id} report={report} />)
      )}
    </div>
  );
}

function AgentCard({ report }: { report: AgentPerformanceReport }) {
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <CardTitle className="font-mono text-xs break-all">{report.agent_id}</CardTitle>
          <StatusChip label="Attempts" detail={String(report.sample_size)} />
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <dl className="grid grid-cols-[170px_1fr] gap-x-3 gap-y-1.5 text-xs">
          <dt className="text-ink-muted">Attempts with recorded failure</dt>
          <dd>{report.attempts_with_recorded_failure}</dd>

          {/* A withheld rate is rendered as withheld, with the reason the policy gave. Substituting zero would
              turn the absence of a measurement into a measurement. */}
          <dt className="text-ink-muted">Failure rate</dt>
          <dd>
            {report.failure_rate === null ? (
              <span className="text-ink-muted">
                withheld — {report.failure_rate_suppressed_reason ?? "reason not reported"}
              </span>
            ) : (
              report.failure_rate
            )}
          </dd>

          <dt className="text-ink-muted">Validation survival</dt>
          <dd>
            {report.validation_survival}
            <span className="block text-ink-muted">{report.validation_survival_reason}</span>
          </dd>
        </dl>

        {report.by_state.length > 0 ? (
          <div>
            <h4 className="mb-1.5 text-xs font-semibold">Attempts by state</h4>
            <div className="flex flex-wrap gap-1.5">
              {report.by_state.map((count) => (
                <StatusChip key={count.state} label={count.state} detail={String(count.count)} />
              ))}
            </div>
          </div>
        ) : null}

        {report.informational_only ? (
          <p className="rounded-md border border-border bg-surface-sunken px-3 py-2 text-[11px] leading-relaxed text-ink-muted">
            Descriptive only. This report must not affect routing, thresholds, mode selection or authority.
          </p>
        ) : null}
      </CardContent>
    </Card>
  );
}

function isError<T>(value: T | CommandError): value is CommandError {
  return typeof (value as CommandError).code === "string";
}
