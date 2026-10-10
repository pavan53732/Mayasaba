#include "mayasaba/blobs.hpp"

#include "mayasaba/base.hpp"
#include "mayasaba/fs.hpp"

namespace mayasaba::storage {

Expected<BlobStore> BlobStore::Open(const std::string& root_dir) {
    auto status = fs::EnsureDirectory(root_dir);
    if (!status.ok()) return Expected<BlobStore>(status);
    BlobStore store;
    store.root_ = root_dir;
    return store;
}

std::string BlobStore::PathFor(const std::string& sha256) const {
    if (sha256.size() < 4) return fs::JoinPath(root_, sha256);
    return fs::JoinPath(fs::JoinPath(root_, sha256.substr(0, 2)), sha256);
}

bool BlobStore::Exists(const std::string& sha256) const {
    return fs::PathExists(PathFor(sha256));
}

Expected<std::string> BlobStore::PutBytes(const std::vector<std::uint8_t>& bytes) {
    auto digest = Sha256::HexOf(bytes.data(), bytes.size());
    if (!digest.ok()) return digest;
    std::string path = PathFor(digest.value());
    if (!fs::PathExists(path)) {
        auto status = fs::WriteFileBytes(path, bytes);
        if (!status.ok()) return Expected<std::string>(status);
    }
    return digest.value();
}

Expected<std::string> BlobStore::PutText(const std::string& text) {
    std::vector<std::uint8_t> bytes(text.begin(), text.end());
    return PutBytes(bytes);
}

Expected<std::string> BlobStore::PutFile(const std::string& source_path) {
    auto bytes = fs::ReadFileBytes(source_path);
    if (!bytes.ok()) return Expected<std::string>(bytes.status());
    return PutBytes(bytes.value());
}

Expected<std::vector<std::uint8_t>> BlobStore::Get(const std::string& sha256) {
    std::string path = PathFor(sha256);
    if (!fs::PathExists(path)) {
        return Fail<std::vector<std::uint8_t>>(ErrorCode::IntegrityFailure,
                                               "managed blob is missing: " + sha256);
    }
    auto bytes = fs::ReadFileBytes(path);
    if (!bytes.ok()) return bytes;
    auto digest = Sha256::HexOf(bytes.value().data(), bytes.value().size());
    if (!digest.ok()) return bytes;
    if (digest.value() != sha256) {
        return Fail<std::vector<std::uint8_t>>(ErrorCode::IntegrityFailure,
                                               "managed blob digest mismatch: " + sha256);
    }
    return bytes;
}

Status BlobStore::Verify(const std::string& sha256) {
    auto bytes = Get(sha256);
    if (!bytes.ok()) return bytes.status();
    return Status::Ok();
}

}  // namespace mayasaba::storage
