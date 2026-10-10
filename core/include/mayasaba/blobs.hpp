// Content-addressed managed bytes (attachments, evidence artifacts).
//
// SQLite owns identity/provenance; these files hold the bytes and are verified against their
// recorded digest on every use. A missing or mismatched blob is an integrity failure.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "mayasaba/status.hpp"

namespace mayasaba::storage {

class BlobStore {
public:
    static Expected<BlobStore> Open(const std::string& root_dir);

    // Writes bytes under their SHA-256 and returns the digest. Idempotent.
    Expected<std::string> PutBytes(const std::vector<std::uint8_t>& bytes);
    Expected<std::string> PutText(const std::string& text);
    Expected<std::string> PutFile(const std::string& source_path);

    // Reads and verifies before returning; digest mismatch is INTEGRITY_FAILURE.
    Expected<std::vector<std::uint8_t>> Get(const std::string& sha256);
    Status Verify(const std::string& sha256);
    bool Exists(const std::string& sha256) const;
    std::string PathFor(const std::string& sha256) const;
    const std::string& root() const { return root_; }

private:
    std::string root_;
};

}  // namespace mayasaba::storage
