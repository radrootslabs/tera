use super::*;
use nostr::secp256k1::{Keypair, Message, SECP256K1};
use radroots_event::{SignedEvent, wire::v1::Nip01EventWire};

pub(super) fn signed_plan(plan: &AuthoredEventPlan, secret: &str) -> SignedEvent {
    let keys = nostr::Keys::parse(secret).unwrap();
    let sig = SECP256K1.sign_schnorr_no_aux_rand(
        &Message::from_digest(*plan.expected_event_id().as_bytes()),
        &Keypair::from_secret_key(SECP256K1, keys.secret_key()),
    );
    let wire = Nip01EventWire {
        id: plan.expected_event_id().to_hex(),
        pubkey: plan.author().to_hex(),
        created_at: plan.created_at(),
        kind: plan.body().kind(),
        tags: plan.body().tags().to_vec(),
        content: plan.body().content().into(),
        sig: sig.to_string(),
        extra: Default::default(),
    };
    SignedEvent::from_wire_verified_id(wire.clone(), serde_json::to_string(&wire).unwrap()).unwrap()
}

pub(super) async fn original(runtime: &TeraRuntime) -> Phase1RevisionTarget {
    let command = Phase1AddCommand::CreateUpdate(CreateUpdate::new("Original harvest").unwrap());
    retain_original(runtime, &command).await
}

pub(super) fn original_target(command: &Phase1AddCommand) -> Phase1RevisionTarget {
    let plan = command.authored_plan(1_700_000_000, AUTHOR).unwrap();
    let kind = command.command_type();
    Phase1RevisionTarget::from_source(
        kind,
        card_id(kind, &plan).unwrap(),
        plan.expected_event_id().to_hex(),
        None,
        AUTHOR,
    )
    .unwrap()
}

pub(super) async fn retain_original(
    runtime: &TeraRuntime,
    command: &Phase1AddCommand,
) -> Phase1RevisionTarget {
    let plan = command.authored_plan(1_700_000_000, AUTHOR).unwrap();
    let event = signed_plan(&plan, SECRET);
    let target = original_target(command);
    super::coordinate_support::retain(runtime, event).await;
    target
}
