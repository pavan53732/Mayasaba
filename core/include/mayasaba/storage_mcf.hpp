// Storage-layer MCF repository: the ONLY place where MCF-v2 outbox/inbox/dead-letter SQL
// lives. The protocol layer calls these typed methods; SQLite Storage owns the schema and is
// the sole SQL executor. Ack/state mutations are verified by read-back so a persistence
// failure can never be reported as a successful acknowledgement.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "mayasaba/status.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::storage {

struct OutboxRecord {
    std::string message_id;
    std::string envelope;          // encoded MCF-v2 envelope JSON
    std::int64_t attempts = 0;
};

struct OutboxSummary {
    std::string message_id;
    std::string channel;
    std::string state;
    std::int64_t attempts = 0;
    std::string created_at;
    std::string acked_at;
};

struct DeadLetterSummary {
    std::string message_id;
    std::string project_id;
    std::string reason;
    std::string created_at;
};

struct InboxRecord {
    std::string message_id;
    std::string state;             // RECEIVED | ACKED
};

class McfRepository {
public:
    explicit McfRepository(Store* store) : store_(store) {}

    Expected<std::int64_t> NextOutboxSequence(const std::string& project_id,
                                              const std::string& sender,
                                              const std::string& channel);

    Status InsertOutbox(const std::string& message_id, const std::string& project_id,
                        const std::string& sender, const std::string& channel, int priority,
                        std::int64_t sequence, const std::string& envelope,
                        const std::string& created_at);

    // Persists the inbox record (idempotent). Fails when the row is not observable after the
    // write: a delivery must never proceed on an unpersisted receipt.
    Status RecordInbox(const std::string& message_id, const std::string& project_id,
                       const std::string& envelope, const std::string& received_at);

    Expected<std::optional<InboxRecord>> InboxRecordFor(const std::string& message_id);

    // Marks the inbox row ACKED and verifies by read-back; a missing/unupdated row is an
    // IoError, never a silent success.
    Status AckInboxVerified(const std::string& message_id, const std::string& at);

    Status MarkOutboxDeliveredVerified(const std::string& message_id, const std::string& at);

    Status MarkOutboxDeadLetteredVerified(const std::string& message_id, const std::string& reason,
                                          const std::string& at);

    Status InsertDeadLetter(const std::string& message_id, const std::string& project_id,
                            const std::string& envelope, const std::string& reason,
                            const std::string& created_at);

    Status RecordDeliveryAttemptFailure(const std::string& message_id,
                                        std::int64_t next_attempt_at);

    Expected<std::optional<OutboxRecord>> NextPendingOutbox(std::int64_t now_ms);

    // Envelope lookup for replay: outbox first, then dead letters.
    Expected<std::optional<std::string>> EnvelopeForReplay(const std::string& message_id);

    Expected<std::vector<OutboxSummary>> OutboxFor(const std::string& project_id,
                                                   const std::string& state);
    Expected<std::vector<DeadLetterSummary>> DeadLetters();

private:
    Store* store_;
};

}  // namespace mayasaba::storage
