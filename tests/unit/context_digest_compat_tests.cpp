// Snapshot digest compatibility tests: versioned digest profiles and legacy (pre-change)
// snapshot recovery.
//
// Every snapshot row records the digest profile its stored digest was hashed under. The loader
// recomputes under exactly that profile and fails closed on any mismatch or unknown profile.
// Profile 1 is the original pre-change shape (entries without "reason", no profile field in the
// hashed input); profile 2 includes each entry's reason and tags the input with the version.
//
// These tests construct the pre-change digest independently (test-side, from the exact original
// JSON shape) so agreement with the production legacy helper is the oracle — the test does not
// share code with the implementation it is checking.
#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <string>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/canonical.hpp"
#include "mayasaba/context.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/workspace.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::context;
using mayasaba::test::ScratchDir;

namespace {

using storage::SqlValue;

// The EXACT pre-change digest input: entries {coverage, rel_path, sha256, size} (no "reason")
// and the top-level keys {entries, epoch, kind, manifest_digest, project_id} (no profile field).
Expected<std::string> OriginalShapeDigest(const std::string& project_id, std::int64_t epoch,
                                          const std::string& kind,
                                          const std::string& manifest_digest,
                                          const std::vector<SnapshotEntry>& entries) {
    nlohmann::json json_entries = nlohmann::json::array();
    for (const auto& entry : entries) {
        json_entries.push_back({{"coverage", entry.coverage},
                                {"rel_path", entry.rel_path},
                                {"sha256", entry.sha256},
                                {"size", entry.size}});
    }
    nlohmann::json input = {{"entries", json_entries},
                            {"epoch", epoch},
                            {"kind", kind},
                            {"manifest_digest", manifest_digest},
                            {"project_id", project_id}};
    return CanonicalDigest(input);
}

// The profile-2 digest input for the same content, constructed independently.
Expected<std::string> Profile2ShapeDigest(const std::string& project_id, std::int64_t epoch,
                                          const std::string& kind,
                                          const std::string& manifest_digest,
                                          const std::vector<SnapshotEntry>& entries) {
    nlohmann::json json_entries = nlohmann::json::array();
    for (const auto& entry : entries) {
        json_entries.push_back({{"coverage", entry.coverage},
                                {"reason", entry.reason},
                                {"rel_path", entry.rel_path},
                                {"sha256", entry.sha256},
                                {"size", entry.size}});
    }
    nlohmann::json input = {{"digest_profile", 2},
                            {"entries", json_entries},
                            {"epoch", epoch},
                            {"kind", kind},
                            {"manifest_digest", manifest_digest},
                            {"project_id", project_id}};
    return CanonicalDigest(input);
}

std::vector<SnapshotEntry> LegacyEntries() {
    std::vector<SnapshotEntry> entries;
    entries.push_back({"src/a.cpp", std::string(64, 'a'), 120, "CONTENT_INSPECTED", ""});
    // A negative-coverage entry carries a reason; the ORIGINAL digest ignored it.
    entries.push_back({"secret.env", "", 0, "EXCLUDED", "excluded by rule (.env)"});
    // The original writer sorted entries by rel_path before hashing (and the loader reads them
    // in the same order), so the digest fixture must use the identical order.
    std::sort(entries.begin(), entries.end(),
              [](const SnapshotEntry& a, const SnapshotEntry& b) {
                  return a.rel_path < b.rel_path;
              });
    return entries;
}

// Inserts a pre-change snapshot row and its entries through the storage API, exactly as the old
// writer would have persisted them (the digest_profile column, if present, defaults to 1).
void InsertLegacySnapshot(storage::Store* store, const std::string& snapshot_id,
                          const std::string& digest, const std::vector<SnapshotEntry>& entries) {
    ASSERT_TRUE(store
                    ->Exec("INSERT INTO snapshots(snapshot_id, project_id, epoch, kind, "
                           "manifest_digest, digest, entry_count, excluded_count, truncated, "
                           "created_at) VALUES(?,?,?,?,?,?,?,?,?,?);",
                           {SqlValue::Text(snapshot_id), SqlValue::Text("proj-legacy"),
                            SqlValue::Int(7), SqlValue::Text("REPOSITORY"),
                            SqlValue::Text("manifest-md"), SqlValue::Text(digest),
                            SqlValue::Int(static_cast<std::int64_t>(entries.size())),
                            SqlValue::Int(1), SqlValue::Int(0),
                            SqlValue::Text("2026-01-01T00:00:00Z")})
                    .ok());
    for (const auto& entry : entries) {
        ASSERT_TRUE(store
                        ->Exec("INSERT INTO snapshot_entries(snapshot_id, rel_path, kind, size, "
                               "sha256, coverage, reason) VALUES(?,?,?,?,?,?,?);",
                               {SqlValue::Text(snapshot_id), SqlValue::Text(entry.rel_path),
                                SqlValue::Text("FILE"),
                                SqlValue::Int(static_cast<std::int64_t>(entry.size)),
                                SqlValue::Text(entry.sha256), SqlValue::Text(entry.coverage),
                                SqlValue::Text(entry.reason)})
                        .ok());
    }
}

bool TableHasColumn(storage::Store* store, const std::string& table, const std::string& column) {
    auto info = store->Query("PRAGMA table_info(" + table + ");");
    if (!info.ok()) return false;
    for (const auto& row : info.value()) {
        if (row.Text("name") == column) return true;
    }
    return false;
}

}  // namespace

// A database in the actual pre-change (V3) shape — max applied migration 3 and a snapshots
// table without digest_profile — must upgrade through the V4 migration, backfill the column to
// profile 1, and recover a genuine legacy snapshot without rewriting its history.
TEST(ContextDigestCompat, LegacySnapshotFromV3SchemaUpgradesAndVerifies) {
    ScratchDir scratch;
    const std::string db_path = scratch.File("v3_legacy.db");
    const auto entries = LegacyEntries();
    auto legacy_digest =
        OriginalShapeDigest("proj-legacy", 7, "REPOSITORY", "manifest-md", entries);
    ASSERT_TRUE(legacy_digest.ok()) << legacy_digest.message();

    // Phase 1: build a genuine pre-change database.
    {
        auto store = storage::Store::Open(db_path);
        ASSERT_TRUE(store.ok()) << store.message();
        // Recreate the snapshots table with the exact pre-change DDL and roll the migration
        // ledger back to 3, so this database is indistinguishable from a real V3 database.
        ASSERT_TRUE(store.value()->Exec("DROP TABLE snapshots;").ok());
        ASSERT_TRUE(store
                        .value()
                        ->Exec("CREATE TABLE snapshots("
                               "snapshot_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, "
                               "epoch INTEGER NOT NULL, kind TEXT NOT NULL, "
                               "manifest_digest TEXT NOT NULL DEFAULT '', digest TEXT NOT NULL, "
                               "entry_count INTEGER NOT NULL DEFAULT 0, "
                               "excluded_count INTEGER NOT NULL DEFAULT 0, "
                               "truncated INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);")
                        .ok());
        ASSERT_TRUE(store.value()->Exec("DELETE FROM schema_migrations WHERE version = 4;").ok());
        EXPECT_FALSE(TableHasColumn(store.value().get(), "snapshots", "digest_profile"))
            << "fixture must start as a genuine pre-change schema";

        InsertLegacySnapshot(store.value().get(), "legacy-1", legacy_digest.value(), entries);
    }

    // Phase 2: reopen. The V4 migration must add digest_profile (default 1) and the legacy row
    // must load without rewriting its history.
    {
        auto store = storage::Store::Open(db_path);
        ASSERT_TRUE(store.ok()) << store.message();

        EXPECT_TRUE(TableHasColumn(store.value().get(), "snapshots", "digest_profile"))
            << "V4 migration must add digest_profile";

        auto version = store.value()->Query("SELECT MAX(version) AS v FROM schema_migrations;");
        ASSERT_TRUE(version.ok());
        ASSERT_FALSE(version.value().empty());
        EXPECT_EQ(version.value()[0].Int("v"), 4);
        EXPECT_EQ(store.value()->SchemaVersion(), 4);

        auto row = store.value()->Query(
            "SELECT digest_profile FROM snapshots WHERE snapshot_id = 'legacy-1';");
        ASSERT_TRUE(row.ok());
        ASSERT_FALSE(row.value().empty());
        EXPECT_EQ(row.value()[0].Int("digest_profile"), 1);

        Synchronizer sync(store.value().get(), scratch.path());
        auto loaded = sync.LoadSnapshot("legacy-1");
        ASSERT_TRUE(loaded.ok()) << loaded.message();
        EXPECT_EQ(loaded.value().digest, legacy_digest.value());
        ASSERT_EQ(loaded.value().entries.size(), entries.size());
        EXPECT_EQ(loaded.value().entries[0].rel_path, "secret.env");
        EXPECT_EQ(loaded.value().entries[0].coverage, "EXCLUDED");
        EXPECT_EQ(loaded.value().entries[0].reason, "excluded by rule (.env)");
        EXPECT_EQ(loaded.value().entries[1].rel_path, "src/a.cpp");
    }
}

// A tampered legacy row must fail closed: changing a persisted entry hash after the digest was
// computed is corruption, and the loader must report an integrity failure, not serve it.
TEST(ContextDigestCompat, LegacySnapshotTamperedFailsClosed) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("tampered.db"));
    ASSERT_TRUE(store.ok()) << store.message();
    const auto entries = LegacyEntries();
    auto legacy_digest =
        OriginalShapeDigest("proj-legacy", 7, "REPOSITORY", "manifest-md", entries);
    ASSERT_TRUE(legacy_digest.ok()) << legacy_digest.message();
    InsertLegacySnapshot(store.value().get(), "legacy-2", legacy_digest.value(), entries);

    ASSERT_TRUE(store
                    .value()
                    ->Exec("UPDATE snapshot_entries SET sha256 = ? WHERE snapshot_id = 'legacy-2' "
                           "AND rel_path = 'src/a.cpp';",
                           {SqlValue::Text(std::string(64, 'f'))})
                    .ok());

    Synchronizer sync(store.value().get(), scratch.path());
    auto loaded = sync.LoadSnapshot("legacy-2");
    ASSERT_FALSE(loaded.ok());
    EXPECT_EQ(loaded.code(), ErrorCode::IntegrityFailure);
}

// A fresh snapshot round-trips and records profile 2; its stored digest is exactly the
// profile-2 shape computed independently by this test.
TEST(ContextDigestCompat, V2RoundTripRecordsProfileTwo) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("v2.db"));
    ASSERT_TRUE(store.ok()) << store.message();
    const std::string root = scratch.File("root");
    test::WriteText(fs::JoinPath(root, "src\\a.cpp"), "int a();");

    auto manifest = workspace::BuildManifest("proj-v2", root);
    ASSERT_TRUE(manifest.ok()) << manifest.message();
    Synchronizer sync(store.value().get(), root);

    auto created = sync.CreateSnapshot("proj-v2", 5, manifest.value());
    ASSERT_TRUE(created.ok()) << created.message();

    auto row = store.value()->Query(
        "SELECT digest_profile FROM snapshots WHERE snapshot_id = ?;",
        {SqlValue::Text(created.value().snapshot_id)});
    ASSERT_TRUE(row.ok());
    ASSERT_FALSE(row.value().empty());
    EXPECT_EQ(row.value()[0].Int("digest_profile"), 2);

    auto expected = Profile2ShapeDigest("proj-v2", 5, "REPOSITORY", manifest.value().digest,
                                        created.value().entries);
    ASSERT_TRUE(expected.ok()) << expected.message();
    EXPECT_EQ(created.value().digest, expected.value());

    auto loaded = sync.LoadSnapshot(created.value().snapshot_id);
    ASSERT_TRUE(loaded.ok()) << loaded.message();
    EXPECT_EQ(loaded.value().digest, created.value().digest);
}

// An unknown digest profile must fail closed: the loader never guesses which hash shape
// produced a stored digest.
TEST(ContextDigestCompat, UnknownDigestProfileFailsClosed) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("unknown.db"));
    ASSERT_TRUE(store.ok()) << store.message();
    const auto entries = LegacyEntries();
    auto legacy_digest =
        OriginalShapeDigest("proj-legacy", 7, "REPOSITORY", "manifest-md", entries);
    ASSERT_TRUE(legacy_digest.ok()) << legacy_digest.message();
    InsertLegacySnapshot(store.value().get(), "legacy-3", legacy_digest.value(), entries);
    ASSERT_TRUE(store.value()
                    ->Exec("UPDATE snapshots SET digest_profile = 99 WHERE snapshot_id = "
                           "'legacy-3';")
                    .ok());

    Synchronizer sync(store.value().get(), scratch.path());
    auto loaded = sync.LoadSnapshot("legacy-3");
    ASSERT_FALSE(loaded.ok());
    EXPECT_EQ(loaded.code(), ErrorCode::IntegrityFailure);
    EXPECT_NE(loaded.message().find("profile"), std::string::npos) << loaded.message();
}

// A row marked profile 1 whose stored digest is profile-2 shaped must NOT be accepted: there is
// no permissive cross-profile fallback, so a legacy row can never be silently re-interpreted
// under the new shape (and vice versa).
TEST(ContextDigestCompat, V2ShapedDigestOnLegacyRowFailsClosed) {
    ScratchDir scratch;
    auto store = storage::Store::Open(scratch.File("cross_profile.db"));
    ASSERT_TRUE(store.ok()) << store.message();
    const auto entries = LegacyEntries();
    auto v2_digest = Profile2ShapeDigest("proj-legacy", 7, "REPOSITORY", "manifest-md", entries);
    ASSERT_TRUE(v2_digest.ok()) << v2_digest.message();

    // Stored digest is profile-2 shaped but the row says profile 1 (the column default).
    InsertLegacySnapshot(store.value().get(), "legacy-4", v2_digest.value(), entries);

    Synchronizer sync(store.value().get(), scratch.path());
    auto loaded = sync.LoadSnapshot("legacy-4");
    ASSERT_FALSE(loaded.ok());
    EXPECT_EQ(loaded.code(), ErrorCode::IntegrityFailure);
}

