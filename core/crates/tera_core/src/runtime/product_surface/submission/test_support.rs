use super::*;
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerFormInput, ComposerMediaInput,
    ComposerPartialForm, ComposerStorageRecord, LocalNetworkId, Phase1CancellationPolicy,
    Phase1QueuePolicy, Phase1RelaySatisfaction,
};
use radroots_identity::PublicKey;

pub(super) const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
pub(super) const OTHER: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
pub(super) const NOW: u64 = 1_800_000_000_000;

pub(super) fn scope(author: &str, context: &str) -> ComposerScope {
    ComposerScope::new(
        PublicKey::from_hex(author).unwrap(),
        LocalNetworkId::new(context.into()).unwrap(),
    )
}

pub(super) fn form(content: &str) -> ComposerPartialForm {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    input.content = content.into();
    input.price_amount = Some("-".into());
    input.event_start_date = Some("2026-09-".into());
    ComposerPartialForm::new(input).unwrap()
}

pub(super) fn source() -> ComposerStorageRecord {
    ComposerStorageRecord::initial(
        ComposerId::new([3; 16]).unwrap(),
        scope(AUTHOR, "nearby"),
        ComposerEditSequence::INITIAL,
        form("PRIVATE unfinished café"),
        NOW,
    )
    .unwrap()
}

pub(super) fn request() -> SubmissionReservationRequest {
    SubmissionReservationRequest::new(
        SubmissionCommandId::new([7; 16]).unwrap(),
        scope(AUTHOR, "nearby"),
        ComposerId::new([3; 16]).unwrap(),
        ComposerRevision::INITIAL,
    )
}

use radroots_sdk::transport::{
    BlossomConfig, BlossomEndpointAuthority, BlossomHostKind, BlossomProfile, BlossomSlot,
};
use std::sync::Arc;

pub(super) fn policy(relay: &str) -> Phase1QueuePolicy {
    Phase1QueuePolicy::new(
        vec![relay.into()],
        Phase1RelaySatisfaction::AllAccepted,
        NOW + 1000,
        Phase1CancellationPolicy::LocalCooperative,
    )
    .unwrap()
}

pub(super) fn blossom() -> BlossomSlot {
    let slot = BlossomSlot::new();
    slot.configure(BlossomConfig::from_profile(
        BlossomProfile::new(
            BlossomHostKind::Simulator,
            BlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:3000",
            std::iter::empty::<&str>(),
        )
        .unwrap(),
    ))
    .unwrap();
    slot
}

pub(super) fn input(kind: AddCommandType) -> ComposerFormInput {
    let mut input = ComposerFormInput::empty(kind);
    input.content = "PRIVATE harvest café".into();
    input
}

pub(super) fn photo() -> (ComposerMediaInput, Arc<[u8]>) {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    (
        ComposerMediaInput {
            opaque_reference: "media:harvest".into(),
            sha256: radroots_blossom::Sha256::digest(&bytes).to_hex(),
            media_type: "image/png".into(),
            byte_size: bytes.len() as u64,
            width: 2,
            height: 2,
            alt: "Harvest".into(),
            prepared_at_unix_s: NOW / 1000,
        },
        bytes.into(),
    )
}
