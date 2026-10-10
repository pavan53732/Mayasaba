// Mayasaba core: registered error codes and boundary results.
//
// Every service boundary returns a Status/Expected instead of throwing across layers;
// platform and library exceptions are translated into these codes (AGENTS.md section 12).
#pragma once

#include <optional>
#include <string>
#include <utility>

namespace mayasaba {

// Registered error codes. These are the boundary errors referenced by contract tests; the
// string names are stable and machine-checkable.
enum class ErrorCode {
    Ok = 0,
    InvalidArgument,
    InvalidJson,
    SchemaViolation,
    NotFound,
    Conflict,
    Denied,             // policy denial; the reason is recorded
    Unavailable,        // dependency/CLI/agent not currently available
    Unsupported,        // observed behavior cannot be safely supported
    ProbeFailed,
    Stale,              // stale context, lease, digest or evidence
    IntegrityFailure,   // digest mismatch / corrupted managed bytes
    Blocked,            // fail-closed: required proof or capability missing
    BudgetExceeded,
    NotReady,
    AlreadyExists,
    IdempotentDuplicate,
    IoError,
    Internal,
};

const char* ErrorCodeName(ErrorCode code);

class Status {
public:
    Status() = default;
    Status(ErrorCode code, std::string message) : code_(code), message_(std::move(message)) {}

    static Status Ok() { return Status(); }
    static Status Error(ErrorCode code, std::string message) { return Status(code, std::move(message)); }

    bool ok() const { return code_ == ErrorCode::Ok; }
    ErrorCode code() const { return code_; }
    const std::string& message() const { return message_; }

    std::string ToString() const {
        if (ok()) return "OK";
        return std::string(ErrorCodeName(code_)) + ": " + message_;
    }

private:
    ErrorCode code_ = ErrorCode::Ok;
    std::string message_;
};

template <class T>
class Expected {
public:
    Expected(T value) : value_(std::move(value)) {}                    // NOLINT(google-explicit-constructor)
    Expected(Status status) : status_(std::move(status)) {}            // NOLINT(google-explicit-constructor)

    bool ok() const { return status_.ok(); }
    explicit operator bool() const { return ok(); }

    const Status& status() const { return status_; }
    const ErrorCode code() const { return status_.code(); }
    const std::string& message() const { return status_.message(); }

    T& value() { return *value_; }
    const T& value() const { return *value_; }
    T&& take() { return std::move(*value_); }
    T value_or(T fallback) const { return value_ ? *value_ : std::move(fallback); }
    const T& operator*() const { return *value_; }
    T& operator*() { return *value_; }
    const T* operator->() const { return &*value_; }
    T* operator->() { return &*value_; }

private:
    std::optional<T> value_;
    Status status_;
};

// Convenience for functions that already return Expected<T>.
template <class T>
Expected<T> Fail(ErrorCode code, std::string message) {
    return Expected<T>(Status::Error(code, std::move(message)));
}

}  // namespace mayasaba
