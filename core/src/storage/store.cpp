#include "mayasaba/store.hpp"

#include <sqlite3.h>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::storage {
namespace {

Status SqliteStatus(sqlite3* db, const std::string& context) {
    return Status::Error(ErrorCode::Internal,
                         context + ": " + (db ? sqlite3_errmsg(db) : "no connection"));
}

void BindParams(sqlite3_stmt* statement, const std::vector<SqlValue>& params) {
    for (std::size_t i = 0; i < params.size(); ++i) {
        int index = static_cast<int>(i) + 1;
        const SqlValue& value = params[i];
        switch (value.kind) {
            case SqlValue::Kind::Null:
                sqlite3_bind_null(statement, index);
                break;
            case SqlValue::Kind::Int:
                sqlite3_bind_int64(statement, index, value.integer);
                break;
            case SqlValue::Kind::Text:
                sqlite3_bind_text(statement, index, value.text.c_str(),
                                  static_cast<int>(value.text.size()), SQLITE_TRANSIENT);
                break;
        }
    }
}

const char* kSchemaV1 = R"SQL(
CREATE TABLE projects(
  project_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL,
  root_canonical TEXT NOT NULL UNIQUE,
  volume_serial INTEGER NOT NULL DEFAULT 0,
  file_index INTEGER NOT NULL DEFAULT 0,
  epoch INTEGER NOT NULL DEFAULT 0,
  lifecycle_phase TEXT NOT NULL DEFAULT 'PROJECT_CREATED',
  condition TEXT NOT NULL DEFAULT 'ACTIVE',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE project_intents(
  intent_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(project_id),
  version INTEGER NOT NULL,
  epoch INTEGER NOT NULL,
  summary TEXT NOT NULL,
  contribution_ids TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  UNIQUE(project_id, version)
);
CREATE TABLE contributions(
  contribution_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(project_id),
  text TEXT NOT NULL,
  attachments TEXT NOT NULL DEFAULT '[]',
  question_ref TEXT,
  decision_ref TEXT,
  state TEXT NOT NULL DEFAULT 'PERSISTED',
  created_at TEXT NOT NULL
);
CREATE INDEX idx_contributions_project ON contributions(project_id, created_at);
CREATE TABLE attachments(
  attachment_id TEXT PRIMARY KEY,
  contribution_id TEXT,
  project_id TEXT NOT NULL,
  name TEXT NOT NULL,
  media_type TEXT NOT NULL,
  size INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  source_path TEXT NOT NULL,
  outside_root INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'READY',
  created_at TEXT NOT NULL
);
CREATE TABLE work_requests(
  work_request_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  contribution_id TEXT NOT NULL,
  operation TEXT NOT NULL,
  title TEXT NOT NULL,
  scope TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL,
  authorization TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_work_requests_project ON work_requests(project_id, created_at);
CREATE TABLE requirements(
  requirement_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  text TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'DRAFT',
  contribution_id TEXT,
  supersedes TEXT,
  epoch INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE TABLE decisions(
  decision_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  decision_point_id TEXT NOT NULL,
  question TEXT NOT NULL,
  resolution TEXT NOT NULL DEFAULT '',
  disposition TEXT NOT NULL,
  rationale TEXT NOT NULL DEFAULT '',
  round_id TEXT,
  outcome TEXT,
  epoch INTEGER NOT NULL,
  context_digest TEXT NOT NULL DEFAULT '',
  supersedes TEXT,
  state TEXT NOT NULL DEFAULT 'HELD',
  created_at TEXT NOT NULL
);
CREATE TABLE tasks(
  task_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  work_request_id TEXT,
  requirement_id TEXT,
  title TEXT NOT NULL,
  objective TEXT NOT NULL,
  contract TEXT NOT NULL,
  status TEXT NOT NULL,
  assigned_agent TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_tasks_project ON tasks(project_id, created_at);
CREATE TABLE task_deps(
  task_id TEXT NOT NULL,
  depends_on TEXT NOT NULL,
  PRIMARY KEY(task_id, depends_on)
);
CREATE TABLE attempts(
  attempt_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  agent TEXT NOT NULL,
  status TEXT NOT NULL,
  lease_version INTEGER NOT NULL,
  session_id TEXT,
  baseline TEXT NOT NULL DEFAULT '{}',
  result TEXT NOT NULL DEFAULT '{}',
  started_at TEXT NOT NULL,
  ended_at TEXT
);
CREATE TABLE leases(
  lease_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  state TEXT NOT NULL DEFAULT 'ACTIVE',
  issued_at TEXT NOT NULL,
  expires_at TEXT,
  revoked_at TEXT,
  allowed_reads TEXT NOT NULL DEFAULT '[]',
  allowed_writes TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX idx_leases_task ON leases(task_id, state);
CREATE TABLE agent_sessions(
  session_id TEXT PRIMARY KEY,
  agent TEXT NOT NULL,
  project_id TEXT NOT NULL,
  state TEXT NOT NULL,
  process_id INTEGER NOT NULL DEFAULT 0,
  capabilities TEXT NOT NULL DEFAULT '{}',
  started_at TEXT NOT NULL,
  ended_at TEXT,
  last_health_at TEXT
);
CREATE TABLE snapshots(
  snapshot_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  kind TEXT NOT NULL,
  manifest_digest TEXT NOT NULL DEFAULT '',
  digest TEXT NOT NULL,
  entry_count INTEGER NOT NULL DEFAULT 0,
  excluded_count INTEGER NOT NULL DEFAULT 0,
  truncated INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE TABLE snapshot_entries(
  snapshot_id TEXT NOT NULL,
  rel_path TEXT NOT NULL,
  kind TEXT NOT NULL,
  size INTEGER NOT NULL DEFAULT 0,
  sha256 TEXT NOT NULL DEFAULT '',
  coverage TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  PRIMARY KEY(snapshot_id, rel_path)
);
CREATE TABLE council_points(
  point_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  trigger_key TEXT NOT NULL,
  trigger_kind TEXT NOT NULL,
  question TEXT NOT NULL,
  affected TEXT NOT NULL DEFAULT '{}',
  required_outcome TEXT NOT NULL DEFAULT '',
  epoch INTEGER NOT NULL,
  context_digest TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  state TEXT NOT NULL DEFAULT 'OPEN',
  outcome TEXT,
  resolution TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(project_id, trigger_key, epoch)
);
CREATE TABLE council_rounds(
  round_id TEXT PRIMARY KEY,
  point_id TEXT NOT NULL,
  round_number INTEGER NOT NULL,
  chair TEXT NOT NULL,
  roles TEXT NOT NULL DEFAULT '{}',
  state TEXT NOT NULL,
  sealed_reason TEXT,
  outcome TEXT,
  positions_digest TEXT NOT NULL DEFAULT '',
  predecessor_round_id TEXT,
  epoch INTEGER NOT NULL,
  context_digest TEXT NOT NULL,
  created_at TEXT NOT NULL,
  sealed_at TEXT,
  UNIQUE(point_id, round_number)
);
CREATE TABLE positions(
  position_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  point_id TEXT NOT NULL,
  agent TEXT NOT NULL,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  rationale TEXT NOT NULL DEFAULT '',
  claims TEXT NOT NULL DEFAULT '[]',
  supersedes TEXT,
  digest TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_positions_round ON positions(round_id, agent);
CREATE TABLE critiques(
  critique_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  author TEXT NOT NULL,
  target_position_id TEXT NOT NULL,
  content TEXT NOT NULL,
  claims TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL
);
CREATE TABLE council_syntheses(
  synthesis_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  author TEXT NOT NULL,
  content TEXT NOT NULL,
  cited_positions TEXT NOT NULL DEFAULT '[]',
  review_agent TEXT,
  review_content TEXT,
  coverage_ok INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE TABLE evidence(
  evidence_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  artifact_path TEXT NOT NULL DEFAULT '',
  artifact_sha256 TEXT NOT NULL DEFAULT '',
  blob_sha256 TEXT,
  check_identity TEXT NOT NULL DEFAULT '',
  environment TEXT NOT NULL DEFAULT '{}',
  source TEXT NOT NULL DEFAULT '{}',
  provenance TEXT NOT NULL DEFAULT '{}',
  observed_at TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE validation_criteria(
  criterion_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  requirement_id TEXT,
  expectation TEXT NOT NULL,
  oracle_class TEXT NOT NULL,
  oracle_spec TEXT NOT NULL DEFAULT '{}',
  blocking INTEGER NOT NULL DEFAULT 1,
  adequacy TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
CREATE TABLE validation_results(
  row_id INTEGER PRIMARY KEY AUTOINCREMENT,
  check_id TEXT NOT NULL,
  check_version TEXT NOT NULL,
  criterion_id TEXT NOT NULL,
  verdict TEXT NOT NULL,
  rationale TEXT NOT NULL,
  diagnostics TEXT NOT NULL DEFAULT '[]',
  failure_kind TEXT NOT NULL DEFAULT 'UNKNOWN',
  evidence_hash TEXT NOT NULL DEFAULT '',
  validated_at TEXT NOT NULL
);
CREATE INDEX idx_validation_criterion ON validation_results(criterion_id);
CREATE TABLE diagnostics(
  diagnostic_id TEXT PRIMARY KEY,
  criterion_id TEXT NOT NULL,
  class TEXT NOT NULL,
  observation TEXT NOT NULL,
  hypothesis TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);
CREATE TABLE repairs(
  repair_id TEXT PRIMARY KEY,
  validation_id TEXT NOT NULL,
  hypothesis TEXT NOT NULL,
  planned_changes TEXT NOT NULL DEFAULT '',
  outcome TEXT NOT NULL,
  diagnostics TEXT NOT NULL DEFAULT '[]',
  evidence_hash TEXT NOT NULL DEFAULT '',
  repaired_at TEXT NOT NULL,
  supersedes_history INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE policy_denials(
  denial_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  action TEXT NOT NULL,
  subject TEXT NOT NULL DEFAULT '{}',
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE events(
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id TEXT NOT NULL,
  event_id TEXT NOT NULL UNIQUE,
  type TEXT NOT NULL,
  payload TEXT NOT NULL,
  prev_hash TEXT NOT NULL DEFAULT '',
  hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_events_project ON events(project_id, seq);
CREATE TABLE mcf_outbox(
  message_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  channel TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 6,
  sequence INTEGER NOT NULL DEFAULT 0,
  envelope TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'PENDING',
  attempts INTEGER NOT NULL DEFAULT 0,
  next_attempt_at INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  acked_at TEXT,
  dead_lettered_at TEXT,
  reason TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_outbox_state ON mcf_outbox(state, priority, sequence);
CREATE TABLE mcf_inbox(
  message_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  envelope TEXT NOT NULL,
  received_at TEXT NOT NULL,
  acked_at TEXT,
  processed_at TEXT,
  state TEXT NOT NULL DEFAULT 'RECEIVED',
  reject_reason TEXT NOT NULL DEFAULT ''
);
CREATE TABLE mcf_dead_letters(
  message_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  envelope TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE idempotency(
  key TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  result TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE publications(
  publication_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT,
  state TEXT NOT NULL,
  journal TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE publication_files(
  publication_id TEXT NOT NULL,
  rel_path TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_sha256 TEXT,
  after_sha256 TEXT NOT NULL,
  staged_path TEXT NOT NULL,
  backup_path TEXT,
  state TEXT NOT NULL DEFAULT 'PENDING',
  PRIMARY KEY(publication_id, rel_path)
);
CREATE TABLE blobs(
  sha256 TEXT PRIMARY KEY,
  size INTEGER NOT NULL,
  path TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE settings(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
)SQL";

struct Migration {
    int version;
    const char* sql;
};

constexpr int kNewestMigrationVersion = 4;
static_assert(kNewestMigrationVersion == kSchemaVersion,
              "kSchemaVersion must equal the newest migration version");

// v3: orchestrator-owned durable tables that were previously created via lazy DDL in the
// Orchestrator (app/src/orchestration.cpp). Per the architecture, schema ownership belongs to
// the SQLite Storage layer (AGENTS.md 4.2, spec section 9). This migration consolidates them
// into the storage owner's migration registry so there is a single schema definition point and
// the Orchestrator never issues DDL.
const char* kSchemaV3 = R"SQL(
CREATE TABLE IF NOT EXISTS orchestrator_triggers(
  trigger_id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  project_id TEXT NOT NULL,
  target_task_id TEXT NOT NULL DEFAULT '',
  council_point_id TEXT NOT NULL DEFAULT '',
  fired INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  fired_at TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS orchestrator_sync_barriers(
  barrier_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  purpose TEXT NOT NULL,
  required_agents TEXT NOT NULL,
  context_digest TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at TEXT NOT NULL,
  satisfied_at TEXT NOT NULL DEFAULT '',
  expires_at_ms INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS orchestrator_sync_acks(
  barrier_id TEXT NOT NULL,
  agent TEXT NOT NULL,
  context_digest TEXT NOT NULL,
  acknowledged_at TEXT NOT NULL,
  PRIMARY KEY(barrier_id, agent)
);
CREATE TABLE IF NOT EXISTS orchestrator_publications(
  publication_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at TEXT NOT NULL
);
)SQL";

const Migration kMigrations[] = {
    {1, kSchemaV1},
    // v2: the outbox sequence is per (project, sender, channel); the sender column was missing
    // from v1, which made every sequence query fail and silently reset sequences to 1.
    {2, "ALTER TABLE mcf_outbox ADD COLUMN sender TEXT NOT NULL DEFAULT '';"},
    {3, kSchemaV3},
    // v4: snapshot digest profiles. Historical snapshot digests were hashed without each
    // entry's reason field; new digests include it and every row records the profile it was
    // hashed under. The ALTER backfills existing rows to profile 1 (the original pre-change
    // shape) so their stored digests verify unchanged, while new snapshots write profile 2.
    // SQLite ALTER TABLE ADD COLUMN with a constant non-NULL default is supported; the
    // versioned migration registry guarantees this runs exactly once per database.
    {4, "ALTER TABLE snapshots ADD COLUMN digest_profile INTEGER NOT NULL DEFAULT 1;"},
};

}  // namespace

// --- Row ---------------------------------------------------------------------------------

std::int64_t Row::Int(const std::string& column) const {
    auto it = fields_.find(column);
    if (it == fields_.end() || it->second.kind == SqlValue::Kind::Null) return 0;
    if (it->second.kind == SqlValue::Kind::Int) return it->second.integer;
    return std::strtoll(it->second.text.c_str(), nullptr, 10);
}

std::string Row::Text(const std::string& column) const {
    auto it = fields_.find(column);
    if (it == fields_.end() || it->second.kind == SqlValue::Kind::Null) return {};
    return it->second.text;
}

bool Row::IsNull(const std::string& column) const {
    auto it = fields_.find(column);
    return it == fields_.end() || it->second.kind == SqlValue::Kind::Null;
}

bool Row::Has(const std::string& column) const { return fields_.find(column) != fields_.end(); }

// --- Transaction -------------------------------------------------------------------------

Transaction::Transaction(Transaction&& other) noexcept
    : store_(other.store_), finished_(other.finished_) {
    other.store_ = nullptr;
    other.finished_ = true;
}

Transaction& Transaction::operator=(Transaction&& other) noexcept {
    if (this != &other) {
        if (store_ && !finished_) Rollback();
        store_ = other.store_;
        finished_ = other.finished_;
        other.store_ = nullptr;
        other.finished_ = true;
    }
    return *this;
}

Transaction::~Transaction() {
    if (store_ && !finished_) Rollback();
}

Status Transaction::Commit() {
    if (!store_ || finished_) return Status::Error(ErrorCode::Internal, "transaction not active");
    auto status = store_->ExecRaw("COMMIT");
    finished_ = true;
    store_->in_transaction_ = false;
    return status;
}

void Transaction::Rollback() {
    if (!store_ || finished_) return;
    (void)store_->ExecRaw("ROLLBACK");
    finished_ = true;
    store_->in_transaction_ = false;
}

// --- Store -------------------------------------------------------------------------------

Store::~Store() {
    if (db_) sqlite3_close(db_);
}

Expected<std::unique_ptr<Store>> Store::Open(const std::string& db_path) {
    std::unique_ptr<Store> store(new Store());
    store->path_ = db_path;
    auto parent = fs::ParentPath(db_path);
    if (!parent.empty()) {
        auto status = fs::EnsureDirectory(parent);
        if (!status.ok()) return Expected<std::unique_ptr<Store>>(status);
    }
    int rc = sqlite3_open_v2(db_path.c_str(), &store->db_,
                             SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
                             nullptr);
    if (rc != SQLITE_OK) {
        auto status = SqliteStatus(store->db_, "sqlite3_open_v2 failed");
        return Expected<std::unique_ptr<Store>>(status);
    }
    sqlite3_busy_timeout(store->db_, 5000);
    // Verify the actual journal mode instead of assuming the pragma succeeded.
    {
        sqlite3_stmt* statement = nullptr;
        rc = sqlite3_prepare_v2(store->db_, "PRAGMA journal_mode=WAL;", -1, &statement, nullptr);
        if (rc == SQLITE_OK && sqlite3_step(statement) == SQLITE_ROW) {
            const unsigned char* text = sqlite3_column_text(statement, 0);
            store->journal_mode_ = text ? reinterpret_cast<const char*>(text) : "unknown";
        }
        if (statement) sqlite3_finalize(statement);
    }
    store->ExecRaw("PRAGMA synchronous=NORMAL;");
    store->ExecRaw("PRAGMA foreign_keys=ON;");
    auto status = store->ApplyMigrations();
    if (!status.ok()) return Expected<std::unique_ptr<Store>>(status);
    return store;
}

std::unique_lock<std::recursive_mutex> Store::Lock() {
    return std::unique_lock<std::recursive_mutex>(mutex_);
}

Status Store::ExecRaw(const std::string& sql) {
    char* error = nullptr;
    int rc = sqlite3_exec(db_, sql.c_str(), nullptr, nullptr, &error);
    if (rc != SQLITE_OK) {
        std::string message = error ? error : "unknown sqlite error";
        if (error) sqlite3_free(error);
        return Status::Error(ErrorCode::Internal, "sql failed: " + message);
    }
    return Status::Ok();
}

Expected<std::vector<Row>> Store::Query(const std::string& sql, const std::vector<SqlValue>& params) {
    auto lock = Lock();
    sqlite3_stmt* statement = nullptr;
    int rc = sqlite3_prepare_v2(db_, sql.c_str(), static_cast<int>(sql.size()), &statement, nullptr);
    if (rc != SQLITE_OK) return Expected<std::vector<Row>>(SqliteStatus(db_, "prepare failed"));
    BindParams(statement, params);
    std::vector<Row> rows;
    for (;;) {
        rc = sqlite3_step(statement);
        if (rc == SQLITE_DONE) break;
        if (rc != SQLITE_ROW) {
            sqlite3_finalize(statement);
            return Expected<std::vector<Row>>(SqliteStatus(db_, "step failed"));
        }
        Row row;
        int columns = sqlite3_column_count(statement);
        for (int i = 0; i < columns; ++i) {
            const char* name = sqlite3_column_name(statement, i);
            SqlValue value;
            switch (sqlite3_column_type(statement, i)) {
                case SQLITE_NULL:
                    value = SqlValue::Null();
                    break;
                case SQLITE_INTEGER:
                    value = SqlValue::Int(sqlite3_column_int64(statement, i));
                    break;
                case SQLITE_FLOAT:
                    value = SqlValue::Text(std::to_string(sqlite3_column_double(statement, i)));
                    break;
                default: {
                    const unsigned char* text = sqlite3_column_text(statement, i);
                    int bytes = sqlite3_column_bytes(statement, i);
                    value = SqlValue::Text(text ? std::string(reinterpret_cast<const char*>(text),
                                                               static_cast<std::size_t>(bytes))
                                                : std::string());
                    break;
                }
            }
            row.fields_[name ? name : ""] = std::move(value);
        }
        rows.push_back(std::move(row));
    }
    sqlite3_finalize(statement);
    return rows;
}

Status Store::Exec(const std::string& sql, const std::vector<SqlValue>& params) {
    auto lock = Lock();
    sqlite3_stmt* statement = nullptr;
    int rc = sqlite3_prepare_v2(db_, sql.c_str(), static_cast<int>(sql.size()), &statement, nullptr);
    if (rc != SQLITE_OK) return SqliteStatus(db_, "prepare failed");
    BindParams(statement, params);
    rc = sqlite3_step(statement);
    if (rc != SQLITE_DONE && rc != SQLITE_ROW) {
        sqlite3_finalize(statement);
        return SqliteStatus(db_, "step failed");
    }
    sqlite3_finalize(statement);
    return Status::Ok();
}

std::int64_t Store::LastInsertRowId() {
    auto lock = Lock();
    return sqlite3_last_insert_rowid(db_);
}

int Store::Changes() {
    auto lock = Lock();
    return sqlite3_changes(db_);
}

Expected<Transaction> Store::Begin() {
    auto lock = Lock();
    if (in_transaction_) {
        return Fail<Transaction>(ErrorCode::Internal, "nested transaction is not supported");
    }
    auto status = ExecRaw("BEGIN IMMEDIATE");
    if (!status.ok()) return Expected<Transaction>(status);
    in_transaction_ = true;
    return Transaction(this);
}

Status Store::ApplyMigrations() {
    auto status = ExecRaw(
        "CREATE TABLE IF NOT EXISTS schema_migrations("
        "version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);");
    if (!status.ok()) return status;
    auto rows = Query("SELECT MAX(version) AS v FROM schema_migrations;");
    if (!rows.ok()) return rows.status();
    int current = rows.value().empty() ? 0 : static_cast<int>(rows.value()[0].Int("v"));
    for (const auto& migration : kMigrations) {
        if (migration.version <= current) continue;
        auto transaction = Begin();
        if (!transaction.ok()) return transaction.status();
        status = ExecRaw(migration.sql);
        if (status.ok()) {
            status = Exec("INSERT INTO schema_migrations(version, applied_at) VALUES(?, ?);",
                          {SqlValue::Int(migration.version), SqlValue::Text(NowUtcIso8601())});
        }
        if (!status.ok()) {
            transaction.value().Rollback();
            return status;
        }
        status = transaction.value().Commit();
        if (!status.ok()) return status;
        current = migration.version;
    }
    schema_version_ = current;
    return Status::Ok();
}

Expected<EventRecord> Store::AppendEvent(const std::string& project_id, const std::string& type,
                                         const nlohmann::json& payload) {
    auto lock = Lock();
    bool own_transaction = !in_transaction_;
    std::optional<Transaction> guard;
    if (own_transaction) {
        auto begun = Begin();
        if (!begun.ok()) return Expected<EventRecord>(begun.status());
        guard.emplace(std::move(begun.value()));
    }
    // On any early return the guard (if owned here) rolls back automatically.

    std::string prev_hash;
    {
        auto rows = Query(
            "SELECT hash FROM events WHERE project_id = ? ORDER BY seq DESC LIMIT 1;",
            {SqlValue::Text(project_id)});
        if (!rows.ok()) return Expected<EventRecord>(rows.status());
        if (!rows.value().empty()) prev_hash = rows.value()[0].Text("hash");
    }
    nlohmann::json canonical_input = {
        {"event_id", NewId("evt")},
        {"payload", payload},
        {"project_id", project_id},
        {"type", type},
    };
    auto canonical = CanonicalDump(canonical_input);
    if (!canonical.ok()) return Expected<EventRecord>(canonical.status());
    auto hash = Sha256::ChainLink(prev_hash, canonical.value());
    if (!hash.ok()) return Expected<EventRecord>(hash.status());

    EventRecord record;
    record.project_id = project_id;
    record.event_id = canonical_input["event_id"].get<std::string>();
    record.type = type;
    record.payload_json = payload.dump();
    record.prev_hash = prev_hash;
    record.hash = hash.value();
    record.created_at = NowUtcIso8601();

    auto status = Exec(
        "INSERT INTO events(project_id, event_id, type, payload, prev_hash, hash, created_at) "
        "VALUES(?,?,?,?,?,?,?);",
        {SqlValue::Text(record.project_id), SqlValue::Text(record.event_id), SqlValue::Text(type),
         SqlValue::Text(record.payload_json), SqlValue::Text(prev_hash), SqlValue::Text(record.hash),
         SqlValue::Text(record.created_at)});
    if (!status.ok()) return Expected<EventRecord>(status);
    record.seq = LastInsertRowId();
    if (guard.has_value()) {
        status = guard->Commit();
        if (!status.ok()) return Expected<EventRecord>(status);
    }
    return record;
}

Expected<std::vector<EventRecord>> Store::EventsFor(const std::string& project_id,
                                                    std::int64_t after_seq, std::int64_t limit) {
    auto rows = Query(
        "SELECT seq, project_id, event_id, type, payload, prev_hash, hash, created_at FROM events "
        "WHERE project_id = ? AND seq > ? ORDER BY seq ASC LIMIT ?;",
        {SqlValue::Text(project_id), SqlValue::Int(after_seq), SqlValue::Int(limit)});
    if (!rows.ok()) return Expected<std::vector<EventRecord>>(rows.status());
    std::vector<EventRecord> records;
    records.reserve(rows.value().size());
    for (const auto& row : rows.value()) {
        EventRecord record;
        record.seq = row.Int("seq");
        record.project_id = row.Text("project_id");
        record.event_id = row.Text("event_id");
        record.type = row.Text("type");
        record.payload_json = row.Text("payload");
        record.prev_hash = row.Text("prev_hash");
        record.hash = row.Text("hash");
        record.created_at = row.Text("created_at");
        records.push_back(std::move(record));
    }
    return records;
}

Status Store::VerifyChain(const std::string& project_id, std::int64_t* first_bad_seq) {
    auto records = EventsFor(project_id, 0, 1'000'000);
    if (!records.ok()) return records.status();
    std::string prev;
    for (const auto& record : records.value()) {
        if (record.prev_hash != prev) {
            if (first_bad_seq) *first_bad_seq = record.seq;
            return Status::Error(ErrorCode::IntegrityFailure,
                                 "event chain predecessor mismatch at seq " +
                                     std::to_string(record.seq));
        }
        auto payload = ParseJsonBounded(record.payload_json);
        if (!payload.ok()) {
            if (first_bad_seq) *first_bad_seq = record.seq;
            return Status::Error(ErrorCode::IntegrityFailure, "event payload unreadable");
        }
        nlohmann::json canonical_input = {
            {"event_id", record.event_id},
            {"payload", payload.value()},
            {"project_id", record.project_id},
            {"type", record.type},
        };
        auto canonical = CanonicalDump(canonical_input);
        if (!canonical.ok()) return canonical.status();
        auto expected = Sha256::ChainLink(record.prev_hash, canonical.value());
        if (!expected.ok()) return expected.status();
        if (expected.value() != record.hash) {
            if (first_bad_seq) *first_bad_seq = record.seq;
            return Status::Error(ErrorCode::IntegrityFailure,
                                 "event chain digest mismatch at seq " +
                                     std::to_string(record.seq));
        }
        prev = record.hash;
    }
    return Status::Ok();
}

std::int64_t Store::EventCount(const std::string& project_id) {
    auto rows = Query("SELECT COUNT(*) AS c FROM events WHERE project_id = ?;",
                      {SqlValue::Text(project_id)});
    if (!rows.ok() || rows.value().empty()) return 0;
    return rows.value()[0].Int("c");
}

std::optional<nlohmann::json> Store::FindIdempotentResult(const std::string& project_id,
                                                          const std::string& operation_id) {
    auto rows = Query("SELECT result FROM idempotency WHERE key = ?;",
                      {SqlValue::Text(project_id + "|" + operation_id)});
    if (!rows.ok() || rows.value().empty()) return std::nullopt;
    auto parsed = ParseJsonBounded(rows.value()[0].Text("result"));
    if (!parsed.ok()) return std::nullopt;
    return parsed.value();
}

Status Store::RecordIdempotentResult(const std::string& project_id, const std::string& operation_id,
                                     const nlohmann::json& result) {
    return Exec(
        "INSERT OR REPLACE INTO idempotency(key, project_id, operation_id, result, created_at) "
        "VALUES(?,?,?,?,?);",
        {SqlValue::Text(project_id + "|" + operation_id), SqlValue::Text(project_id),
         SqlValue::Text(operation_id), SqlValue::Text(result.dump()), SqlValue::Text(NowUtcIso8601())});
}

Status Store::BackupTo(const std::string& destination_path) {
    auto lock = Lock();
    sqlite3* destination = nullptr;
    int rc = sqlite3_open_v2(destination_path.c_str(), &destination,
                             SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, nullptr);
    if (rc != SQLITE_OK) {
        if (destination) sqlite3_close(destination);
        return Status::Error(ErrorCode::IoError, "cannot open backup destination");
    }
    sqlite3_backup* backup = sqlite3_backup_init(destination, "main", db_, "main");
    if (!backup) {
        sqlite3_close(destination);
        return SqliteStatus(db_, "sqlite3_backup_init failed");
    }
    do {
        rc = sqlite3_backup_step(backup, 64);
        if (rc == SQLITE_BUSY || rc == SQLITE_LOCKED) {
            sqlite3_sleep(10);
        }
    } while (rc == SQLITE_OK || rc == SQLITE_BUSY || rc == SQLITE_LOCKED);
    sqlite3_backup_finish(backup);
    sqlite3_close(destination);
    if (rc != SQLITE_DONE) {
        return Status::Error(ErrorCode::IoError, "backup did not complete");
    }
    return Status::Ok();
}

Expected<std::string> Store::QuickCheck() {
    auto rows = Query("PRAGMA quick_check;");
    if (!rows.ok()) return Expected<std::string>(rows.status());
    std::string result;
    for (const auto& row : rows.value()) {
        if (!result.empty()) result += "; ";
        result += row.Has("quick_check") ? row.Text("quick_check") : "unknown";
    }
    return result;
}

std::optional<std::string> Store::GetSetting(const std::string& key) {
    auto rows = Query("SELECT value FROM settings WHERE key = ?;", {SqlValue::Text(key)});
    if (!rows.ok() || rows.value().empty()) return std::nullopt;
    return rows.value()[0].Text("value");
}

Status Store::SetSetting(const std::string& key, const std::string& value) {
    return Exec("INSERT OR REPLACE INTO settings(key, value) VALUES(?, ?);",
                {SqlValue::Text(key), SqlValue::Text(value)});
}

}  // namespace mayasaba::storage
