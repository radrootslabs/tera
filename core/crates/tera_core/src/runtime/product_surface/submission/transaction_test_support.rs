use super::{
    CapturedSubmission, SubmissionReservationRequest, repository::SubmissionRepository,
    test_support::*,
};
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerPartialForm, ComposerStorageRecord,
};
use radroots_storage::authored_draft::AuthoredDraftStore;

pub(super) async fn capture<S: AuthoredDraftStore + ?Sized>(
    store: &S,
    request: &SubmissionReservationRequest,
    media: bool,
) -> CapturedSubmission {
    let mut input = input(if media {
        AddCommandType::CreatePhotoUpdate
    } else {
        AddCommandType::CreateUpdate
    });
    let bytes = if media {
        let (photo, bytes) = photo();
        input.media.push(photo);
        vec![bytes]
    } else {
        vec![]
    };
    let source = ComposerStorageRecord::initial(
        request.composer_id(),
        request.scope().clone(),
        ComposerEditSequence::INITIAL,
        ComposerPartialForm::new(input).unwrap(),
        NOW,
    )
    .unwrap();
    store
        .append_authored_draft(source.into_stored(), None)
        .await
        .unwrap();
    let reservation = SubmissionRepository { store }
        .reserve(request, Some(NOW))
        .await
        .unwrap();
    CapturedSubmission::capture(
        reservation,
        policy("wss://relay.example"),
        Some(&blossom()),
        bytes,
    )
    .unwrap()
}
