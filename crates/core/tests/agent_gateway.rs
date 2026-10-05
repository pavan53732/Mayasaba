use mayasaba_core::agent_gateway::queue_agent_handshake;
use mayasaba_agents::{AgentType, HandshakeEnvelopeInput, Transport};
use mayasaba_bus::Bus;
use mayasaba_storage::{NewProject, Storage};

fn bus() -> Bus {
    let mut storage = Storage::open_in_memory().expect("storage");
    storage.create_project(&NewProject {
        project_id:"44444444-4444-4444-8444-444444444444".into(),
        local_path:r"C:\work\gateway".into(),
        brief_id:"brf_gateway".into(),
        brief_body:"gateway".into(),
        brief_source:"TEST".into(),
        event_id:"55555555-5555-4555-8555-555555555555".into(),
        created_at:"2026-10-05T17:30:00Z".into(),
    }).expect("project");
    Bus::new(storage)
}

#[test]
fn handshake_is_enqueued_durably_but_not_dispatched() {
    let mut bus = bus();
    let input = HandshakeEnvelopeInput {
        message_id:"11111111-1111-4111-8111-111111111111".into(),
        event_id:"22222222-2222-4222-8222-222222222222".into(),
        correlation_id:"33333333-3333-4333-8333-333333333333".into(),
        project_id:"44444444-4444-4444-8444-444444444444".into(),
        session_id:"66666666-6666-4666-8666-666666666666".into(),
        agent_id:"77777777-7777-4777-8777-777777777777".into(),
        agent_type:AgentType::Hermes,
        adapter_version:"0.1.0".into(),
        protocol_versions:vec!["MCF-2".into()],
        capabilities:vec!["version_probe".into(),"structured_transport".into()],
        native_transport:Transport::HermesStreamJson,
        workspace_id:None,
        project_epoch:0,
        created_at:"2026-10-05T17:30:01Z".into(),
    };

    let result = queue_agent_handshake(&mut bus,&input).expect("queue handshake");
    assert!(!result.message_id.is_empty());
    assert!(!result.outbox_id.is_empty());
    assert!(!result.deduplicated);

    let row: (String,String,String) = bus.storage().conn().query_row(
        "SELECT delivery_state, message_type, json_extract(envelope_json,'$.channel')
         FROM messages WHERE message_id=?1",
        [&result.message_id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).expect("message");
    assert_eq!(row.0,"QUEUED");
    assert_eq!(row.1,"HANDSHAKE");
    assert_eq!(row.2,"agent");
    assert_eq!(bus.backlog().expect("backlog"),1);
}
