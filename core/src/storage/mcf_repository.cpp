#include "mayasaba/storage_mcf.hpp"

namespace mayasaba::storage {
namespace {

using SqlValue = storage::SqlValue;

Status VerifyReadback(Store* store, const std::string& sql, const std::vector<SqlValue>& params,
                      const std::string& column, const std::string& expected,
                      const std::string& what) {
    auto rows = store->Query(sql, params);
    if (!rows.ok()) return Status::Error(rows.code(), rows.message());
    if (rows.value().empty()) {
        return Status::Error(ErrorCode::IoError, what + ": row not found after write");
    }
    const std::string actual = rows.value()[0].Text(column);
    if (actual != expected) {
        return Status::Error(ErrorCode::IoError,
                             what + ": read-back state is '" + actual + "', expected '" +
                                 expected + "'");
    }
    return Status::Ok();
}

}  // namespace

Expected<std::int64_t> McfRepository::NextOutboxSequence(const std::string& project_id,
                                                         const std::string& sender,
                                                         const std::string& channel) {
    auto rows = store_->Query(
        "SELECT COALESCE(MAX(sequence), 0) + 1 AS next FROM mcf_outbox "
        "WHERE project_id = ? AND sender = ? AND channel = ?;",
        {SqlValue::Text(project_id), SqlValue::Text(sender), SqlValue::Text(channel)});
    if (!rows.ok()) return Expected<std::int64_t>(rows.status());
    if (rows.value().empty()) return std::int64_t{1};
    return rows.value()[0].Int("next");
}

Status McfRepository::InsertOutbox(const std::string& message_id, const std::string& project_id,
                                   const std::string& sender, const std::string& channel,
                                   int priority, std::int64_t sequence,
                                   const std::string& envelope, const std::string& created_at) {
    return store_->Exec(
        "INSERT INTO mcf_outbox(message_id, project_id, sender, channel, priority, sequence, "
        "envelope, state, attempts, next_attempt_at, created_at) "
        "VALUES(?,?,?,?,?,?,?,'PENDING',0,0,?);",
        {SqlValue::Text(message_id), SqlValue::Text(project_id), SqlValue::Text(sender),
         SqlValue::Text(channel), SqlValue::Int(priority), SqlValue::Int(sequence),
         SqlValue::Text(envelope), SqlValue::Text(created_at)});
}

Status McfRepository::RecordInbox(const std::string& message_id, const std::string& project_id,
                                  const std::string& envelope, const std::string& received_at) {
    auto written = store_->Exec(
        "INSERT OR IGNORE INTO mcf_inbox(message_id, project_id, envelope, received_at, state) "
        "VALUES(?,?,?,?,'RECEIVED');",
        {SqlValue::Text(message_id), SqlValue::Text(project_id), SqlValue::Text(envelope),
         SqlValue::Text(received_at)});
    if (!written.ok()) return written;
    // The receipt must be observable before any delivery proceeds.
    auto rows = store_->Query("SELECT state FROM mcf_inbox WHERE message_id=?;",
                              {SqlValue::Text(message_id)});
    if (!rows.ok()) return Status::Error(rows.code(), rows.message());
    if (rows.value().empty()) {
        return Status::Error(ErrorCode::IoError,
                             "inbox receipt not observable after write for " + message_id);
    }
    return Status::Ok();
}

Expected<std::optional<InboxRecord>> McfRepository::InboxRecordFor(const std::string& message_id) {
    auto rows = store_->Query("SELECT message_id, state FROM mcf_inbox WHERE message_id=?;",
                              {SqlValue::Text(message_id)});
    if (!rows.ok()) return Expected<std::optional<InboxRecord>>(rows.status());
    if (rows.value().empty()) return std::optional<InboxRecord>{};
    InboxRecord record;
    record.message_id = rows.value()[0].Text("message_id");
    record.state = rows.value()[0].Text("state");
    return std::optional<InboxRecord>(std::move(record));
}

Status McfRepository::AckInboxVerified(const std::string& message_id, const std::string& at) {
    auto written = store_->Exec(
        "UPDATE mcf_inbox SET state='ACKED', acked_at=?, processed_at=? WHERE message_id=?;",
        {SqlValue::Text(at), SqlValue::Text(at), SqlValue::Text(message_id)});
    if (!written.ok()) return written;
    return VerifyReadback(store_, "SELECT state FROM mcf_inbox WHERE message_id=?;",
                          {SqlValue::Text(message_id)}, "state", "ACKED",
                          "inbox ack " + message_id);
}

Status McfRepository::MarkOutboxDeliveredVerified(const std::string& message_id,
                                                  const std::string& at) {
    auto written = store_->Exec(
        "UPDATE mcf_outbox SET state='DELIVERED', acked_at=? WHERE message_id=?;",
        {SqlValue::Text(at), SqlValue::Text(message_id)});
    if (!written.ok()) return written;
    return VerifyReadback(store_, "SELECT state FROM mcf_outbox WHERE message_id=?;",
                          {SqlValue::Text(message_id)}, "state", "DELIVERED",
                          "outbox delivery " + message_id);
}

Status McfRepository::MarkOutboxDeadLetteredVerified(const std::string& message_id,
                                                     const std::string& reason,
                                                     const std::string& at) {
    auto written = store_->Exec(
        "UPDATE mcf_outbox SET state='DEAD_LETTER', dead_lettered_at=?, reason=? "
        "WHERE message_id=?;",
        {SqlValue::Text(at), SqlValue::Text(reason), SqlValue::Text(message_id)});
    if (!written.ok()) return written;
    return VerifyReadback(store_, "SELECT state FROM mcf_outbox WHERE message_id=?;",
                          {SqlValue::Text(message_id)}, "state", "DEAD_LETTER",
                          "outbox dead-letter " + message_id);
}

Status McfRepository::InsertDeadLetter(const std::string& message_id,
                                       const std::string& project_id,
                                       const std::string& envelope, const std::string& reason,
                                       const std::string& created_at) {
    return store_->Exec(
        "INSERT OR REPLACE INTO mcf_dead_letters(message_id, project_id, envelope, reason, "
        "created_at) VALUES(?,?,?,?,?);",
        {SqlValue::Text(message_id), SqlValue::Text(project_id), SqlValue::Text(envelope),
         SqlValue::Text(reason), SqlValue::Text(created_at)});
}

Status McfRepository::RecordDeliveryAttemptFailure(const std::string& message_id,
                                                   std::int64_t next_attempt_at) {
    return store_->Exec(
        "UPDATE mcf_outbox SET attempts=attempts+1, next_attempt_at=? WHERE message_id=?;",
        {SqlValue::Int(next_attempt_at), SqlValue::Text(message_id)});
}

Expected<std::optional<OutboxRecord>> McfRepository::NextPendingOutbox(std::int64_t now_ms) {
    auto rows = store_->Query(
        "SELECT message_id, envelope, attempts FROM mcf_outbox "
        "WHERE state='PENDING' AND next_attempt_at <= ? "
        "ORDER BY priority ASC, sequence ASC LIMIT 1;",
        {SqlValue::Int(now_ms)});
    if (!rows.ok()) return Expected<std::optional<OutboxRecord>>(rows.status());
    if (rows.value().empty()) return std::optional<OutboxRecord>{};
    OutboxRecord record;
    record.message_id = rows.value()[0].Text("message_id");
    record.envelope = rows.value()[0].Text("envelope");
    record.attempts = rows.value()[0].Int("attempts");
    return std::optional<OutboxRecord>(std::move(record));
}

Expected<std::optional<std::string>> McfRepository::EnvelopeForReplay(
    const std::string& message_id) {
    auto rows = store_->Query("SELECT envelope FROM mcf_outbox WHERE message_id=?;",
                              {SqlValue::Text(message_id)});
    if (!rows.ok()) return Expected<std::optional<std::string>>(rows.status());
    if (rows.value().empty()) {
        rows = store_->Query("SELECT envelope FROM mcf_dead_letters WHERE message_id=?;",
                             {SqlValue::Text(message_id)});
        if (!rows.ok()) return Expected<std::optional<std::string>>(rows.status());
    }
    if (rows.value().empty()) return std::optional<std::string>{};
    return std::optional<std::string>(rows.value()[0].Text("envelope"));
}

Expected<std::vector<OutboxSummary>> McfRepository::OutboxFor(const std::string& project_id,
                                                              const std::string& state) {
    auto rows = state.empty()
                    ? store_->Query(
                          "SELECT message_id, channel, state, attempts, created_at, acked_at "
                          "FROM mcf_outbox WHERE project_id=? ORDER BY sequence ASC;",
                          {SqlValue::Text(project_id)})
                    : store_->Query(
                          "SELECT message_id, channel, state, attempts, created_at, acked_at "
                          "FROM mcf_outbox WHERE project_id=? AND state=? ORDER BY sequence ASC;",
                          {SqlValue::Text(project_id), SqlValue::Text(state)});
    if (!rows.ok()) return Expected<std::vector<OutboxSummary>>(rows.status());
    std::vector<OutboxSummary> out;
    out.reserve(rows.value().size());
    for (const auto& row : rows.value()) {
        OutboxSummary summary;
        summary.message_id = row.Text("message_id");
        summary.channel = row.Text("channel");
        summary.state = row.Text("state");
        summary.attempts = row.Int("attempts");
        summary.created_at = row.Text("created_at");
        summary.acked_at = row.Text("acked_at");
        out.push_back(std::move(summary));
    }
    return out;
}

Expected<std::vector<DeadLetterSummary>> McfRepository::DeadLetters() {
    auto rows = store_->Query(
        "SELECT message_id, project_id, reason, created_at FROM mcf_dead_letters ORDER BY "
        "created_at ASC;");
    if (!rows.ok()) return Expected<std::vector<DeadLetterSummary>>(rows.status());
    std::vector<DeadLetterSummary> out;
    out.reserve(rows.value().size());
    for (const auto& row : rows.value()) {
        DeadLetterSummary summary;
        summary.message_id = row.Text("message_id");
        summary.project_id = row.Text("project_id");
        summary.reason = row.Text("reason");
        summary.created_at = row.Text("created_at");
        out.push_back(std::move(summary));
    }
    return out;
}

}  // namespace mayasaba::storage
