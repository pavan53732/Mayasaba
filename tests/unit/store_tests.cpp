// Storage layer tests: migrations, event chain integrity, idempotency, backup, blobs.
#include <gtest/gtest.h>

#include <thread>

#include "mayasaba/blobs.hpp"
#include "mayasaba/store.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::storage;
using mayasaba::test::ScratchDir;

TEST(Store, OpensAppliesMigrationsAndRecordsJournalMode) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok()) << store.message();
    EXPECT_EQ(store.value()->SchemaVersion(), kSchemaVersion);
    // The actual journal mode is read back, never assumed.
    EXPECT_FALSE(store.value()->JournalMode().empty());
    auto check = store.value()->QuickCheck();
    ASSERT_TRUE(check.ok()) << check.message();
    EXPECT_EQ(check.value(), "ok");
}

TEST(Store, ReopenIsStable) {
    ScratchDir scratch;
    std::string path = scratch.File("mayasaba.db");
    {
        auto store = Store::Open(path);
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store.value()->SetSetting("ui.theme", "light").ok());
    }
    auto reopened = Store::Open(path);
    ASSERT_TRUE(reopened.ok());
    auto setting = reopened.value()->GetSetting("ui.theme");
    ASSERT_TRUE(setting.has_value());
    EXPECT_EQ(*setting, "light");
}

TEST(Store, EventChainAppendAndVerify) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();

    auto first = db.AppendEvent("proj_a", "ProjectCreated", {{"root", "C:/x"}});
    ASSERT_TRUE(first.ok()) << first.message();
    EXPECT_TRUE(first.value().prev_hash.empty());

    auto second = db.AppendEvent("proj_a", "ContributionPersisted", {{"text", "hello"}});
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(second.value().prev_hash, first.value().hash);

    // A different project has its own chain.
    auto other = db.AppendEvent("proj_b", "ProjectCreated", {{"root", "C:/y"}});
    ASSERT_TRUE(other.ok());
    EXPECT_TRUE(other.value().prev_hash.empty());

    EXPECT_TRUE(db.VerifyChain("proj_a").ok());
    EXPECT_TRUE(db.VerifyChain("proj_b").ok());
    EXPECT_EQ(db.EventCount("proj_a"), 2);
}

TEST(Store, EventChainDetectsTampering) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();
    ASSERT_TRUE(db.AppendEvent("proj", "A", {{"v", 1}}).ok());
    ASSERT_TRUE(db.AppendEvent("proj", "B", {{"v", 2}}).ok());
    ASSERT_TRUE(db.AppendEvent("proj", "C", {{"v", 3}}).ok());

    // Corrupt the payload of the middle event.
    ASSERT_TRUE(db.Exec("UPDATE events SET payload = '{\"v\":999}' WHERE seq = 2;").ok());
    std::int64_t bad_seq = 0;
    auto status = db.VerifyChain("proj", &bad_seq);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.code(), ErrorCode::IntegrityFailure);
    EXPECT_EQ(bad_seq, 2);
}

TEST(Store, EventChainDetectsDeletion) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();
    ASSERT_TRUE(db.AppendEvent("proj", "A", {{"v", 1}}).ok());
    ASSERT_TRUE(db.AppendEvent("proj", "B", {{"v", 2}}).ok());
    ASSERT_TRUE(db.Exec("DELETE FROM events WHERE seq = 1;").ok());
    auto status = db.VerifyChain("proj");
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.code(), ErrorCode::IntegrityFailure);
}

TEST(Store, IdempotencyLedger) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();
    EXPECT_FALSE(db.FindIdempotentResult("proj", "op-1").has_value());
    ASSERT_TRUE(db.RecordIdempotentResult("proj", "op-1", {{"status", "AUTHORIZED"}}).ok());
    auto found = db.FindIdempotentResult("proj", "op-1");
    ASSERT_TRUE(found.has_value());
    EXPECT_EQ((*found)["status"], "AUTHORIZED");
    EXPECT_FALSE(db.FindIdempotentResult("other", "op-1").has_value());
}

TEST(Store, TransactionsRollBackAtomically) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();
    {
        auto transaction = db.Begin();
        ASSERT_TRUE(transaction.ok());
        ASSERT_TRUE(db.Exec("INSERT INTO settings(key, value) VALUES('k', 'v');").ok());
        transaction.value().Rollback();
    }
    EXPECT_FALSE(db.GetSetting("k").has_value());
    {
        auto transaction = db.Begin();
        ASSERT_TRUE(transaction.ok());
        ASSERT_TRUE(db.Exec("INSERT INTO settings(key, value) VALUES('k2', 'v2');").ok());
        ASSERT_TRUE(transaction.value().Commit().ok());
    }
    EXPECT_TRUE(db.GetSetting("k2").has_value());
}

TEST(Store, EventAppendJoinsCallerTransaction) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();
    {
        auto transaction = db.Begin();
        ASSERT_TRUE(transaction.ok());
        ASSERT_TRUE(db.AppendEvent("proj", "A", {{"v", 1}}).ok());
        transaction.value().Rollback();
    }
    EXPECT_EQ(db.EventCount("proj"), 0);
}

TEST(Store, OnlineBackupProducesConsistentCopy) {
    ScratchDir scratch;
    std::string path = scratch.File("mayasaba.db");
    auto store = Store::Open(path);
    ASSERT_TRUE(store.ok());
    ASSERT_TRUE(store.value()->AppendEvent("proj", "A", {{"v", 1}}).ok());
    std::string backup_path = scratch.File("backup.db");
    ASSERT_TRUE(store.value()->BackupTo(backup_path).ok());
    auto backup = Store::Open(backup_path);
    ASSERT_TRUE(backup.ok());
    EXPECT_EQ(backup.value()->EventCount("proj"), 1);
    EXPECT_TRUE(backup.value()->VerifyChain("proj").ok());
}

TEST(BlobStore, RoundTripAndIntegrityFailure) {
    ScratchDir scratch;
    auto blobs = BlobStore::Open(scratch.File("blobs"));
    ASSERT_TRUE(blobs.ok()) << blobs.message();
    auto digest = blobs.value().PutText("mayasaba evidence bytes");
    ASSERT_TRUE(digest.ok()) << digest.message();
    EXPECT_TRUE(blobs.value().Exists(digest.value()));
    auto bytes = blobs.value().Get(digest.value());
    ASSERT_TRUE(bytes.ok());
    EXPECT_EQ(std::string(bytes.value().begin(), bytes.value().end()), "mayasaba evidence bytes");

    // Corrupt the stored blob: verification must fail closed.
    auto status = fs::WriteFileText(blobs.value().PathFor(digest.value()), "tampered");
    ASSERT_TRUE(status.ok());
    auto corrupted = blobs.value().Get(digest.value());
    ASSERT_FALSE(corrupted.ok());
    EXPECT_EQ(corrupted.code(), ErrorCode::IntegrityFailure);
}

TEST(BlobStore, MissingBlobIsIntegrityFailure) {
    ScratchDir scratch;
    auto blobs = BlobStore::Open(scratch.File("blobs"));
    ASSERT_TRUE(blobs.ok());
    auto missing = blobs.value().Get(std::string(64, 'a'));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.code(), ErrorCode::IntegrityFailure);
}

TEST(Store, SerializedConcurrentWritesStayConsistent) {
    ScratchDir scratch;
    auto store = Store::Open(scratch.File("mayasaba.db"));
    ASSERT_TRUE(store.ok());
    auto& db = *store.value();
    constexpr int kThreads = 4;
    constexpr int kPerThread = 25;
    std::vector<std::thread> threads;
    std::atomic<int> failures{0};
    for (int t = 0; t < kThreads; ++t) {
        threads.emplace_back([&db, t, &failures] {
            for (int i = 0; i < kPerThread; ++i) {
                auto event = db.AppendEvent("proj", "Tick", {{"t", t}, {"i", i}});
                if (!event.ok()) ++failures;
            }
        });
    }
    for (auto& thread : threads) thread.join();
    EXPECT_EQ(failures.load(), 0);
    EXPECT_EQ(db.EventCount("proj"), kThreads * kPerThread);
    EXPECT_TRUE(db.VerifyChain("proj").ok());
}
