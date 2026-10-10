// MCF-v2 bus tests: envelope contract, delivery/ACK, retry, dedup, dead letters, replay,
// ordering gaps, transactional persistence.
#include <gtest/gtest.h>

#include "mayasaba/bus.hpp"
#include "mayasaba/store.hpp"
#include "test_support.hpp"

using namespace mayasaba;
using namespace mayasaba::protocol;
using mayasaba::storage::Store;
using mayasaba::test::ScratchDir;

namespace {

struct Fixture {
    ScratchDir scratch;
    std::unique_ptr<Store> store;
    std::unique_ptr<Bus> bus;
    int handled = 0;
    std::vector<Envelope> received;

    Fixture() {
        auto opened = Store::Open(scratch.File("bus.db"));
        EXPECT_TRUE(opened.ok());
        store = std::move(opened.value());
        bus = std::make_unique<Bus>(store.get());
        bus->RegisterHandler("project.created", [this](DeliveryContext& context) {
            ++handled;
            received.push_back(context.envelope);
            return DeliveryResult::Delivered;
        });
    }

    Envelope MakeEnvelope(const std::string& type = "project.created",
                          const std::string& operation_id = "") {
        Envelope envelope;
        envelope.message_id = NewId("msg");
        envelope.event_id = NewId("evt");
        envelope.project_id = "proj_00000000-0000-0000-0000-000000000001";
        envelope.sender = "controller";
        envelope.channel = "control";
        envelope.message_type = type;
        envelope.operation_id = operation_id;
        envelope.timestamp = NowUtcIso8601();
        envelope.lane = Lane::TaskControl;
        return envelope;
    }
};

}  // namespace

TEST(BusEnvelope, RoundTripAndValidation) {
    Envelope envelope;
    envelope.message_id = NewId("msg");
    envelope.event_id = NewId("evt");
    envelope.project_id = "proj_x";
    envelope.sender = "controller";
    envelope.channel = "council";
    envelope.message_type = "council.position.submitted";
    envelope.lane = Lane::Council;
    envelope.payload = {{"position_id", "pos_1"}};
    auto encoded = EncodeEnvelope(envelope);
    ASSERT_TRUE(encoded.ok()) << encoded.message();
    auto decoded = DecodeEnvelope(encoded.value());
    ASSERT_TRUE(decoded.ok()) << decoded.message();
    EXPECT_EQ(decoded.value().message_type, "council.position.submitted");
    EXPECT_EQ(decoded.value().lane, Lane::Council);
    EXPECT_EQ(decoded.value().payload["position_id"], "pos_1");
}

TEST(BusEnvelope, RejectsUnregisteredTypeAndBadIds) {
    Envelope envelope;
    envelope.message_id = NewId("msg");
    envelope.event_id = NewId("evt");
    envelope.project_id = "proj_x";
    envelope.sender = "controller";
    envelope.channel = "control";
    envelope.message_type = "totally.unregistered.type";
    EXPECT_FALSE(EncodeEnvelope(envelope).ok());

    envelope.message_type = "project.created";
    envelope.message_id = "not-a-uuid";
    EXPECT_FALSE(EncodeEnvelope(envelope).ok());
}

TEST(BusEnvelope, RejectsHostileDecodeInput) {
    EXPECT_FALSE(DecodeEnvelope("not json").ok());
    std::string deep;
    for (int i = 0; i < 500; ++i) deep.push_back('[');
    EXPECT_FALSE(DecodeEnvelope(deep).ok());
    // Valid JSON but wrong version.
    auto wrong = DecodeEnvelope(R"({"protocol_version":99,"message_type":"project.created"})");
    EXPECT_FALSE(wrong.ok());
}

TEST(Bus, PersistBeforeAckAndDeliverOnce) {
    Fixture fixture;
    auto sent = fixture.bus->Send(fixture.MakeEnvelope());
    ASSERT_TRUE(sent.ok()) << sent.message();
    // Persisted before any delivery attempt.
    auto pending = fixture.bus->OutboxFor(fixture.MakeEnvelope().project_id, "PENDING");
    ASSERT_TRUE(pending.ok());
    EXPECT_EQ(pending.value().size(), 1u);
    EXPECT_EQ(fixture.handled, 0);

    EXPECT_EQ(fixture.bus->PumpOnce(), 1u);
    EXPECT_EQ(fixture.handled, 1);
    EXPECT_EQ(fixture.bus->counters().delivered.load(), 1u);
    auto delivered = fixture.bus->OutboxFor("proj_00000000-0000-0000-0000-000000000001",
                                            "DELIVERED");
    ASSERT_TRUE(delivered.ok());
    EXPECT_EQ(delivered.value().size(), 1u);
}

TEST(Bus, MaterialDeduplicationSuppressesRepeatSideEffects) {
    Fixture fixture;
    auto first = fixture.bus->Send(fixture.MakeEnvelope("project.created", "op-material-1"));
    ASSERT_TRUE(first.ok());
    fixture.bus->PumpOnce();
    EXPECT_EQ(fixture.handled, 1);

    // The service records the idempotent result of the material operation.
    ASSERT_TRUE(fixture.store
                    ->RecordIdempotentResult("proj_00000000-0000-0000-0000-000000000001",
                                             "mcf:op-material-1", {{"ok", true}})
                    .ok());
    auto duplicate = fixture.bus->Send(fixture.MakeEnvelope("project.created", "op-material-1"));
    ASSERT_TRUE(duplicate.ok());
    fixture.bus->PumpOnce();
    EXPECT_EQ(fixture.handled, 1);  // still exactly one side effect
    EXPECT_EQ(fixture.bus->counters().deduplicated.load(), 1u);
}

TEST(Bus, RetryThenDeadLetterAfterBudget) {
    Fixture fixture;
    int attempts = 0;
    fixture.bus->RegisterHandler("task.assignment", [&attempts](DeliveryContext&) {
        ++attempts;
        return DeliveryResult::NotYet;
    });
    auto sent = fixture.bus->Send(fixture.MakeEnvelope("task.assignment"));
    ASSERT_TRUE(sent.ok());
    for (int i = 0; i < 12; ++i) {
        fixture.bus->PumpOnce();
        // Skip backoff waits deterministically.
        ASSERT_TRUE(fixture.store
                        ->Exec("UPDATE mcf_outbox SET next_attempt_at=0 WHERE state='PENDING';")
                        .ok());
        if (!fixture.bus->DeadLetters().value().empty()) break;
    }
    auto dead = fixture.bus->DeadLetters();
    ASSERT_TRUE(dead.ok());
    ASSERT_EQ(dead.value().size(), 1u);
    EXPECT_NE(dead.value()[0]["reason"].get<std::string>().find("retry budget"), std::string::npos);
    EXPECT_GE(attempts, 8);
    // A dead letter is recorded in the event chain too.
    auto events = fixture.store->EventsFor("proj_00000000-0000-0000-0000-000000000001");
    ASSERT_TRUE(events.ok());
    bool found = false;
    for (const auto& event : events.value()) {
        if (event.type == "mcf.deadletter") found = true;
    }
    EXPECT_TRUE(found);
}

TEST(Bus, UnhandledTypeIsRejectedExplicitly) {
    Fixture fixture;
    auto sent = fixture.bus->Send(fixture.MakeEnvelope("evidence.recorded"));
    ASSERT_TRUE(sent.ok());
    fixture.bus->PumpOnce();
    auto dead = fixture.bus->DeadLetters();
    ASSERT_TRUE(dead.ok());
    ASSERT_EQ(dead.value().size(), 1u);
    EXPECT_NE(dead.value()[0]["reason"].get<std::string>().find("no handler"),
              std::string::npos);
    EXPECT_EQ(fixture.bus->counters().rejected.load(), 1u);
}

TEST(Bus, OrderingGapsAreReportedNotRepaired) {
    Fixture fixture;
    auto first = fixture.MakeEnvelope();
    first.sequence = 1;
    auto third = fixture.MakeEnvelope();
    third.sequence = 3;
    ASSERT_TRUE(fixture.bus->Send(first).ok());
    ASSERT_TRUE(fixture.bus->Send(third).ok());
    fixture.bus->PumpOnce();
    auto notices = fixture.bus->TakeOrderingGapNotices();
    ASSERT_EQ(notices.size(), 1u);
    EXPECT_NE(notices[0].find("expected 2"), std::string::npos);
}

TEST(Bus, ReplayRequiresFreshAuthorizationForMaterialActions) {
    Fixture fixture;
    auto sent = fixture.bus->Send(fixture.MakeEnvelope("project.created", "op-material-2"));
    ASSERT_TRUE(sent.ok());
    fixture.bus->PumpOnce();
    auto denied = fixture.bus->Replay(sent.value().message_id, false);
    ASSERT_FALSE(denied.ok());
    EXPECT_EQ(denied.code(), ErrorCode::Denied);
    auto allowed = fixture.bus->Replay(sent.value().message_id, true);
    ASSERT_TRUE(allowed.ok()) << allowed.message();
    EXPECT_NE(allowed.value().message_id, sent.value().message_id);
    EXPECT_EQ(allowed.value().correlation_id, sent.value().message_id);
    EXPECT_TRUE(allowed.value().security["replay"].get<bool>());
    fixture.bus->PumpOnce();
    EXPECT_EQ(fixture.handled, 2);  // replayed delivery happened once more
}

TEST(Bus, SendJoinsCallerTransaction) {
    Fixture fixture;
    {
        auto transaction = fixture.store->Begin();
        ASSERT_TRUE(transaction.ok());
        ASSERT_TRUE(fixture.bus->Send(fixture.MakeEnvelope()).ok());
        transaction.value().Rollback();
    }
    auto pending = fixture.bus->OutboxFor("proj_00000000-0000-0000-0000-000000000001",
                                          "PENDING");
    ASSERT_TRUE(pending.ok());
    EXPECT_TRUE(pending.value().empty());
}

TEST(Bus, RegisterHandlerRejectsUnknownTypes) {
    Fixture fixture;
    auto status = fixture.bus->RegisterHandler("not.registered", [](DeliveryContext&) {
        return DeliveryResult::Delivered;
    });
    EXPECT_FALSE(status.ok());
    EXPECT_EQ(status.code(), ErrorCode::SchemaViolation);
}

TEST(Bus, DispatcherThreadDelivers) {
    Fixture fixture;
    fixture.bus->StartDispatcher();
    ASSERT_TRUE(fixture.bus->Send(fixture.MakeEnvelope()).ok());
    for (int i = 0; i < 100 && fixture.handled == 0; ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    fixture.bus->StopDispatcher();
    EXPECT_EQ(fixture.handled, 1);
}

// Regression: the v1 outbox schema had no sender column, so every sequence query failed and
// NextSequence silently reset to 1 for every message. Sequences must increment per
// (project, sender, channel).
TEST(Bus, SequenceIncrementsPerSenderAndChannel) {
    Fixture fixture;
    auto first = fixture.bus->Send(fixture.MakeEnvelope());
    ASSERT_TRUE(first.ok());
    EXPECT_EQ(first.value().sequence, 1);
    auto second = fixture.bus->Send(fixture.MakeEnvelope());
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(second.value().sequence, 2);
    auto third = fixture.bus->Send(fixture.MakeEnvelope());
    ASSERT_TRUE(third.ok());
    EXPECT_EQ(third.value().sequence, 3);

    // A different channel has its own sequence space.
    Envelope other = fixture.MakeEnvelope();
    other.channel = "council";
    auto other_sent = fixture.bus->Send(other);
    ASSERT_TRUE(other_sent.ok());
    EXPECT_EQ(other_sent.value().sequence, 1);
}

// A delivery must never proceed on an unpersisted receipt: when the inbox write cannot be
// observed, the handler is NOT invoked, the failure is counted and surfaced, and the message
// remains PENDING for a later retry. This is the fix for ignored persistence failures before
// ACK (the old code discarded the write result and delivered anyway).
TEST(Bus, DeliveryIsBlockedWhenInboxReceiptCannotPersist) {
    Fixture fixture;
    ASSERT_TRUE(fixture.store->Exec("DROP TABLE mcf_inbox;").ok());
    auto sent = fixture.bus->Send(fixture.MakeEnvelope());
    ASSERT_TRUE(sent.ok());
    std::size_t processed = fixture.bus->PumpOnce();
    EXPECT_EQ(fixture.handled, 0);  // no handler ran on an unpersisted receipt
    EXPECT_EQ(processed, 1u);
    EXPECT_GE(fixture.bus->counters().persistence_failures.load(), 1u);
    auto notices = fixture.bus->TakePersistenceNotices();
    ASSERT_FALSE(notices.empty());
    bool receipt_noted = false;
    for (const auto& notice : notices) {
        if (notice.find("inbox receipt") != std::string::npos) receipt_noted = true;
    }
    EXPECT_TRUE(receipt_noted);
    // The message stays PENDING (not delivered, not dead-lettered).
    auto outbox = fixture.bus->OutboxFor(fixture.MakeEnvelope().project_id);
    ASSERT_TRUE(outbox.ok());
    ASSERT_EQ(outbox.value().size(), 1u);
    EXPECT_EQ(outbox.value()[0]["state"], "PENDING");
}

// An ACK is verified by read-back: if the inbox row cannot be acknowledged, the delivery is
// not reported as delivered and the failure is surfaced instead of silently dropped.
TEST(Bus, AckFailureIsSurfacedAndMessageStaysPending) {
    Fixture fixture;
    auto sent = fixture.bus->Send(fixture.MakeEnvelope());
    ASSERT_TRUE(sent.ok());
    // Remove the receipt row between delivery and ack by dropping the table right after the
    // receipt is written: the pump writes the receipt, runs the handler, then acks; to force
    // the ack to fail we drop the table via a handler that runs mid-pump.
    fixture.bus->RegisterHandler("project.created", [&](DeliveryContext& context) {
        ++fixture.handled;
        (void)context;
        EXPECT_TRUE(fixture.store->Exec("DROP TABLE mcf_inbox;").ok());
        return DeliveryResult::Delivered;
    });
    std::size_t processed = fixture.bus->PumpOnce();
    EXPECT_EQ(processed, 1u);
    EXPECT_EQ(fixture.handled, 1);  // the handler ran exactly once
    EXPECT_GE(fixture.bus->counters().persistence_failures.load(), 1u);
    auto notices = fixture.bus->TakePersistenceNotices();
    ASSERT_FALSE(notices.empty());
    EXPECT_NE(notices[0].find("inbox ack"), std::string::npos);
    // The message is still PENDING: the ack was never recorded, so it will be redelivered
    // (at-least-once; idempotency keys make the duplicate safe).
    auto outbox = fixture.bus->OutboxFor(fixture.MakeEnvelope().project_id);
    ASSERT_TRUE(outbox.ok());
    ASSERT_EQ(outbox.value().size(), 1u);
    EXPECT_EQ(outbox.value()[0]["state"], "PENDING");
}
