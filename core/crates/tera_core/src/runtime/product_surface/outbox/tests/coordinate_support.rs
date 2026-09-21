use super::*;
use nostr::secp256k1::{Keypair, Message, SECP256K1};
use radroots_event::{SignedEvent, admission::RawEvent, wire::v1::Nip01EventWire};
use radroots_storage::event::{EventAdmission, EventStore};
use radroots_transport::{
    TransportId,
    source::{EventProvenance, ObservedEvent},
    target::Target,
};

pub(super) fn signed_head(secret: &str, kind: u32, identifier: &str, at: u64) -> SignedEvent {
    signed_head_content(
        secret,
        kind,
        identifier,
        at,
        "Not a renderable calendar or food record",
    )
}

pub(super) fn signed_head_content(
    secret: &str,
    kind: u32,
    identifier: &str,
    at: u64,
    content: &str,
) -> SignedEvent {
    let keys = nostr::Keys::parse(secret).unwrap();
    let author = keys.public_key().to_string();
    // Deliberately unsupported product content still has protocol head authority.
    let tags = vec![vec!["d".into(), identifier.into()]];
    let id =
        radroots_event::wire::compute_canonical_nip01_event_id(&author, at, kind, &tags, content)
            .unwrap();
    let sig = SECP256K1.sign_schnorr_no_aux_rand(
        &Message::from_digest(*id.as_bytes()),
        &Keypair::from_secret_key(SECP256K1, keys.secret_key()),
    );
    let wire = Nip01EventWire {
        id: id.to_hex(),
        pubkey: author,
        created_at: at,
        kind,
        tags,
        content: content.into(),
        sig: sig.to_string(),
        extra: Default::default(),
    };
    SignedEvent::from_wire_verified_id(wire.clone(), serde_json::to_string(&wire).unwrap()).unwrap()
}

pub(super) async fn retain(runtime: &TeraRuntime, event: SignedEvent) {
    let verified = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&radroots_event_codec::verify::Nip01SignatureVerifier)
        .unwrap();
    let target = Target::new(TransportId::NOSTR, "wss://relay.example").unwrap();
    let provenance = EventProvenance::new(
        TransportId::NOSTR,
        target.fingerprint().clone(),
        1_700_000_000_000,
    )
    .unwrap();
    EventStore::admit(
        runtime.client.storage().unwrap(),
        EventAdmission::verified(ObservedEvent::new(event, provenance), verified).unwrap(),
    )
    .await
    .unwrap();
}

pub(super) fn calendar(
    identifier: &str,
    title: &str,
) -> (Phase1AddCommand, Phase1DraftFormSnapshot) {
    let event = AuthoredCalendarTimeEvent::new(identifier, title, 1_900_003_600).unwrap();
    let form = Phase1DraftFormSnapshot {
        command_type: AddCommandType::CreateEvent,
        content: String::new(),
        identifier: Some(identifier.into()),
        title: Some(title.into()),
        event_timing: Some(Phase1DraftEventTiming::Timed),
        event_start_unix_s: Some(1_900_003_600),
        ..update_form()
    };
    (
        Phase1AddCommand::CreateEvent(CreateEvent::time(event)),
        form,
    )
}

pub(super) fn intent(prior: &SignedEvent, title: &str) -> Phase1ReviseIntent {
    let identifier = &prior.envelope().tags().as_slice()[0].as_slice()[1];
    let (command, form) = calendar(identifier, title);
    let source = CardSourceIdentity::address(31_923, AUTHOR, identifier).unwrap();
    let target = Phase1RevisionTarget::from_source(
        AddCommandType::CreateEvent,
        CardId::derive(TodayCardType::Event, &source),
        prior.id().to_hex(),
        Some(format!("31923:{AUTHOR}:{identifier}")),
        AUTHOR,
    )
    .unwrap();
    Phase1ReviseIntent::new(target, command, vec![], form).unwrap()
}

pub(super) async fn saved(
    runtime: &TeraRuntime,
    id: [u8; 16],
    identifier: &str,
) -> Phase1DraftStatus {
    let (command, form) = calendar(identifier, "New market");
    runtime
        .phase1_save_draft_with_form(
            id,
            command,
            1_750_000_000,
            vec![],
            form,
            None,
            1_750_000_000_000,
        )
        .await
        .unwrap()
}

pub(super) async fn queue(
    runtime: &TeraRuntime,
    saved: &Phase1DraftStatus,
) -> Result<Phase1DraftStatus, Phase1DraftError> {
    runtime
        .phase1_queue_draft(
            *saved.draft().draft_id().as_bytes(),
            saved.draft().revision().get(),
            policy(),
            saved.draft().updated_at_unix_ms() + 1,
        )
        .await
}

pub(super) fn config(root: &std::path::Path) -> MobileUserStoreConfig {
    MobileUserStoreConfig::from_encoded(
        root,
        AUTHOR,
        &"04".repeat(32),
        1_750_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap()
}
