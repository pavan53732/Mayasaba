// MCF-v2 communication fabric (layer 3).
//
// Typed, versioned envelopes over bounded in-process dispatch queues. Delivery is at least
// once, made safe by idempotency keys; an ACK means receipt and persistence only. Agent/bus
// traffic is distinct from the typed UI command/query boundary (see application/controller).
#pragma once

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <deque>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"
#include "mayasaba/storage_mcf.hpp"
#include "mayasaba/store.hpp"

namespace mayasaba::protocol {

// Priority lanes, highest first.
enum class Lane {
    EmergencyControl = 0,
    Synchronization = 1,
    TaskControl = 2,
    FailureRecovery = 3,
    Council = 4,
    ProgressHeartbeat = 5,
    Bulk = 6,
};

const char* LaneName(Lane lane);

struct Envelope {
    int protocol_version = 2;
    std::string schema_version = "mcf-v2.1";
    std::string message_id;
    std::string event_id;
    std::string project_id;
    std::string session_id;          // agent session, when applicable
    std::string sender;              // "controller" | agent id
    std::vector<std::string> recipients;
    std::string channel;             // "council" | "task" | "control" | "agent" | ...
    std::string message_type;        // registered type name
    std::string phase;               // lifecycle phase hint (advisory)
    std::string correlation_id;
    std::int64_t sequence = 0;       // per (project, sender, channel)
    std::int64_t project_epoch = 0;
    Lane lane = Lane::Bulk;
    std::string timestamp;
    bool ack_required = true;
    bool response_expected = false;
    bool blocking = false;
    std::string operation_id;        // material actions: idempotency identity
    nlohmann::json payload = nlohmann::json::object();
    nlohmann::json security = nlohmann::json::object();
};

// Serializes/validates envelopes against the registry (bounded, schema-checked).
Expected<std::string> EncodeEnvelope(const Envelope& envelope);
Expected<Envelope> DecodeEnvelope(const std::string& json_text);

struct RejectionReason {
    std::string message_id;
    std::string reason;
};

// Handler result: delivered (ACK) or not-yet (retry) or rejected (dead letter).
enum class DeliveryResult { Delivered, NotYet, Rejected };

struct DeliveryContext {
    const Envelope& envelope;
    // Set a reject reason for Rejected deliveries.
    std::string* reject_reason = nullptr;
};

using MessageHandler = std::function<DeliveryResult(DeliveryContext&)>;

class Bus {
public:
    // The bus persists through the storage owner. `max_in_flight` bounds the queue; pressure
    // answers "not yet" instead of dropping material events.
    Bus(storage::Store* store, std::size_t max_in_flight = 4096);
    ~Bus();

    Bus(const Bus&) = delete;
    Bus& operator=(const Bus&) = delete;

    // Registers the single handler for a message type. Registration is controller-owned;
    // adapters and services register through their owning service.
    Status RegisterHandler(const std::string& message_type, MessageHandler handler);

    // Sends a message: persists outbox + envelope atomically with the caller's transaction
    // when one is active, then queues it for delivery. Material messages (operation_id set)
    // are deduplicated by (project, operation) so a duplicate cannot repeat a side effect.
    Expected<Envelope> Send(Envelope envelope);

    // Runs one delivery pass synchronously (used by tests and by the dispatcher thread).
    // Returns the number of messages processed.
    std::size_t PumpOnce(std::size_t budget = 64);

    // Starts/stops the background dispatcher thread.
    void StartDispatcher();
    void StopDispatcher();

    // Explicit controller-invoked replay: creates a NEW message with a new id and refuses
    // material actions unless freshly authorized by the caller (authorized=false blocks it).
    Expected<Envelope> Replay(const std::string& message_id, bool freshly_authorized);

    // Outbox/inbox inspection (diagnostics; redacted).
    Expected<std::vector<nlohmann::json>> OutboxFor(const std::string& project_id,
                                                    const std::string& state = "");
    Expected<std::vector<nlohmann::json>> DeadLetters();
    std::int64_t NextSequence(const std::string& project_id, const std::string& sender,
                              const std::string& channel);

    // Ordering-gap detection: records a gap event when an inbound sequence skips.
    std::vector<std::string> TakeOrderingGapNotices();

    // Persistence diagnostics: every failed outbox/inbox/dead-letter write is recorded here
    // (and counted) instead of being silently discarded.
    std::vector<std::string> TakePersistenceNotices();

    struct Counters {
        std::atomic<std::uint64_t> sent{0};
        std::atomic<std::uint64_t> delivered{0};
        std::atomic<std::uint64_t> retried{0};
        std::atomic<std::uint64_t> dead_lettered{0};
        std::atomic<std::uint64_t> rejected{0};
        std::atomic<std::uint64_t> deduplicated{0};
        std::atomic<std::uint64_t> backpressure{0};
        std::atomic<std::uint64_t> persistence_failures{0};
    };
    const Counters& counters() const { return counters_; }

private:
    struct QueuedMessage {
        Envelope envelope;
        int attempts = 0;
    };

    DeliveryResult Deliver(const Envelope& envelope, std::string* reject_reason);
    Status DeadLetter(const Envelope& envelope, const std::string& encoded,
                      const std::string& reason);
    Status RecordInbox(const Envelope& envelope, const std::string& encoded);
    Status AckInbox(const std::string& message_id);
    Status MarkOutbox(const std::string& message_id, const std::string& state,
                      const std::string& reason = "");
    void NotePersistenceFailure(const std::string& what, const Status& status);

    storage::Store* store_;
    storage::McfRepository repo_;
    std::size_t max_in_flight_;
    std::mutex mutex_;
    std::condition_variable cv_;
    std::deque<QueuedMessage> queue_[7];  // one deque per lane
    std::map<std::string, MessageHandler> handlers_;
    std::map<std::string, std::int64_t> last_inbound_sequence_;  // project|sender|channel
    std::vector<std::string> ordering_gap_notices_;
    std::vector<std::string> persistence_notices_;
    std::thread dispatcher_;
    bool running_ = false;
    Counters counters_;
};

// Registered message types (machine-readable registry in contracts/registry/messages.json).
// The bus rejects unregistered types so a typo cannot silently create a new channel.
std::vector<std::string> RegisteredMessageTypes();
bool IsRegisteredMessageType(const std::string& type);

}  // namespace mayasaba::protocol
