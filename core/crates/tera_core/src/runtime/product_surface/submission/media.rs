//! Bounded byte materialization for the media already captured by one operation.

use std::sync::Arc;

use radroots_blossom::{MediaType, Sha256};
use radroots_sdk::transport::{
    BlossomImageDimensions, BlossomUploadRequest, BlossomUploadTransaction,
};

use super::{
    SubmissionOperationError as E, SubmissionReservationRequest, operation_load::LoadedOperation,
};
use crate::{
    TeraRuntime,
    runtime::product_surface::{Phase1DraftError, Phase1MediaStage},
};

#[path = "media_runtime.rs"]
mod runtime;

#[derive(Clone)]
pub struct SubmissionMediaRequest {
    submission: SubmissionReservationRequest,
    expected_revision: u64,
    reference: String,
    bytes: Arc<[u8]>,
}

impl SubmissionMediaRequest {
    pub fn new(
        submission: SubmissionReservationRequest,
        expected_revision: u64,
        reference: String,
        bytes: Arc<[u8]>,
    ) -> Result<Self, E> {
        if expected_revision == 0
            || reference.is_empty()
            || reference.len() > 4096
            || reference.trim() != reference
            || reference.chars().any(char::is_control)
            || bytes.is_empty()
        {
            return Err(E::InvalidMedia);
        }
        Ok(Self {
            submission,
            expected_revision,
            reference,
            bytes,
        })
    }
}

/// Native HTTP evidence is bounded before any verification or journal mutation.
pub struct SubmissionMediaResponse {
    status_code: u16,
    media_type: Option<String>,
    content_encoding: Option<String>,
    body: Vec<u8>,
}

impl SubmissionMediaResponse {
    pub fn new(
        status_code: u16,
        media_type: Option<String>,
        content_encoding: Option<String>,
        body: Vec<u8>,
    ) -> Result<Self, E> {
        let size = body
            .len()
            .saturating_add(media_type.as_ref().map_or(0, String::len))
            .saturating_add(content_encoding.as_ref().map_or(0, String::len));
        if size > 16_384 {
            return Err(E::InvalidMedia);
        }
        Ok(Self {
            status_code,
            media_type,
            content_encoding,
            body,
        })
    }
}

impl TeraRuntime {
    fn materialize_submission_media(
        &self,
        loaded: &LoadedOperation,
        input: &SubmissionMediaRequest,
        completing: bool,
    ) -> Result<BlossomUploadTransaction, E> {
        if loaded.head.revision().get() != input.expected_revision {
            return Err(Phase1DraftError::RevisionConflict.into());
        }
        let media = loaded
            .payload
            .media()
            .iter()
            .find(|media| media.local_reference() == input.reference)
            .ok_or(E::InvalidMedia)?;
        if loaded.head.operation_id().is_some()
            || if completing {
                media.stage() != Phase1MediaStage::Uploading
            } else {
                !matches!(
                    media.stage(),
                    Phase1MediaStage::Pending
                        | Phase1MediaStage::Preparing
                        | Phase1MediaStage::Uploading
                        | Phase1MediaStage::Failed
                )
            }
        {
            return Err(E::InvalidMedia);
        }
        let source = loaded
            .captured
            .form()
            .input()
            .media
            .iter()
            .find(|media| media.opaque_reference == input.reference)
            .ok_or(E::InvalidMedia)?;
        if source.byte_size != input.bytes.len() as u64
            || source.sha256 != Sha256::digest(&input.bytes).to_hex()
        {
            return Err(E::InvalidMedia);
        }
        let request = BlossomUploadRequest::new(
            input.bytes.clone(),
            MediaType::parse(&source.media_type).map_err(|_| E::InvalidMedia)?,
            BlossomImageDimensions::new(source.width, source.height)
                .map_err(|_| E::InvalidMedia)?,
            crate::runtime::product_surface::phase1_operation_now_unix_ms()?,
        )
        .map_err(|_| E::InvalidMedia)?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        let transaction = blossom
            .prepare_upload(request)
            .map_err(|_| E::InvalidMedia)?;
        if Some(*transaction.config_fingerprint().as_bytes()) != loaded.payload.media_policy() {
            return Err(E::MediaPolicyChanged);
        }
        if transaction.expected_url().as_str() != media.url() {
            return Err(E::InvalidMedia);
        }
        Ok(transaction)
    }
}
