use std::sync::Arc;

use radroots_blossom::{MediaType, Sha256};
use radroots_sdk::transport::{BlossomImageDimensions, BlossomUploadRequest};
use radroots_signing::SigningOperationId;
use radroots_storage::authored_draft::{AuthoredDraftId, AuthoredDraftRevision};

use super::super::recovery_reconciliation::{
    RECOVERY_MEDIA_TYPE_MAX_BYTES, RECOVERY_URL_MAX_BYTES,
};
use super::{Phase1DraftError as E, SubmissionMediaResponse};

const SOURCE_MAX_BYTES: usize = 10 * 1024 * 1024;

pub struct RecoveryNativeIdentity {
    pub(in crate::runtime::product_surface) parent: AuthoredDraftId,
    pub(in crate::runtime::product_surface) revision: AuthoredDraftRevision,
    pub(in crate::runtime::product_surface) attempt: SigningOperationId,
    pub(in crate::runtime::product_surface) upload_url: String,
}

impl RecoveryNativeIdentity {
    pub fn new(
        parent: [u8; 16],
        revision: u64,
        attempt: [u8; 16],
        upload_url: String,
    ) -> Result<Self, E> {
        if revision > i64::MAX as u64
            || upload_url.is_empty()
            || upload_url.len() > RECOVERY_URL_MAX_BYTES
            || upload_url.trim() != upload_url
            || upload_url.chars().any(char::is_control)
        {
            return Err(E::InvalidMedia);
        }
        Ok(Self {
            parent: AuthoredDraftId::new(parent).map_err(|_| E::InvalidMedia)?,
            revision: AuthoredDraftRevision::new(revision).map_err(|_| E::InvalidMedia)?,
            attempt: SigningOperationId::new(attempt).map_err(|_| E::InvalidMedia)?,
            upload_url,
        })
    }
}

pub struct RecoveryNativeMedia {
    pub(in crate::runtime::product_surface) hash: Sha256,
    pub(in crate::runtime::product_surface) media_type: MediaType,
    pub(in crate::runtime::product_surface) byte_size: u64,
}

impl RecoveryNativeMedia {
    pub fn new(hash: Sha256, media_type: MediaType, byte_size: u64) -> Result<Self, E> {
        if byte_size == 0
            || byte_size > SOURCE_MAX_BYTES as u64
            || media_type.as_str().len() > RECOVERY_MEDIA_TYPE_MAX_BYTES
        {
            return Err(E::InvalidMedia);
        }
        Ok(Self {
            hash,
            media_type,
            byte_size,
        })
    }
}

pub struct RecoveryNativeReceipt {
    pub(in crate::runtime::product_surface) identity: RecoveryNativeIdentity,
    pub(in crate::runtime::product_surface) media: RecoveryNativeMedia,
    pub(in crate::runtime::product_surface) response: SubmissionMediaResponse,
}

impl RecoveryNativeReceipt {
    pub fn new(
        identity: RecoveryNativeIdentity,
        media: RecoveryNativeMedia,
        response: SubmissionMediaResponse,
    ) -> Self {
        Self {
            identity,
            media,
            response,
        }
    }
}

pub struct RecoveryMedia {
    pub(in crate::runtime::product_surface) reference: String,
    pub(in crate::runtime::product_surface) bytes: Arc<[u8]>,
    pub(in crate::runtime::product_surface) media_type: MediaType,
    pub(in crate::runtime::product_surface) dimensions: BlossomImageDimensions,
}

impl RecoveryMedia {
    pub fn new(
        reference: String,
        bytes: Arc<[u8]>,
        media_type: MediaType,
        width: u32,
        height: u32,
    ) -> Result<Self, E> {
        if reference.is_empty()
            || reference.len() > 4096
            || reference.trim() != reference
            || reference.chars().any(char::is_control)
            || bytes.is_empty()
            || bytes.len() > SOURCE_MAX_BYTES
            || media_type.as_str().len() > RECOVERY_MEDIA_TYPE_MAX_BYTES
        {
            return Err(E::InvalidMedia);
        }
        Ok(Self {
            reference,
            bytes,
            media_type,
            dimensions: BlossomImageDimensions::new(width, height).map_err(|_| E::InvalidMedia)?,
        })
    }

    pub(in crate::runtime::product_surface) fn request_at(
        &self,
        at: u64,
    ) -> Result<BlossomUploadRequest, E> {
        BlossomUploadRequest::new(
            self.bytes.clone(),
            self.media_type.clone(),
            self.dimensions,
            at,
        )
        .map_err(|_| E::InvalidMedia)
    }
}
