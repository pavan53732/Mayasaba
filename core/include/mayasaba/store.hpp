// SQLite storage layer (layer 13): the only component that writes SQL.
//
// One connection, one serialized writer path, WAL as a locally verified candidate with the
// actual journal mode recorded, online-backup support, a migration registry, the append-only
// per-project event chain and the idempotency ledger.
#pragma once

#include <cstdint>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include <nlohmann/json.hpp>

#include "mayasaba/status.hpp"

struct sqlite3;

namespace mayasaba::storage {

// --- Small SQL value/row helpers ---------------------------------------------------------
struct SqlValue {
    enum class Kind { Null, Int, Text };
    Kind kind = Kind::Null;
    std::int64_t integer = 0;
    std::string text;

    static SqlValue Null() { return {}; }
    static SqlValue Int(std::int64_t v) {
        SqlValue value;
        value.kind = Kind::Int;
        value.integer = v;
        return value;
    }
    static SqlValue Text(std::string v) {
        SqlValue value;
        value.kind = Kind::Text;
        value.text = std::move(v);
        return value;
    }
};

class Row {
public:
    std::int64_t Int(const std::string& column) const;
    std::string Text(const std::string& column) const;
    bool IsNull(const std::string& column) const;
    bool Has(const std::string& column) const;
    const std::unordered_map<std::string, SqlValue>& fields() const { return fields_; }

    std::unordered_map<std::string, SqlValue> fields_;
};

struct EventRecord {
    std::int64_t seq = 0;
    std::string project_id;
    std::string event_id;
    std::string type;
    std::string payload_json;
    std::string prev_hash;
    std::string hash;
    std::string created_at;
};

class Store;

class Transaction {
public:
    Transaction(Transaction&& other) noexcept;
    Transaction& operator=(Transaction&& other) noexcept;
    ~Transaction();

    Status Commit();
    void Rollback();
    bool active() const { return store_ != nullptr; }

private:
    friend class Store;
    explicit Transaction(Store* store) : store_(store) {}
    Store* store_ = nullptr;
    bool finished_ = false;
};

class Store {
public:
    static Expected<std::unique_ptr<Store>> Open(const std::string& db_path);
    ~Store();

    Store(const Store&) = delete;
    Store& operator=(const Store&) = delete;

    // Serialized access guard. Every public read/write takes this lock; helpers assume it is
    // already held so a service can compose several statements into one consistent view.
    std::unique_lock<std::recursive_mutex> Lock();

    // SQL boundary (used by owner services only).
    Expected<std::vector<Row>> Query(const std::string& sql,
                                     const std::vector<SqlValue>& params = {});
    Status Exec(const std::string& sql, const std::vector<SqlValue>& params = {});
    std::int64_t LastInsertRowId();
    int Changes();

    // Transactions.
    Expected<Transaction> Begin();
    bool InTransaction() const { return in_transaction_; }

    // Append-only event chain (per project, SHA-256 linked).
    Expected<EventRecord> AppendEvent(const std::string& project_id, const std::string& type,
                                      const nlohmann::json& payload);
    Expected<std::vector<EventRecord>> EventsFor(const std::string& project_id,
                                                 std::int64_t after_seq = 0,
                                                 std::int64_t limit = 1000);
    // Verifies the whole chain; returns the first inconsistent sequence number on failure.
    Status VerifyChain(const std::string& project_id, std::int64_t* first_bad_seq = nullptr);
    std::int64_t EventCount(const std::string& project_id);

    // Idempotency ledger (project + operation identity).
    std::optional<nlohmann::json> FindIdempotentResult(const std::string& project_id,
                                                       const std::string& operation_id);
    Status RecordIdempotentResult(const std::string& project_id, const std::string& operation_id,
                                  const nlohmann::json& result);

    // Durability and maintenance.
    Status BackupTo(const std::string& destination_path);
    Expected<std::string> QuickCheck();
    std::string JournalMode() const { return journal_mode_; }
    int SchemaVersion() const { return schema_version_; }
    const std::string& path() const { return path_; }

    // Settings (Mayasaba-owned configuration; nearest layer wins).
    std::optional<std::string> GetSetting(const std::string& key);
    Status SetSetting(const std::string& key, const std::string& value);

private:
    friend class Store;
    friend class Transaction;
    Store() = default;
    Status ApplyMigrations();
    Status ExecRaw(const std::string& sql);

    sqlite3* db_ = nullptr;
    std::string path_;
    std::string journal_mode_ = "unknown";
    int schema_version_ = 0;
    std::recursive_mutex mutex_;
    bool in_transaction_ = false;
};

// Current embedded schema version (migration registry).
constexpr int kSchemaVersion = 2;

}  // namespace mayasaba::storage
