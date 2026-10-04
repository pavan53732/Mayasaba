PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projects (
  project_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  local_path TEXT NOT NULL,
  phase TEXT NOT NULL,
  status TEXT NOT NULL,
  current_epoch INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_epochs (
  project_id TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(project_id, epoch),
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

-- ProjectPath and ProjectBrief are core entities in docs/DATA-MODEL.md. Before these tables existed the
-- data model asserted durable entities this schema did not implement, and contract verification could not
-- see it because nothing read schema.sql. verify.mjs now resolves every DATA-MODEL core entity to a table,
-- a declared column, or a declared alias, so a documented entity without storage fails the gate.
CREATE TABLE IF NOT EXISTS project_paths (
  path_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  path TEXT NOT NULL,
  purpose TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(project_id, path),
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS project_briefs (
  brief_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  body TEXT NOT NULL,
  source TEXT NOT NULL,
  supersedes_brief_id TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(project_id, version),
  FOREIGN KEY(project_id) REFERENCES projects(project_id),
  FOREIGN KEY(supersedes_brief_id) REFERENCES project_briefs(brief_id)
);

CREATE TABLE IF NOT EXISTS agents (
  agent_id TEXT PRIMARY KEY,
  agent_type TEXT NOT NULL,
  executable TEXT NOT NULL,
  resolved_path TEXT,
  version TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_sessions (
  session_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  native_session_id TEXT,
  state TEXT NOT NULL,
  health_state TEXT NOT NULL,
  process_id INTEGER,
  workspace_id TEXT,
  capability_snapshot_id TEXT,
  current_epoch INTEGER NOT NULL,
  started_at TEXT,
  stopped_at TEXT,
  FOREIGN KEY(project_id) REFERENCES projects(project_id),
  FOREIGN KEY(agent_id) REFERENCES agents(agent_id)
);

CREATE TABLE IF NOT EXISTS agent_capabilities (
  capability_snapshot_id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL,
  session_id TEXT,
  capabilities_json TEXT NOT NULL,
  detected_at TEXT NOT NULL,
  FOREIGN KEY(agent_id) REFERENCES agents(agent_id)
);

CREATE TABLE IF NOT EXISTS council_sessions (
  council_session_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at TEXT NOT NULL,
  closed_at TEXT,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS council_rounds (
  round_id TEXT PRIMARY KEY,
  council_session_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  state TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  sealed_at TEXT,
  FOREIGN KEY(council_session_id) REFERENCES council_sessions(council_session_id),
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS council_participants (
  round_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  session_id TEXT,
  participation_state TEXT NOT NULL,
  PRIMARY KEY(round_id, agent_id),
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id),
  FOREIGN KEY(agent_id) REFERENCES agents(agent_id)
);

CREATE TABLE IF NOT EXISTS council_positions (
  position_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  message_id TEXT,
  position_type TEXT NOT NULL,
  body_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id)
);

CREATE TABLE IF NOT EXISTS council_questions (
  question_id TEXT PRIMARY KEY,
  round_id TEXT,
  project_id TEXT NOT NULL,
  source_agent_id TEXT,
  status TEXT NOT NULL,
  body TEXT NOT NULL,
  normalized_key TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS council_outcomes (
  outcome_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  outcome_type TEXT NOT NULL,
  body_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id)
);

CREATE TABLE IF NOT EXISTS requirements (
  requirement_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  statement TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  supersedes_requirement_id TEXT,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS requirement_acceptance (
  acceptance_id TEXT PRIMARY KEY,
  requirement_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  predicate_json TEXT NOT NULL,
  FOREIGN KEY(requirement_id) REFERENCES requirements(requirement_id)
);

CREATE TABLE IF NOT EXISTS decisions (
  decision_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  class TEXT NOT NULL,
  status TEXT NOT NULL,
  subject TEXT NOT NULL,
  decision_text TEXT NOT NULL,
  rationale TEXT,
  created_at TEXT NOT NULL,
  supersedes_decision_id TEXT,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS architecture_artifacts (
  architecture_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  kind TEXT NOT NULL,
  body_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS contracts (
  contract_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  kind TEXT NOT NULL,
  body_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS context_snapshots (
  context_snapshot_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  scope TEXT NOT NULL,
  state_digest TEXT NOT NULL,
  pack_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  superseded_at TEXT,
  invalidated_at TEXT,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS tasks (
  task_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  objective TEXT NOT NULL,
  status TEXT NOT NULL,
  priority INTEGER NOT NULL,
  risk TEXT NOT NULL,
  workspace_id TEXT,
  current_epoch INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS task_dependencies (
  task_id TEXT NOT NULL,
  depends_on_task_id TEXT NOT NULL,
  PRIMARY KEY(task_id, depends_on_task_id),
  FOREIGN KEY(task_id) REFERENCES tasks(task_id),
  FOREIGN KEY(depends_on_task_id) REFERENCES tasks(task_id)
);

CREATE TABLE IF NOT EXISTS task_leases (
  lease_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  lease_version INTEGER NOT NULL,
  project_epoch INTEGER NOT NULL,
  context_snapshot_id TEXT NOT NULL,
  state_digest TEXT NOT NULL,
  allowed_paths_json TEXT NOT NULL,
  required_capabilities_json TEXT NOT NULL,
  policy_scope TEXT NOT NULL,
  issued_at TEXT NOT NULL,
  heartbeat_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  status TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES tasks(task_id)
);

CREATE TABLE IF NOT EXISTS handoffs (
  handoff_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  sender_agent_id TEXT NOT NULL,
  sender_session_id TEXT NOT NULL,
  receiver_agent_id TEXT NOT NULL,
  state TEXT NOT NULL,
  package_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
  workspace_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  root_path TEXT NOT NULL,
  agent_id TEXT,
  branch_name TEXT,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS workspace_checkpoints (
  checkpoint_id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  task_id TEXT,
  agent_id TEXT,
  session_id TEXT,
  epoch INTEGER NOT NULL,
  kind TEXT NOT NULL,
  repository_head TEXT,
  diff_hash TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
);

CREATE TABLE IF NOT EXISTS admissions (
  admission_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  lease_id TEXT,
  agent_id TEXT,
  session_id TEXT,
  kind TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  context_digest TEXT,
  base_checkpoint_ref TEXT,
  changed_paths_json TEXT NOT NULL,
  checks_json TEXT NOT NULL,
  verdict TEXT NOT NULL,
  refusal_reasons_json TEXT,
  supersedes_admission_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id),
  FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id),
  FOREIGN KEY(task_id) REFERENCES tasks(task_id)
);

CREATE TABLE IF NOT EXISTS workspace_changes (
  change_id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  task_id TEXT,
  agent_id TEXT,
  path TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_hash TEXT,
  after_hash TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
  message_id TEXT PRIMARY KEY,
  event_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  message_type TEXT NOT NULL,
  channel TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  correlation_id TEXT NOT NULL,
  causation_id TEXT,
  idempotency_key TEXT,
  delivery_state TEXT NOT NULL,
  envelope_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(session_id, channel, sequence),
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS message_attempts (
  attempt_id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  outcome TEXT,
  error_json TEXT,
  FOREIGN KEY(message_id) REFERENCES messages(message_id)
);

-- MessageReceipt is a core entity in docs/DATA-MODEL.md: the durable record that a message was dispatched
-- and whether its receipt was acknowledged. ACK means receipt, not successful execution (AGENTS.md §7), so
-- receipt state is tracked separately from delivery state on messages.
CREATE TABLE IF NOT EXISTS message_receipts (
  receipt_id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  receipt_state TEXT NOT NULL,
  acknowledged_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(message_id) REFERENCES messages(message_id),
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS inbox (
  message_id TEXT PRIMARY KEY,
  received_at TEXT NOT NULL,
  persisted_at TEXT NOT NULL,
  acked_at TEXT,
  processing_state TEXT NOT NULL,
  terminal_event_id TEXT
);

CREATE TABLE IF NOT EXISTS outbox (
  outbox_id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL UNIQUE,
  project_id TEXT NOT NULL,
  queued_at TEXT NOT NULL,
  dispatch_state TEXT NOT NULL,
  next_attempt_at TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(message_id) REFERENCES messages(message_id),
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS dead_letters (
  dead_letter_id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  final_error_json TEXT NOT NULL,
  attempts INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS events (
  event_id TEXT PRIMARY KEY,
  project_id TEXT,
  session_id TEXT,
  event_type TEXT NOT NULL,
  sequence INTEGER,
  correlation_id TEXT,
  causation_id TEXT,
  epoch INTEGER,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  prev_hash TEXT NOT NULL,
  event_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS event_cursors (
  cursor_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  consumer_id TEXT NOT NULL,
  last_sequence INTEGER NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(project_id, consumer_id)
);

CREATE TABLE IF NOT EXISTS trace_links (
  trace_link_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  link_type TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_id TEXT NOT NULL,
  target_type TEXT NOT NULL,
  target_id TEXT NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  UNIQUE(project_id, link_type, source_type, source_id, target_type, target_id)
);

CREATE TABLE IF NOT EXISTS trace_link_versions (
  trace_link_version_id TEXT PRIMARY KEY,
  trace_link_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  active INTEGER NOT NULL,
  supersedes_version INTEGER,
  created_at TEXT NOT NULL,
  FOREIGN KEY(trace_link_id) REFERENCES trace_links(trace_link_id)
);

CREATE TABLE IF NOT EXISTS trace_coverage (
  project_id TEXT NOT NULL,
  requirement_id TEXT NOT NULL,
  coverage_status TEXT NOT NULL,
  computed_at TEXT NOT NULL,
  PRIMARY KEY(project_id, requirement_id)
);

CREATE TABLE IF NOT EXISTS command_executions (
  execution_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  workspace_id TEXT NOT NULL,
  requested_by_agent_id TEXT,
  classification TEXT NOT NULL,
  executable TEXT NOT NULL,
  arguments_json TEXT NOT NULL,
  cwd TEXT NOT NULL,
  status TEXT NOT NULL,
  exit_code INTEGER,
  started_at TEXT,
  ended_at TEXT,
  timeout_seconds INTEGER NOT NULL,
  stdout_artifact_id TEXT,
  stderr_artifact_id TEXT
);

CREATE TABLE IF NOT EXISTS process_records (
  process_record_id TEXT PRIMARY KEY,
  execution_id TEXT NOT NULL,
  pid INTEGER NOT NULL,
  parent_pid INTEGER,
  state TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  FOREIGN KEY(execution_id) REFERENCES command_executions(execution_id)
);

CREATE TABLE IF NOT EXISTS builds (
  build_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  status TEXT NOT NULL,
  command_execution_id TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS test_runs (
  test_run_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  status TEXT NOT NULL,
  command_execution_id TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS failures (
  failure_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  fingerprint TEXT NOT NULL,
  category TEXT NOT NULL,
  packet_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS diagnoses (
  diagnosis_id TEXT PRIMARY KEY,
  failure_id TEXT NOT NULL,
  agent_id TEXT,
  diagnosis_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(failure_id) REFERENCES failures(failure_id)
);

CREATE TABLE IF NOT EXISTS repairs (
  repair_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  failure_id TEXT,
  status TEXT NOT NULL,
  attempt_no INTEGER NOT NULL,
  repair_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS reviews (
  review_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  reviewer_agent_id TEXT,
  verdict TEXT NOT NULL,
  findings_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS validation_runs (
  validation_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  scope_json TEXT NOT NULL,
  checks_json TEXT NOT NULL,
  verdict TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artifacts (
  artifact_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT,
  sha256 TEXT,
  size_bytes INTEGER,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS evidence (
  evidence_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  source_json TEXT NOT NULL,
  sha256 TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS evidence_links (
  evidence_id TEXT NOT NULL,
  artifact_id TEXT NOT NULL,
  PRIMARY KEY(evidence_id, artifact_id),
  FOREIGN KEY(evidence_id) REFERENCES evidence(evidence_id),
  FOREIGN KEY(artifact_id) REFERENCES artifacts(artifact_id)
);

CREATE TABLE IF NOT EXISTS user_questions (
  question_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  round_id TEXT,
  body TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_answers (
  answer_id TEXT PRIMARY KEY,
  question_id TEXT NOT NULL,
  answer TEXT NOT NULL,
  answered_at TEXT NOT NULL,
  FOREIGN KEY(question_id) REFERENCES user_questions(question_id)
);

-- UserContribution is a core entity in docs/DATA-MODEL.md: a post-intake free-text user message plus the
-- advisory classification the intake router produced and the authoritative outcome the owning service
-- returned. The classification is advisory; result_type is what the service actually did, and the Control
-- Room displays the outcome rather than the label. A contribution never substitutes for the authoritative
-- object it produced.
CREATE TABLE IF NOT EXISTS user_contributions (
  contribution_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  body TEXT NOT NULL,
  classification TEXT NOT NULL,
  classification_confidence REAL,
  classification_source TEXT NOT NULL,
  result_type TEXT NOT NULL,
  result_reference TEXT,
  epoch_before INTEGER NOT NULL,
  epoch_after INTEGER NOT NULL,
  context_snapshot_before TEXT,
  context_snapshot_after TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

CREATE TABLE IF NOT EXISTS barriers (
  barrier_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  state TEXT NOT NULL,
  predicate_json TEXT NOT NULL,
  deadline TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id)
);

CREATE INDEX IF NOT EXISTS idx_messages_project_state ON messages(project_id, delivery_state);
CREATE INDEX IF NOT EXISTS idx_messages_correlation ON messages(correlation_id);
-- DEC-027 makes `project_id + operation_id` the canonical idempotency scope, and `operation_id` is an optional
-- envelope field with no column of its own: it lives inside `envelope_json`. This index covers the extracted
-- value so the lookup the bus performs on every enqueue is indexed rather than a scan.
--
-- An index is additive and reaches a database that already exists, because `open()` re-executes this whole
-- batch on every connection and `CREATE INDEX IF NOT EXISTS` is a no-op when it is already there. A new column
-- could not: there is no migration runner, so an ALTER here would never reach an existing database.
CREATE INDEX IF NOT EXISTS idx_messages_operation ON messages(project_id, json_extract(envelope_json, '$.operation_id'));
-- The dispatcher's one query is "which queue entries are pending and due?", ordered by when they became due.
-- `dispatch_state` first because it is the equality predicate and `next_attempt_at` second because it is both
-- the range predicate and the sort, so one index serves the filter and the order.
CREATE INDEX IF NOT EXISTS idx_outbox_due ON outbox(dispatch_state, next_attempt_at);
CREATE INDEX IF NOT EXISTS idx_messages_priority ON messages(json_extract(envelope_json, '$.priority'));
-- Every delivery attempt is recorded, and the observability requirement is that a message's attempts can be
-- read back with their outcomes, timestamps and retry count.
CREATE INDEX IF NOT EXISTS idx_attempts_message ON message_attempts(message_id, attempt_no);
CREATE INDEX IF NOT EXISTS idx_dead_letters_project ON dead_letters(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_events_project_sequence ON events(project_id, sequence);
CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id);
CREATE INDEX IF NOT EXISTS idx_events_project_hash ON events(project_id, event_hash);
CREATE INDEX IF NOT EXISTS idx_admissions_task ON admissions(project_id, task_id, kind, verdict);
CREATE INDEX IF NOT EXISTS idx_leases_expiry ON task_leases(status, expires_at);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(project_id, status);
CREATE INDEX IF NOT EXISTS idx_failures_fingerprint ON failures(project_id, fingerprint);
CREATE INDEX IF NOT EXISTS idx_trace_source ON trace_links(project_id, source_type, source_id);
CREATE INDEX IF NOT EXISTS idx_trace_target ON trace_links(project_id, target_type, target_id);
CREATE INDEX IF NOT EXISTS idx_validation_status ON validation_runs(project_id, verdict);
CREATE INDEX IF NOT EXISTS idx_council_round_state ON council_rounds(project_id, state);
CREATE INDEX IF NOT EXISTS idx_barrier_state ON barriers(project_id, state);
CREATE INDEX IF NOT EXISTS idx_briefs_project_version ON project_briefs(project_id, version);
CREATE INDEX IF NOT EXISTS idx_briefs_supersedes ON project_briefs(supersedes_brief_id);
CREATE INDEX IF NOT EXISTS idx_paths_project ON project_paths(project_id);
CREATE INDEX IF NOT EXISTS idx_receipts_message ON message_receipts(message_id);
CREATE INDEX IF NOT EXISTS idx_receipts_state ON message_receipts(project_id, receipt_state);
CREATE INDEX IF NOT EXISTS idx_contributions_project ON user_contributions(project_id, created_at);

-- --- Council decision-quality records (DEC-052).
-- New tables only. crates/storage applies this file with CREATE TABLE IF NOT EXISTS on every open and has no
-- migration runner, so a database created from an earlier revision gains these tables on its next open and no
-- existing column is ever altered. These are the first CHECK constraints in this file: the mode, role, grade,
-- budget-kind, outcome-status and source vocabularies they close were previously free text or absent.
-- `council_decision_outcomes` is also the first table to reference `decisions(decision_id)`.

CREATE TABLE IF NOT EXISTS council_mode_selections (
  selection_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  round_id TEXT,
  decision_class TEXT NOT NULL CHECK(decision_class IN ('ARCHITECTURE','STACK_TECHNOLOGY','IRREVERSIBLE','SECURITY','DATA_LOSS','ROUTINE')),
  mode TEXT NOT NULL CHECK(mode IN ('SOLO','REVIEW','FULL')),
  inputs_json TEXT NOT NULL,
  reasons_json TEXT NOT NULL,
  selector_version TEXT NOT NULL,
  override_source TEXT NOT NULL CHECK(override_source IN ('NONE','USER')),
  supersedes_selection_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(project_id),
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id),
  FOREIGN KEY(supersedes_selection_id) REFERENCES council_mode_selections(selection_id)
);

-- A mode selection is recorded for every material decision point, including SOLO, which opens no round; that is
-- why round_id is nullable. Escalation appends a superseding row rather than updating the previous one.
CREATE INDEX IF NOT EXISTS idx_council_mode_project ON council_mode_selections(project_id, created_at);

CREATE TABLE IF NOT EXISTS council_round_roles (
  round_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('PROPOSER','SKEPTIC','VERIFIER')),
  assigned_reason TEXT NOT NULL,
  assigned_at TEXT NOT NULL,
  PRIMARY KEY(round_id, agent_id, role),
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id),
  FOREIGN KEY(agent_id) REFERENCES agents(agent_id)
);

CREATE TABLE IF NOT EXISTS council_claim_grades (
  claim_id TEXT PRIMARY KEY,
  position_id TEXT NOT NULL,
  round_id TEXT NOT NULL,
  grade TEXT NOT NULL CHECK(grade IN ('ASSUMPTION','CITED','VERIFIED')),
  load_bearing INTEGER NOT NULL CHECK(load_bearing IN (0,1)),
  basis_json TEXT NOT NULL,
  computed_at TEXT NOT NULL,
  FOREIGN KEY(position_id) REFERENCES council_positions(position_id),
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id)
);

CREATE INDEX IF NOT EXISTS idx_council_claim_grade_position ON council_claim_grades(position_id);

CREATE TABLE IF NOT EXISTS council_budget_ledger (
  entry_id TEXT PRIMARY KEY,
  council_session_id TEXT,
  round_id TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('ROUND_OPENED','PAUSED','RESUMED','SPIKE_EXECUTED','TOKENS_REPORTED','BUDGET_EXHAUSTED','SEALED')),
  amount INTEGER,
  availability TEXT NOT NULL CHECK(availability IN ('REPORTED','UNAVAILABLE')),
  detail_json TEXT,
  recorded_at TEXT NOT NULL,
  FOREIGN KEY(council_session_id) REFERENCES council_sessions(council_session_id),
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id)
);

-- Append-only. A paused interval is a PAUSED row and its matching RESUMED row, so wall-clock can exclude the
-- interval without ever rewriting an entry. `availability` is UNAVAILABLE where an adapter reports no token
-- usage: the amount is then NULL and no number is invented.
CREATE INDEX IF NOT EXISTS idx_council_budget_round ON council_budget_ledger(round_id, recorded_at);

CREATE TABLE IF NOT EXISTS council_decision_outcomes (
  outcome_record_id TEXT PRIMARY KEY,
  decision_id TEXT NOT NULL,
  council_session_id TEXT,
  round_id TEXT,
  mode TEXT NOT NULL CHECK(mode IN ('SOLO','REVIEW','FULL')),
  decision_class TEXT NOT NULL CHECK(decision_class IN ('ARCHITECTURE','STACK_TECHNOLOGY','IRREVERSIBLE','SECURITY','DATA_LOSS','ROUTINE')),
  status TEXT NOT NULL CHECK(status IN ('HELD','AMENDED','REVERSED','UNRESOLVED')),
  validation_evidence_id TEXT,
  source TEXT NOT NULL CHECK(source IN ('VALIDATION_RESULT','REOPEN_DECISION','USER_SUPERSESSION')),
  supersedes_outcome_id TEXT,
  recorded_at TEXT NOT NULL,
  CHECK(status <> 'HELD' OR validation_evidence_id IS NOT NULL),
  FOREIGN KEY(decision_id) REFERENCES decisions(decision_id),
  FOREIGN KEY(council_session_id) REFERENCES council_sessions(council_session_id),
  FOREIGN KEY(round_id) REFERENCES council_rounds(round_id),
  FOREIGN KEY(validation_evidence_id) REFERENCES evidence(evidence_id),
  FOREIGN KEY(supersedes_outcome_id) REFERENCES council_decision_outcomes(outcome_record_id)
);

-- HELD is the only status that asserts the decision survived, so the table requires evidence for it and for
-- nothing else. Supporting evidence is referenced directly rather than through evidence_links, which links
-- evidence to artifacts only. Append-only: a reversal or amendment supersedes rather than updates.
CREATE INDEX IF NOT EXISTS idx_council_outcome_decision ON council_decision_outcomes(decision_id, recorded_at);

CREATE TABLE IF NOT EXISTS council_outcome_agent_links (
  outcome_record_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  position_id TEXT,
  stance TEXT NOT NULL,
  PRIMARY KEY(outcome_record_id, agent_id),
  FOREIGN KEY(outcome_record_id) REFERENCES council_decision_outcomes(outcome_record_id),
  FOREIGN KEY(agent_id) REFERENCES agents(agent_id),
  FOREIGN KEY(position_id) REFERENCES council_positions(position_id)
);
