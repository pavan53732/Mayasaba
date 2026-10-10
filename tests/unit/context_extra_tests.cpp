// H4 extra coverage: Context Synchronizer (layer 6).
//
// Independent behavioral + negative coverage for: snapshot immutability and digest provenance,
// coverage-provenance validation, staleness detection, secret filtering, bounded content
// delivery and the reverse-dependency invalidation closure. Each test uses a real SQLite store
// on a scratch directory (never the user's project data).
#include <gtest/gtest.h>

#include <algorithm>
#include <memory>
#include <string>
#include <vector>

#include "mayasaba/base.hpp"
#include "mayasaba/context.hpp"
#include "mayasaba/fs.hpp"
#include "mayasaba/store.hpp"
#include "mayasaba/workspace.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::context;
using mayasaba::test::ScratchDir;
using mayasaba::test::WriteText;

namespace {

bool Contains(const std::vector<std::string>& values, const std::string& needle) {
    return std::find(values.begin(), values.end(), needle) != values.end();
}

workspace::ManifestEntry* FindEntry(workspace::Manifest& manifest, const std::string& rel_path) {
    for (auto& entry : manifest.entries) {
        if (entry.rel_path == rel_path) return &entry;
    }
    return nullptr;
}

const SnapshotEntry* FindSnapshotEntry(const Snapshot& snapshot, const std::string& rel_path) {
    for (const auto& entry : snapshot.entries) {
        if (entry.rel_path == rel_path) return &entry;
    }
    return nullptr;
}

struct Ctx {
    ScratchDir scratch;
    std::unique_ptr<storage::Store> store;
    std::string root;

    Ctx() {
        auto opened = storage::Store::Open(scratch.File("ctx_extra.db"));
        EXPECT_TRUE(opened.ok()) << opened.message();
        store = std::move(opened.value());
        root = scratch.File("root");
    }
};

}  // namespace

// --- Snapshot immutability + digest provenance ---------------------------------------------

TEST(ContextExtra, SnapshotIsWriteOnceAndDigestIsStable) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "a.txt"), "alpha");
    WriteText(fs::JoinPath(ctx.root, "b.txt"), "beta");
    auto manifest = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest.ok()) << manifest.message();
    Synchronizer sync(ctx.store.get(), ctx.root);

    auto first = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(first.ok()) << first.message();
    auto second = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(second.ok()) << second.message();

    // Distinct identities, identical content digest (digest is over the persisted entry set).
    EXPECT_NE(first.value().snapshot_id, second.value().snapshot_id);
    EXPECT_EQ(first.value().digest, second.value().digest);

    // A later create must not have rewritten the earlier snapshot.
    auto reloaded = sync.LoadSnapshot(first.value().snapshot_id);
    ASSERT_TRUE(reloaded.ok()) << reloaded.message();
    EXPECT_EQ(reloaded.value().digest, first.value().digest);
    EXPECT_EQ(reloaded.value().entries.size(), first.value().entries.size());

    // Write-once guard: re-inserting the same snapshot id at the storage boundary is rejected,
    // which proves CreateSnapshot used a plain INSERT, never INSERT OR REPLACE.
    auto dup = ctx.store->Exec(
        "INSERT INTO snapshots(snapshot_id, project_id, epoch, kind, manifest_digest, digest, "
        "entry_count, excluded_count, truncated, created_at) VALUES(?,?,?,?,?,?,?,?,?,?);",
        {storage::SqlValue::Text(first.value().snapshot_id), storage::SqlValue::Text("proj"),
         storage::SqlValue::Int(0), storage::SqlValue::Text("REPOSITORY"),
         storage::SqlValue::Text(first.value().manifest_digest),
         storage::SqlValue::Text(first.value().digest), storage::SqlValue::Int(0),
         storage::SqlValue::Int(0), storage::SqlValue::Int(0),
         storage::SqlValue::Text("2026-01-01T00:00:00.000Z")});
    EXPECT_FALSE(dup.ok()) << "an existing snapshot id must never be rewritten";
}

TEST(ContextExtra, LoadSnapshotFailsClosedOnTamperedDigest) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "a.txt"), "alpha");
    auto manifest = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);
    auto snap = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snap.ok());

    ASSERT_TRUE(ctx.store
                    ->Exec("UPDATE snapshots SET digest='deadbeef' WHERE snapshot_id=?;",
                           {storage::SqlValue::Text(snap.value().snapshot_id)})
                    .ok());
    auto reloaded = sync.LoadSnapshot(snap.value().snapshot_id);
    EXPECT_FALSE(reloaded.ok());
    EXPECT_EQ(reloaded.code(), ErrorCode::IntegrityFailure);
}

// --- Coverage provenance -------------------------------------------------------------------

TEST(ContextExtra, CoverageStateContract) {
    for (const char* state : {"INVENTORIED", "CONTENT_INSPECTED", "ANALYZED", "EXCLUDED",
                              "UNREADABLE", "UNSUPPORTED", "TOO_LARGE", "STALE"}) {
        EXPECT_TRUE(IsValidCoverage(state)) << state;
    }
    EXPECT_FALSE(IsValidCoverage("VIBES"));
    EXPECT_FALSE(IsValidCoverage(""));
    EXPECT_FALSE(IsValidCoverage("content_inspected"));  // case-sensitive registry values
    EXPECT_TRUE(CoverageRequiresReadEvidence("CONTENT_INSPECTED"));
    EXPECT_TRUE(CoverageRequiresReadEvidence("ANALYZED"));
    EXPECT_FALSE(CoverageRequiresReadEvidence("INVENTORIED"));
    EXPECT_FALSE(CoverageRequiresReadEvidence("EXCLUDED"));
}

TEST(ContextExtra, SnapshotRejectsInvalidOrUnsupportedCoverage) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "a.txt"), "alpha");
    auto base = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(base.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);

    {  // unknown coverage state
        auto m = base.value();
        auto* entry = FindEntry(m, "a.txt");
        ASSERT_NE(entry, nullptr);
        entry->coverage = "VIBES";
        auto r = sync.CreateSnapshot("proj", 0, m);
        EXPECT_EQ(r.code(), ErrorCode::InvalidArgument);
        EXPECT_NE(r.message().find("coverage"), std::string::npos);
    }
    {  // CONTENT_INSPECTED with no read evidence (empty sha256)
        auto m = base.value();
        auto* entry = FindEntry(m, "a.txt");
        ASSERT_NE(entry, nullptr);
        entry->coverage = "CONTENT_INSPECTED";
        entry->sha256.clear();
        auto r = sync.CreateSnapshot("proj", 0, m);
        EXPECT_EQ(r.code(), ErrorCode::InvalidArgument);
        EXPECT_NE(r.message().find("read evidence"), std::string::npos);
    }
    {  // ANALYZED with no read evidence
        auto m = base.value();
        auto* entry = FindEntry(m, "a.txt");
        ASSERT_NE(entry, nullptr);
        entry->coverage = "ANALYZED";
        entry->sha256.clear();
        EXPECT_EQ(sync.CreateSnapshot("proj", 0, m).code(), ErrorCode::InvalidArgument);
    }
    {  // negative state without a reason
        auto m = base.value();
        auto* entry = FindEntry(m, "a.txt");
        ASSERT_NE(entry, nullptr);
        entry->coverage = "EXCLUDED";
        entry->reason.clear();
        auto r = sync.CreateSnapshot("proj", 0, m);
        EXPECT_EQ(r.code(), ErrorCode::InvalidArgument);
        EXPECT_NE(r.message().find("reason"), std::string::npos);
    }
    {  // positive: CONTENT_INSPECTED with real read evidence is accepted and round-trips
        auto m = base.value();
        auto* entry = FindEntry(m, "a.txt");
        ASSERT_NE(entry, nullptr);
        entry->coverage = "CONTENT_INSPECTED";  // keeps its real sha256
        auto r = sync.CreateSnapshot("proj", 0, m);
        ASSERT_TRUE(r.ok()) << r.message();
        auto loaded = sync.LoadSnapshot(r.value().snapshot_id);
        ASSERT_TRUE(loaded.ok()) << loaded.message();
        const SnapshotEntry* loaded_entry = FindSnapshotEntry(loaded.value(), "a.txt");
        ASSERT_NE(loaded_entry, nullptr);
        EXPECT_EQ(loaded_entry->coverage, "CONTENT_INSPECTED");
        EXPECT_FALSE(loaded_entry->sha256.empty());
    }
}

TEST(ContextExtra, ExcludedOrUnreadableEntriesAreNeverDelivered) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "secret.txt"), "content that is out of scope");
    auto base = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(base.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);

    auto m = base.value();
    auto* entry = FindEntry(m, "secret.txt");
    ASSERT_NE(entry, nullptr);
    entry->coverage = "EXCLUDED";
    entry->reason = "user excluded this path";
    entry->sha256.clear();
    auto snap = sync.CreateSnapshot("proj", 0, m);
    ASSERT_TRUE(snap.ok()) << snap.message();

    auto ref = sync.ReadContent(snap.value(), "secret.txt");
    ASSERT_TRUE(ref.ok());
    EXPECT_TRUE(ref.value().content.empty());
    EXPECT_NE(ref.value().unavailable_reason.find("coverage=EXCLUDED"), std::string::npos);
}

// --- Staleness -----------------------------------------------------------------------------

TEST(ContextExtra, StaleEntriesIsReadOnlyAndReturnsRelativePaths) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "keep.txt"), "keep");
    WriteText(fs::JoinPath(ctx.root, "gone.txt"), "gone");
    WriteText(fs::JoinPath(ctx.root, "edit.txt"), "before");
    auto manifest = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);
    auto snap = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snap.ok());

    WriteText(fs::JoinPath(ctx.root, "edit.txt"), "after-after");
    ASSERT_TRUE(fs::RemoveFile(fs::JoinPath(ctx.root, "gone.txt")).ok());

    const std::size_t before_entries = snap.value().entries.size();
    auto stale = sync.StaleEntries(snap.value());
    ASSERT_TRUE(stale.ok());
    EXPECT_TRUE(Contains(stale.value(), "edit.txt"));   // modified, by hash
    EXPECT_TRUE(Contains(stale.value(), "gone.txt"));   // deleted, by identity
    EXPECT_FALSE(Contains(stale.value(), "keep.txt"));
    // Mutates nothing: neither the in-memory snapshot nor the persisted rows change.
    EXPECT_EQ(snap.value().entries.size(), before_entries);
    auto reloaded = sync.LoadSnapshot(snap.value().snapshot_id);
    ASSERT_TRUE(reloaded.ok());
    EXPECT_EQ(reloaded.value().entries.size(), before_entries);
    EXPECT_EQ(reloaded.value().digest, snap.value().digest);
}

TEST(ContextExtra, ReadTaskContextFlagsChangedEntryInsteadOfServingStaleBytes) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "src\\a.txt"), "original-content");
    auto manifest = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);
    auto snap = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snap.ok());

    WriteText(fs::JoinPath(ctx.root, "src\\a.txt"), "tampered-different-bytes");
    auto refs = sync.ReadTaskContext(snap.value(), {"src"}, 1u << 20);
    ASSERT_TRUE(refs.ok()) << refs.message();
    ASSERT_EQ(refs.value().size(), 1u);
    EXPECT_TRUE(refs.value()[0].content.empty());
    EXPECT_NE(refs.value()[0].unavailable_reason.find("changed since snapshot"),
              std::string::npos);
}

// --- Secret filtering ----------------------------------------------------------------------

TEST(ContextExtra, SecretShapedContentIsWithheldWithSecretFilteredReason) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "key.pem"),
              "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----\n");
    WriteText(fs::JoinPath(ctx.root, "auth.txt"),
              "Authorization: Bearer abcdef0123456789ABCDEF\n");
    WriteText(fs::JoinPath(ctx.root, "db.env"),
              "DATABASE_URL=postgres://appuser:s3cretpass@localhost:5432/appdb\n");
    WriteText(fs::JoinPath(ctx.root, "conn.txt"),
              "Server=db;User Id=admin;Password=hunter2xyz;\n");
    WriteText(fs::JoinPath(ctx.root, "plain.txt"), "int add(int a, int b) { return a + b; }\n");

    auto manifest = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);
    auto snap = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snap.ok());

    for (const char* path : {"key.pem", "auth.txt", "db.env", "conn.txt"}) {
        auto ref = sync.ReadContent(snap.value(), path);
        ASSERT_TRUE(ref.ok()) << path;
        EXPECT_TRUE(ref.value().content.empty()) << path;
        EXPECT_EQ(ref.value().unavailable_reason.rfind("secret-filtered", 0), 0u) << path;
    }

    // Negative case: ordinary code is delivered unchanged.
    auto plain = sync.ReadContent(snap.value(), "plain.txt");
    ASSERT_TRUE(plain.ok());
    EXPECT_TRUE(plain.value().unavailable_reason.empty());
    EXPECT_FALSE(plain.value().content.empty());
}

TEST(ContextExtra, WithholdDetectsBearerAndConnectionStringsDirectly) {
    std::string reason;
    EXPECT_TRUE(ShouldWithholdContent("Authorization: Bearer 0123456789abcdef", &reason));
    EXPECT_TRUE(ShouldWithholdContent("postgres://u:p@host:5432/db", &reason));
    EXPECT_TRUE(ShouldWithholdContent("-----BEGIN PRIVATE KEY-----", &reason));
    EXPECT_TRUE(ShouldWithholdContent("client_secret: \"abcdef123456\"", &reason));
    EXPECT_FALSE(ShouldWithholdContent("bearer token authentication is documented here", &reason));
    EXPECT_FALSE(ShouldWithholdContent("int main() { return 0; }", &reason));
}

// --- Reverse-dependency invalidation closure -----------------------------------------------

TEST(ContextExtra, InvalidationClosureIncludesTransitiveDependents) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "a.h"), "v1");
    WriteText(fs::JoinPath(ctx.root, "mid.h"), "#include \"a.h\"\n");
    WriteText(fs::JoinPath(ctx.root, "lib.cpp"), "#include \"mid.h\"\n");
    WriteText(fs::JoinPath(ctx.root, "other.h"), "v1");
    WriteText(fs::JoinPath(ctx.root, "unrelated.cpp"), "#include \"other.h\"\n");

    auto manifest1 = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest1.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);
    auto snap = sync.CreateSnapshot("proj", 0, manifest1.value());
    ASSERT_TRUE(snap.ok());

    WriteText(fs::JoinPath(ctx.root, "a.h"), "v2-changed");
    auto manifest2 = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest2.ok());

    auto invalidated = sync.InvalidatedPaths(snap.value(), manifest2.value());
    ASSERT_TRUE(invalidated.ok()) << invalidated.message();
    EXPECT_TRUE(Contains(invalidated.value(), "a.h"));      // the changed file itself
    EXPECT_TRUE(Contains(invalidated.value(), "mid.h"));    // direct dependent
    EXPECT_TRUE(Contains(invalidated.value(), "lib.cpp"));  // transitive dependent
    EXPECT_FALSE(Contains(invalidated.value(), "unrelated.cpp"));
    EXPECT_FALSE(Contains(invalidated.value(), "other.h"));

    // Without reverse dependencies, only the changed path is returned (closure is real, not
    // just the changed set).
    auto changed_only = sync.InvalidatedPaths(snap.value(), manifest2.value(), false);
    ASSERT_TRUE(changed_only.ok());
    EXPECT_TRUE(Contains(changed_only.value(), "a.h"));
    EXPECT_FALSE(Contains(changed_only.value(), "mid.h"));
    EXPECT_FALSE(Contains(changed_only.value(), "lib.cpp"));
}

// --- Bounded content delivery --------------------------------------------------------------

TEST(ContextExtra, ReadContentEnforcesBoundsAndReportsBinaryWithoutThrowing) {
    Ctx ctx;
    WriteText(fs::JoinPath(ctx.root, "big.txt"), std::string(100, 'x'));
    const std::vector<std::uint8_t> binary{'a', 0, 'b', 'c'};
    ASSERT_TRUE(fs::WriteFileBytes(fs::JoinPath(ctx.root, "bin.dat"), binary).ok());

    auto manifest = workspace::BuildManifest("proj", ctx.root);
    ASSERT_TRUE(manifest.ok());
    Synchronizer sync(ctx.store.get(), ctx.root);
    auto snap = sync.CreateSnapshot("proj", 0, manifest.value());
    ASSERT_TRUE(snap.ok());

    auto bounded = sync.ReadContent(snap.value(), "big.txt", 10);
    ASSERT_TRUE(bounded.ok());
    EXPECT_TRUE(bounded.value().truncated);
    EXPECT_EQ(bounded.value().content.size(), 10u);

    auto bin = sync.ReadContent(snap.value(), "bin.dat");
    ASSERT_TRUE(bin.ok());
    EXPECT_TRUE(bin.value().content.empty());
    EXPECT_FALSE(bin.value().unavailable_reason.empty());
}

