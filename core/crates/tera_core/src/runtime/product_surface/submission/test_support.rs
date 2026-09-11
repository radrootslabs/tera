use super::*;
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerFormInput, ComposerPartialForm,
    ComposerStorageRecord, LocalNetworkId,
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
