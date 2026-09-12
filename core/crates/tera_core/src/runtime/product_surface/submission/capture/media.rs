use super::SubmissionCaptureError as E;
use crate::runtime::product_surface::ComposerMediaInput;
use radroots_blossom::{BlobDescriptor, ByteVerifiedDescriptor, MediaType, Sha256};
use radroots_event::{
    media::AuthoredImage,
    post::{AuthoredPostImage, PostImageDimensions},
};
use radroots_sdk::transport::{
    BlossomConfigFingerprint, BlossomImageDimensions, BlossomSlot, BlossomUploadRequest,
};
use std::sync::Arc;

pub(super) struct Prepared {
    pub input: ComposerMediaInput,
    pub descriptor: ByteVerifiedDescriptor,
}

impl Prepared {
    pub fn image(&self) -> Result<AuthoredImage, E> {
        AuthoredImage::try_from_verified_descriptor(self.descriptor.clone())
            .map_err(|_| E::InvalidInput("invalid_image_media"))
    }
    pub fn post_image(&self) -> Result<AuthoredPostImage, E> {
        AuthoredPostImage::new(
            self.image()?,
            PostImageDimensions::new(self.input.width, self.input.height)
                .map_err(|_| E::InvalidInput("invalid_image_dimensions"))?,
            self.input.alt.clone(),
        )
        .map_err(|_| E::InvalidInput("invalid_image"))
    }
}

pub(super) fn prepare(
    inputs: &[ComposerMediaInput],
    blossom: Option<&BlossomSlot>,
    expected_policy: Option<BlossomConfigFingerprint>,
    bytes: Vec<Arc<[u8]>>,
) -> Result<Vec<Prepared>, E> {
    if inputs.len() != bytes.len() {
        return Err(E::InvalidInput("media_count_mismatch"));
    }
    inputs
        .iter()
        .zip(bytes)
        .map(|(input, bytes)| {
            let blossom = blossom.ok_or(E::InvalidInput("blossom_not_configured"))?;
            let media_type = MediaType::parse(&input.media_type)
                .map_err(|_| E::InvalidInput("invalid_media_type"))?;
            let hash = Sha256::from_hex(&input.sha256)
                .map_err(|_| E::InvalidInput("invalid_media_digest"))?;
            if input.byte_size != bytes.len() as u64 || hash != Sha256::digest(&bytes) {
                return Err(E::InvalidInput("media_verification_failed"));
            }
            let dimensions = BlossomImageDimensions::new(input.width, input.height)
                .map_err(|_| E::InvalidInput("invalid_image_dimensions"))?;
            let verified_at = input
                .prepared_at_unix_s
                .checked_mul(1000)
                .ok_or(E::InvalidInput("invalid_media_time"))?;
            let request = BlossomUploadRequest::new(
                Arc::clone(&bytes),
                media_type.clone(),
                dimensions,
                verified_at,
            )
            .map_err(|_| E::InvalidInput("media_verification_failed"))?;
            let transaction = blossom
                .prepare_upload(request)
                .map_err(|_| E::InvalidInput("invalid_media_policy"))?;
            if Some(transaction.config_fingerprint()) != expected_policy {
                return Err(E::InvalidInput("media_policy_changed"));
            }
            let descriptor = BlobDescriptor::new(
                transaction.expected_url().clone(),
                hash,
                input.byte_size,
                media_type.clone(),
                input.prepared_at_unix_s,
            )
            .and_then(BlobDescriptor::approve_reference)
            .and_then(|descriptor| descriptor.verify_bytes(&bytes, &media_type))
            .map_err(|_| E::InvalidInput("media_verification_failed"))?;
            Ok(Prepared {
                input: input.clone(),
                descriptor,
            })
        })
        .collect()
}
