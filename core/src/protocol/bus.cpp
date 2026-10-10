#include "mayasaba/bus.hpp"

#include <algorithm>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/jsonio.hpp"

namespace mayasaba::protocol {
namespace {

constexpr std::size_t kMaxEnvelopeBytes = 4u << 20;
constexpr int kMaxDeliveryAttempts = 8;
constexpr std::int64_t kBaseBackoffMillis = 200;
constexpr std::int64_t kMaxBackoffMillis = 5000;

int LaneIndex(Lane lane) { return static_cast<int>(lane); }

Expected<Lane> ParseLane(const std::string& name) {
    static const std::pair<const char*, Lane> kNames[] = {
        {"emergency_control", Lane::EmergencyControl},
        {"synchronization", Lane::Synchronization},
        {"task_control", Lane::TaskControl},
        {"failure_recovery", Lane::FailureRecovery},
        {"council", Lane::Council},
        {"progress_heartbeat", Lane::ProgressHeartbeat},
        {"bulk", Lane::Bulk},
    };
    for (const auto& [text, lane] : kNames) {
        if (name == text) return lane;
    }
    return Fail<Lane>(ErrorCode::SchemaViolation, "unknown lane: " + name);
}

}  // namespace

const char* LaneName(Lane lane) {
    switch (lane) {
        case Lane::EmergencyControl: return "emergency_control";
        case Lane::Synchronization: return "synchronization";
        case Lane::TaskControl: return "task_control";
        case Lane::FailureRecovery: return "failure_recovery";
        case Lane::Council: return "council";
        case Lane::ProgressHeartbeat: return "progress_heartbeat";
        case Lane::Bulk: return "bulk";
    }
    return "bulk";
}

std::vector<std::string> RegisteredMessageTypes() {
    return {
        "agent.session.started", "agent.session.ready", "agent.session.event",
        "agent.session.completed", "agent.session.failed", "agent.session.cancelled",
        "agent.prompt",
        "council.assignment.proposal", "council.assignment.critique",
        "council.assignment.rebuttal", "council.assignment.synthesis",
        "council.position.submitted", "council.critique.submitted",
        "council.synthesis.submitted", "council.round.sealed",
        "task.assignment", "task.result", "task.cancel",
        "exploration.assignment", "exploration.findings",
        "research.assignment", "research.findings",
        "sync.snapshot.published", "sync.barrier.confirmed",
        "control.pause", "control.stop",
        "project.created", "project.epoch.advanced", "contribution.persisted",
        "workrequest.authorized", "workrequest.blocked", "requirement.approved",
        "decision.point.opened", "decision.committed",
        "task.leased", "task.completed", "attempt.failed",
        "validation.result.recorded", "publication.applied", "publication.conflict",
        "evidence.recorded", "policy.denied", "agent.readiness.changed",
        "ordering.gap.detected", "mcf.deadletter",
    };
}

bool IsRegisteredMessageType(const std::string& type) {
    static const std::vector<std::string> kTypes = RegisteredMessageTypes();
    return std::find(kTypes.begin(), kTypes.end(), type) != kTypes.end();
}

Expected<std::string> EncodeEnvelope(const Envelope& envelope) {
    if (envelope.protocol_version != 2 || envelope.schema_version != "mcf-v2.1") {
        return Fail<std::string>(ErrorCode::SchemaViolation, "unsupported envelope version");
    }
    if (!IsRegisteredMessageType(envelope.message_type)) {
        return Fail<std::string>(ErrorCode::SchemaViolation,
                                 "unregistered message type: " + envelope.message_type);
    }
    if (!IsValidId(envelope.message_id) || !IsValidId(envelope.event_id)) {
        return Fail<std::string>(ErrorCode::SchemaViolation, "message/event id is not a UUID");
    }
    if (envelope.project_id.empty() || envelope.sender.empty() || envelope.channel.empty()) {
        return Fail<std::string>(ErrorCode::SchemaViolation,
                                 "envelope requires project, sender and channel");
    }
    if (!envelope.payload.is_object()) {
        return Fail<std::string>(ErrorCode::SchemaViolation, "payload must be a JSON object");
    }
    nlohmann::json encoded = {
        {"protocol_version", envelope.protocol_version},
        {"schema_version", envelope.schema_version},
        {"message_id", envelope.message_id},
        {"event_id", envelope.event_id},
        {"project_id", envelope.project_id},
        {"session_id", envelope.session_id},
        {"sender", envelope.sender},
        {"recipients", envelope.recipients},
        {"channel", envelope.channel},
        {"message_type", envelope.message_type},
        {"phase", envelope.phase},
        {"correlation_id", envelope.correlation_id},
        {"sequence", envelope.sequence},
        {"project_epoch", envelope.project_epoch},
        {"lane", LaneName(envelope.lane)},
        {"timestamp", envelope.timestamp},
        {"ack_required", envelope.ack_required},
        {"response_expected", envelope.response_expected},
        {"blocking", envelope.blocking},
        {"operation_id", envelope.operation_id},
        {"payload", envelope.payload},
        {"security", envelope.security},
    };
    std::string text = DumpCompact(encoded);
    if (text.size() > kMaxEnvelopeBytes) {
        return Fail<std::string>(ErrorCode::InvalidArgument, "envelope exceeds size limit");
    }
    // Canonical-encodability is a contract requirement: hashed payloads must round-trip.
    auto canonical = CanonicalDump(encoded);
    if (!canonical.ok()) return canonical;
    return text;
}

Expected<Envelope> DecodeEnvelope(const std::string& json_text) {
    if (json_text.size() > kMaxEnvelopeBytes) {
        return Fail<Envelope>(ErrorCode::InvalidArgument, "envelope exceeds size limit");
    }
    auto parsed = ParseJsonBounded(json_text);
    if (!parsed.ok()) return Fail<Envelope>(parsed.code(), parsed.message());
    const nlohmann::json& value = parsed.value();
    Envelope envelope;
    try {
        envelope.protocol_version = value.value("protocol_version", 0);
        envelope.schema_version = value.value("schema_version", std::string());
        envelope.message_id = value.value("message_id", std::string());
        envelope.event_id = value.value("event_id", std::string());
        envelope.project_id = value.value("project_id", std::string());
        envelope.session_id = value.value("session_id", std::string());
        envelope.sender = value.value("sender", std::string());
        if (value.contains("recipients") && value["recipients"].is_array()) {
            for (const auto& item : value["recipients"]) {
                if (!item.is_string()) {
                    return Fail<Envelope>(ErrorCode::SchemaViolation, "recipient is not a string");
                }
                envelope.recipients.push_back(item.get<std::string>());
            }
        }
        envelope.channel = value.value("channel", std::string());
        envelope.message_type = value.value("message_type", std::string());
        envelope.phase = value.value("phase", std::string());
        envelope.correlation_id = value.value("correlation_id", std::string());
        envelope.sequence = value.value("sequence", 0);
        envelope.project_epoch = value.value("project_epoch", 0);
        auto lane = ParseLane(value.value("lane", std::string("bulk")));
        if (!lane.ok()) return Fail<Envelope>(lane.code(), lane.message());
        envelope.lane = lane.value();
        envelope.timestamp = value.value("timestamp", std::string());
        envelope.ack_required = value.value("ack_required", true);
        envelope.response_expected = value.value("response_expected", false);
        envelope.blocking = value.value("blocking", false);
        envelope.operation_id = value.value("operation_id", std::string());
        if (value.contains("payload")) envelope.payload = value["payload"];
        if (value.contains("security")) envelope.security = value["security"];
    } catch (const nlohmann::json::exception& error) {
        return Fail<Envelope>(ErrorCode::SchemaViolation,
                              std::string("envelope field type error: ") + error.what());
    }
    if (envelope.protocol_version != 2) {
        return Fail<Envelope>(ErrorCode::SchemaViolation, "unsupported protocol version");
    }
    if (!IsRegisteredMessageType(envelope.message_type)) {
        return Fail<Envelope>(ErrorCode::SchemaViolation,
                              "unregistered message type: " + envelope.message_type);
    }
    return envelope;
}

Bus::Bus(storage::Store* store, std::size_t max_in_flight)
    : store_(store), repo_(store), max_in_flight_(max_in_flight) {}

Bus::~Bus() { StopDispatcher(); }

Status Bus::RegisterHandler(const std::string& message_type, MessageHandler handler) {
    if (!IsRegisteredMessageType(message_type)) {
        return Status::Error(ErrorCode::SchemaViolation,
                             "cannot register handler for unregistered type: " + message_type);
    }
    std::lock_guard<std::mutex> lock(mutex_);
    handlers_[message_type] = std::move(handler);
    return Status::Ok();
}

std::int64_t Bus::NextSequence(const std::string& project_id, const std::string& sender,
                               const std::string& channel) {
    auto next = repo_.NextOutboxSequence(project_id, sender, channel);
    if (!next.ok()) {
        NotePersistenceFailure("next outbox sequence", next.status());
        return 1;
    }
    return next.value();
}

Expected<Envelope> Bus::Send(Envelope envelope) {
    if (envelope.message_id.empty()) envelope.message_id = NewId("msg");
    if (envelope.event_id.empty()) envelope.event_id = NewId("evt");
    if (envelope.timestamp.empty()) envelope.timestamp = NowUtcIso8601();
    if (envelope.sender.empty()) envelope.sender = "controller";

    // Material-action deduplication: a duplicate can never repeat a side effect.
    if (!envelope.operation_id.empty()) {
        auto prior = store_->FindIdempotentResult(envelope.project_id, "mcf:" + envelope.operation_id);
        if (prior.has_value()) {
            ++counters_.deduplicated;
            return envelope;  // suppressed duplicate; caller sees the same identity
        }
    }

    bool own_transaction = !store_->InTransaction();
    std::optional<storage::Transaction> guard;
    if (own_transaction) {
        auto begun = store_->Begin();
        if (!begun.ok()) return Expected<Envelope>(begun.status());
        guard.emplace(std::move(begun.value()));
    }
    if (envelope.sequence == 0) {
        envelope.sequence = NextSequence(envelope.project_id, envelope.sender, envelope.channel);
    }
    auto encoded = EncodeEnvelope(envelope);
    if (!encoded.ok()) return Expected<Envelope>(encoded.status());
    auto status = repo_.InsertOutbox(envelope.message_id, envelope.project_id, envelope.sender,
                                     envelope.channel, LaneIndex(envelope.lane), envelope.sequence,
                                     encoded.value(), envelope.timestamp);
    if (!status.ok()) return Expected<Envelope>(status);
    if (guard.has_value()) {
        status = guard->Commit();
        if (!status.ok()) return Expected<Envelope>(status);
    }
    ++counters_.sent;
    cv_.notify_all();
    return envelope;
}

Status Bus::RecordInbox(const Envelope& envelope, const std::string& encoded) {
    return repo_.RecordInbox(envelope.message_id, envelope.project_id, encoded, NowUtcIso8601());
}

Status Bus::AckInbox(const std::string& message_id) {
    // An ACK means receipt and persistence. The repository verifies the state by read-back;
    // a failed ACK is never reported as success.
    return repo_.AckInboxVerified(message_id, NowUtcIso8601());
}

Status Bus::MarkOutbox(const std::string& message_id, const std::string& state,
                       const std::string& reason) {
    if (state == "DELIVERED") {
        return repo_.MarkOutboxDeliveredVerified(message_id, NowUtcIso8601());
    }
    if (state == "DEAD_LETTER") {
        return repo_.MarkOutboxDeadLetteredVerified(message_id, reason, NowUtcIso8601());
    }
    return Status::Error(ErrorCode::InvalidArgument, "unknown outbox state: " + state);
}

void Bus::NotePersistenceFailure(const std::string& what, const Status& status) {
    ++counters_.persistence_failures;
    std::lock_guard<std::mutex> lock(mutex_);
    persistence_notices_.push_back(what + ": " + RedactSecrets(status.message()));
}

Status Bus::DeadLetter(const Envelope& envelope, const std::string& encoded,
                       const std::string& reason) {
    Status first_failure = Status::Ok();
    auto inserted = repo_.InsertDeadLetter(envelope.message_id, envelope.project_id, encoded,
                                           RedactSecrets(reason), NowUtcIso8601());
    if (!inserted.ok()) {
        NotePersistenceFailure("dead-letter insert " + envelope.message_id, inserted);
        first_failure = inserted;
    }
    auto marked = MarkOutbox(envelope.message_id, "DEAD_LETTER", reason);
    if (!marked.ok()) {
        NotePersistenceFailure("dead-letter outbox mark " + envelope.message_id, marked);
        if (first_failure.ok()) first_failure = marked;
    }
    auto event = store_->AppendEvent(envelope.project_id, "mcf.deadletter",
                                     {{"message_id", envelope.message_id},
                                      {"message_type", envelope.message_type},
                                      {"reason", RedactSecrets(reason)}});
    if (!event.ok()) {
        NotePersistenceFailure("dead-letter event " + envelope.message_id, event.status());
        if (first_failure.ok()) first_failure = event.status();
    }
    if (marked.ok()) ++counters_.dead_lettered;
    return first_failure;
}

DeliveryResult Bus::Deliver(const Envelope& envelope, std::string* reject_reason) {
    // Ordering-gap detection: gaps are reported, never silently repaired. The gap event is
    // written outside the lock (the note helper takes the same mutex).
    bool gap_detected = false;
    std::int64_t gap_expected = 0;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        std::string key = envelope.project_id + "|" + envelope.sender + "|" + envelope.channel;
        if (envelope.sequence > 0) {
            auto it = last_inbound_sequence_.find(key);
            if (it != last_inbound_sequence_.end() && envelope.sequence > it->second + 1) {
                std::string notice = "ordering gap on " + key + ": expected " +
                                     std::to_string(it->second + 1) + ", received " +
                                     std::to_string(envelope.sequence);
                ordering_gap_notices_.push_back(notice);
                gap_detected = true;
                gap_expected = it->second + 1;
            }
            if (it == last_inbound_sequence_.end() || envelope.sequence > it->second) {
                last_inbound_sequence_[key] = envelope.sequence;
            }
        }
    }
    if (gap_detected) {
        auto event = store_->AppendEvent(envelope.project_id, "ordering.gap.detected",
                                         {{"channel", envelope.channel},
                                          {"sender", envelope.sender},
                                          {"expected", gap_expected},
                                          {"received", envelope.sequence}});
        if (!event.ok()) {
            NotePersistenceFailure("ordering gap event " + envelope.message_id, event.status());
        }
    }
    MessageHandler handler;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = handlers_.find(envelope.message_type);
        if (it == handlers_.end()) {
            if (reject_reason) {
                *reject_reason = "no handler registered for " + envelope.message_type;
            }
            return DeliveryResult::Rejected;
        }
        handler = it->second;
    }
    DeliveryContext context{envelope, reject_reason};
    return handler(context);
}

std::size_t Bus::PumpOnce(std::size_t budget) {
    std::size_t processed = 0;
    while (processed < budget) {
        auto pending = repo_.NextPendingOutbox(UnixTimeMillis());
        if (!pending.ok()) {
            NotePersistenceFailure("outbox poll", pending.status());
            break;
        }
        if (!pending.value().has_value()) break;
        const std::string message_id = pending.value()->message_id;
        const std::string encoded = pending.value()->envelope;
        const int attempts = static_cast<int>(pending.value()->attempts);
        auto envelope = DecodeEnvelope(encoded);
        if (!envelope.ok()) {
            ++counters_.rejected;
            auto marked = MarkOutbox(message_id, "DEAD_LETTER", "stored envelope unreadable");
            if (!marked.ok()) NotePersistenceFailure("unreadable envelope " + message_id, marked);
            ++processed;
            continue;
        }
        // Inbox-side dedup: an already-ACKed message is never delivered twice.
        auto inbox = repo_.InboxRecordFor(message_id);
        if (inbox.ok() && inbox.value().has_value() && inbox.value()->state == "ACKED") {
            auto marked = MarkOutbox(message_id, "DELIVERED");
            if (!marked.ok()) NotePersistenceFailure("outbox mark " + message_id, marked);
            ++processed;
            continue;
        }
        if (!inbox.ok()) {
            NotePersistenceFailure("inbox lookup " + message_id, inbox.status());
        }
        // The receipt must be persisted before delivery proceeds: a delivery on an
        // unpersisted receipt could be replayed as if never received.
        auto receipt = RecordInbox(envelope.value(), encoded);
        if (!receipt.ok()) {
            NotePersistenceFailure("inbox receipt " + message_id, receipt);
            auto backed_off = repo_.RecordDeliveryAttemptFailure(message_id,
                                                                 UnixTimeMillis() + 500);
            if (!backed_off.ok()) {
                NotePersistenceFailure("retry bookkeeping " + message_id, backed_off);
            }
            ++processed;
            continue;
        }
        std::string reject_reason;
        DeliveryResult result = Deliver(envelope.value(), &reject_reason);
        switch (result) {
            case DeliveryResult::Delivered: {
                auto acked = AckInbox(message_id);
                if (!acked.ok()) {
                    // The handler ran, but the acknowledgement is not persisted: the message
                    // stays PENDING and will be redelivered. Idempotency keys make the
                    // duplicate safe; the failure is recorded, never hidden.
                    NotePersistenceFailure("inbox ack " + message_id, acked);
                    ++counters_.retried;
                    auto backed_off = repo_.RecordDeliveryAttemptFailure(message_id,
                                                                         UnixTimeMillis() + 500);
                    if (!backed_off.ok()) {
                        NotePersistenceFailure("retry bookkeeping " + message_id, backed_off);
                    }
                    break;
                }
                auto marked = MarkOutbox(message_id, "DELIVERED");
                if (!marked.ok()) {
                    NotePersistenceFailure("outbox mark " + message_id, marked);
                }
                ++counters_.delivered;
                break;
            }
            case DeliveryResult::NotYet: {
                ++counters_.retried;
                if (attempts + 1 >= kMaxDeliveryAttempts) {
                    auto dead = DeadLetter(envelope.value(), encoded,
                                           "retry budget exhausted: " + reject_reason);
                    if (!dead.ok()) {
                        auto backed_off = repo_.RecordDeliveryAttemptFailure(
                            message_id, UnixTimeMillis() + kMaxBackoffMillis);
                        if (!backed_off.ok()) {
                            NotePersistenceFailure("retry bookkeeping " + message_id, backed_off);
                        }
                    }
                } else {
                    std::int64_t backoff = std::min<std::int64_t>(
                        kBaseBackoffMillis << std::min(attempts, 8), kMaxBackoffMillis);
                    auto backed_off = repo_.RecordDeliveryAttemptFailure(message_id,
                                                                         UnixTimeMillis() + backoff);
                    if (!backed_off.ok()) {
                        NotePersistenceFailure("retry bookkeeping " + message_id, backed_off);
                    }
                }
                break;
            }
            case DeliveryResult::Rejected:
                ++counters_.rejected;
                (void)DeadLetter(envelope.value(), encoded,
                                 reject_reason.empty() ? "rejected" : reject_reason);
                break;
        }
        ++processed;
    }
    if (processed >= budget) ++counters_.backpressure;
    return processed;
}

void Bus::StartDispatcher() {
    std::lock_guard<std::mutex> lock(mutex_);
    if (running_) return;
    running_ = true;
    dispatcher_ = std::thread([this] {
        while (true) {
            {
                std::unique_lock<std::mutex> lock(mutex_);
                if (!running_) return;
                cv_.wait_for(lock, std::chrono::milliseconds(50));
                if (!running_) return;
            }
            PumpOnce(128);
        }
    });
}

void Bus::StopDispatcher() {
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!running_) return;
        running_ = false;
    }
    cv_.notify_all();
    if (dispatcher_.joinable()) dispatcher_.join();
}

Expected<Envelope> Bus::Replay(const std::string& message_id, bool freshly_authorized) {
    auto stored = repo_.EnvelopeForReplay(message_id);
    if (!stored.ok()) return Expected<Envelope>(stored.status());
    if (!stored.value().has_value()) {
        return Fail<Envelope>(ErrorCode::NotFound, "message not found for replay: " + message_id);
    }
    auto original = DecodeEnvelope(stored.value().value());
    if (!original.ok()) return original;
    if (!original.value().operation_id.empty() && !freshly_authorized) {
        return Fail<Envelope>(ErrorCode::Denied,
                              "material message replay requires fresh authorization");
    }
    Envelope replayed = original.value();
    replayed.message_id = NewId("msg");
    replayed.event_id = NewId("evt");
    replayed.timestamp = NowUtcIso8601();
    replayed.correlation_id = original.value().message_id;
    replayed.security["replay"] = true;
    replayed.security["replay_of"] = original.value().message_id;
    return Send(std::move(replayed));
}

Expected<std::vector<nlohmann::json>> Bus::OutboxFor(const std::string& project_id,
                                                     const std::string& state) {
    auto rows = repo_.OutboxFor(project_id, state);
    if (!rows.ok()) return Expected<std::vector<nlohmann::json>>(rows.status());
    std::vector<nlohmann::json> out;
    for (const auto& row : rows.value()) {
        out.push_back({{"message_id", row.message_id},
                       {"channel", row.channel},
                       {"state", row.state},
                       {"attempts", row.attempts},
                       {"created_at", row.created_at},
                       {"acked_at", row.acked_at}});
    }
    return out;
}

Expected<std::vector<nlohmann::json>> Bus::DeadLetters() {
    auto rows = repo_.DeadLetters();
    if (!rows.ok()) return Expected<std::vector<nlohmann::json>>(rows.status());
    std::vector<nlohmann::json> out;
    for (const auto& row : rows.value()) {
        out.push_back({{"message_id", row.message_id},
                       {"project_id", row.project_id},
                       {"reason", row.reason},
                       {"created_at", row.created_at}});
    }
    return out;
}

std::vector<std::string> Bus::TakeOrderingGapNotices() {
    std::lock_guard<std::mutex> lock(mutex_);
    std::vector<std::string> notices;
    notices.swap(ordering_gap_notices_);
    return notices;
}

std::vector<std::string> Bus::TakePersistenceNotices() {
    std::lock_guard<std::mutex> lock(mutex_);
    std::vector<std::string> notices;
    notices.swap(persistence_notices_);
    return notices;
}

}  // namespace mayasaba::protocol
