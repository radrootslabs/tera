use super::{coordinate_support, retraction_support::*, *};
use radroots_event::{
    SignedEvent,
    admission::{RawEvent, SignatureVerifier},
};
use radroots_storage::event::{EventAdmission, EventStore};
use radroots_transport::{
    TransportId,
    source::{EventProvenance, ObservedEvent},
    target::Target,
};

struct Permissive;
impl SignatureVerifier for Permissive {
    fn verify_signature(
        &self,
        _: &radroots_event::envelope::EventEnvelope,
    ) -> Result<(), radroots_event::admission::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn source_authority_reverifies_stored_signature_and_actual_product_family() {
    let runtime = signing_runtime();
    let command = Phase1AddCommand::CreateUpdate(CreateUpdate::new("Original harvest").unwrap());
    let plan = command.authored_plan(1_700_000_000, AUTHOR).unwrap();
    let mut wire = signed_plan(&plan, SECRET).wire().clone();
    wire.sig = "0".repeat(128);
    let event =
        SignedEvent::from_wire_verified_id(wire.clone(), serde_json::to_string(&wire).unwrap())
            .unwrap();
    let verified = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Permissive)
        .unwrap();
    let relay = Target::new(TransportId::NOSTR, "wss://relay.example").unwrap();
    let observed = ObservedEvent::new(
        event,
        EventProvenance::new(
            TransportId::NOSTR,
            relay.fingerprint().clone(),
            1_700_000_000_000,
        )
        .unwrap(),
    );
    EventStore::admit(
        runtime.client.storage().unwrap(),
        EventAdmission::verified(observed, verified).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        runtime
            .require_revision_source(&original_target(&command))
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );

    // The native family and derived card may agree while the signed source is a different family.
    let valid = Phase1AddCommand::CreateUpdate(CreateUpdate::new("Another original").unwrap());
    let target = retain_original(&runtime, &valid).await;
    let source =
        CardSourceIdentity::Event(radroots_event::EventId::parse(&target.source_event_id).unwrap());
    let forged = Phase1RevisionTarget::new(
        AddCommandType::CreateAsk,
        CardId::derive(TodayCardType::Ask, &source),
        target.source_event_id,
        1,
        None,
        AUTHOR,
    )
    .unwrap();
    assert_eq!(
        runtime.require_revision_source(&forged).await.unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
}

#[tokio::test]
async fn address_source_requires_exact_signed_kind_and_coordinate_even_when_metadata_is_consistent()
{
    let runtime = signing_runtime();
    let (command, _) = coordinate_support::calendar("original", "Market");
    let plan = command.authored_plan(1_700_000_000, AUTHOR).unwrap();
    let event = signed_plan(&plan, SECRET);
    coordinate_support::retain(&runtime, event.clone()).await;
    for (kind, identifier, accepted) in [
        (31_923, "original", true),
        (31_923, "forged", false),
        (31_922, "original", false),
    ] {
        let source = CardSourceIdentity::address(kind, AUTHOR, identifier).unwrap();
        let target = Phase1RevisionTarget::new(
            AddCommandType::CreateEvent,
            CardId::derive(TodayCardType::Event, &source),
            event.id().to_hex(),
            kind,
            Some(format!("{kind}:{AUTHOR}:{identifier}")),
            AUTHOR,
        )
        .unwrap();
        assert_eq!(
            runtime.require_revision_source(&target).await.is_ok(),
            accepted
        );
    }
    assert!(runtime.phase1_draft_heads(20).await.unwrap().is_empty());
}
