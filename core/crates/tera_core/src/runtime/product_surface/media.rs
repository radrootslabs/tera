//! Typed inbound-media trust, receipt, and bounded cache metadata.
//!
//! Structural Nostr references are intentionally distinct from locally
//! verified artifacts. A caller cannot represent renderable media with a URL
//! and a boolean: the `Verified` state always contains a receipt derived from
//! an actual byte commitment and bound to the active retrieval configuration.

use std::collections::BTreeMap;
#[cfg(feature = "mobile-social")]
use std::path::{Path, PathBuf};

use radroots_blossom::{BlobUrl, MediaType, Sha256, descriptor::ByteCommitment};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256 as Sha256Hasher};
use thiserror::Error;
#[cfg(feature = "mobile-social")]
use tokio::io::AsyncWriteExt;

const MEDIA_REFERENCE_SCHEMA_VERSION: u16 = 1;
const MEDIA_RECEIPT_SCHEMA_VERSION: u16 = 1;
const MEDIA_CACHE_SCHEMA_VERSION: u16 = 1;
const MEDIA_URL_MAX_BYTES: usize = 8_192;
const MEDIA_ALT_MAX_BYTES: usize = 2_048;
const MEDIA_FAILURE_CODE_MAX_BYTES: usize = 96;
const MEDIA_DIMENSION_MAX_EDGE: u32 = 16_384;
const MEDIA_DIMENSION_MAX_PIXELS: u64 = 100_000_000;
const MEDIA_REFERENCE_FINGERPRINT_DOMAIN: &[u8] = b"radroots.inbound-media-reference.v1\0";
#[cfg(feature = "mobile-social")]
const MEDIA_CACHE_EXTENSIONS: &[&str] = &["gif", "jpg", "png", "webp"];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Phase1MediaConfigurationFingerprint([u8; 32]);

impl Phase1MediaConfigurationFingerprint {
    pub fn new(value: [u8; 32]) -> Result<Self, Phase1InboundMediaError> {
        (value != [0; 32])
            .then_some(Self(value))
            .ok_or(Phase1InboundMediaError::InvalidConfiguration)
    }

    pub fn parse(value: &str) -> Result<Self, Phase1InboundMediaError> {
        let decoded =
            hex::decode(value).map_err(|_| Phase1InboundMediaError::InvalidConfiguration)?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| Phase1InboundMediaError::InvalidConfiguration)?;
        Self::new(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }

    fn validate(self) -> Result<(), Phase1InboundMediaError> {
        Self::new(self.0).map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Phase1MediaArtifactId([u8; 32]);

impl Phase1MediaArtifactId {
    pub fn parse(value: &str) -> Result<Self, Phase1InboundMediaError> {
        let hash = Sha256::from_hex(value).map_err(|_| Phase1InboundMediaError::InvalidDigest)?;
        Ok(Self(*hash.as_bytes()))
    }

    pub const fn from_sha256(value: Sha256) -> Self {
        Self(*value.as_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

/// Signed-event media facts. These facts do not imply that any bytes were
/// fetched, trusted, stored, or rendered.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Phase1StructuralMediaReference {
    schema_version: u16,
    source_url: String,
    expected_sha256: Option<String>,
    expected_media_type: Option<String>,
    expected_width: Option<u32>,
    expected_height: Option<u32>,
    expected_byte_size: Option<u64>,
    alt: Option<String>,
    fingerprint: [u8; 32],
}

impl Phase1StructuralMediaReference {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_url: impl Into<String>,
        expected_sha256: Option<String>,
        expected_media_type: Option<String>,
        expected_width: Option<u32>,
        expected_height: Option<u32>,
        expected_byte_size: Option<u64>,
        alt: Option<String>,
    ) -> Result<Self, Phase1InboundMediaError> {
        let source_url = source_url.into();
        validate_url_text(&source_url)?;
        let parsed =
            url::Url::parse(&source_url).map_err(|_| Phase1InboundMediaError::InvalidReference)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.as_str() != source_url
        {
            return Err(Phase1InboundMediaError::InvalidReference);
        }
        let path_digest = BlobUrl::parse(&source_url)
            .ok()
            .map(|value| value.hash_path().hash().to_hex());
        let expected_sha256 = match expected_sha256 {
            Some(value) => {
                let digest =
                    Sha256::from_hex(&value).map_err(|_| Phase1InboundMediaError::InvalidDigest)?;
                if digest.to_hex() != value
                    || path_digest
                        .as_deref()
                        .is_some_and(|path| digest.to_hex() != path)
                {
                    return Err(Phase1InboundMediaError::MetadataMismatch);
                }
                Some(value)
            }
            None => path_digest,
        };
        let expected_media_type = expected_media_type
            .map(|value| {
                MediaType::parse(&value)
                    .map(|parsed| parsed.to_string())
                    .map_err(|_| Phase1InboundMediaError::InvalidMediaType)
            })
            .transpose()?;
        validate_dimensions(expected_width, expected_height)?;
        if expected_byte_size == Some(0) {
            return Err(Phase1InboundMediaError::InvalidByteSize);
        }
        if alt.as_deref().is_some_and(|value| {
            value.len() > MEDIA_ALT_MAX_BYTES || value.chars().any(char::is_control)
        }) {
            return Err(Phase1InboundMediaError::InvalidAlt);
        }
        let mut value = Self {
            schema_version: MEDIA_REFERENCE_SCHEMA_VERSION,
            source_url: parsed.to_string(),
            expected_sha256,
            expected_media_type,
            expected_width,
            expected_height,
            expected_byte_size,
            alt,
            fingerprint: [0; 32],
        };
        value.fingerprint = value.derive_fingerprint();
        Ok(value)
    }

    fn validate(&self) -> Result<(), Phase1InboundMediaError> {
        if self.schema_version != MEDIA_REFERENCE_SCHEMA_VERSION {
            return Err(Phase1InboundMediaError::UnsupportedSchema);
        }
        let canonical = Self::new(
            self.source_url.clone(),
            self.expected_sha256.clone(),
            self.expected_media_type.clone(),
            self.expected_width,
            self.expected_height,
            self.expected_byte_size,
            self.alt.clone(),
        )?;
        (canonical == *self)
            .then_some(())
            .ok_or(Phase1InboundMediaError::CorruptState)
    }

    fn derive_fingerprint(&self) -> [u8; 32] {
        let mut digest = Sha256Hasher::new();
        digest.update(MEDIA_REFERENCE_FINGERPRINT_DOMAIN);
        digest.update(self.source_url.as_bytes());
        update_optional(&mut digest, self.expected_sha256.as_deref());
        update_optional(&mut digest, self.expected_media_type.as_deref());
        update_optional_u64(&mut digest, self.expected_width.map(u64::from));
        update_optional_u64(&mut digest, self.expected_height.map(u64::from));
        update_optional_u64(&mut digest, self.expected_byte_size);
        update_optional(&mut digest, self.alt.as_deref());
        digest.finalize().into()
    }

    pub fn source_url(&self) -> &str {
        self.source_url.as_str()
    }

    pub fn expected_sha256(&self) -> Option<&str> {
        self.expected_sha256.as_deref()
    }

    pub fn expected_media_type(&self) -> Option<&str> {
        self.expected_media_type.as_deref()
    }

    pub const fn expected_width(&self) -> Option<u32> {
        self.expected_width
    }

    pub const fn expected_height(&self) -> Option<u32> {
        self.expected_height
    }

    pub const fn expected_byte_size(&self) -> Option<u64> {
        self.expected_byte_size
    }

    pub fn alt(&self) -> Option<&str> {
        self.alt.as_deref()
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Phase1InboundMediaPending {
    operation_id: [u8; 16],
    configuration: Phase1MediaConfigurationFingerprint,
    started_at_unix_ms: u64,
}

impl Phase1InboundMediaPending {
    pub fn new(
        operation_id: [u8; 16],
        configuration: Phase1MediaConfigurationFingerprint,
        started_at_unix_ms: u64,
    ) -> Result<Self, Phase1InboundMediaError> {
        configuration.validate()?;
        if operation_id == [0; 16] || started_at_unix_ms == 0 {
            return Err(Phase1InboundMediaError::InvalidOperation);
        }
        Ok(Self {
            operation_id,
            configuration,
            started_at_unix_ms,
        })
    }

    pub const fn operation_id(&self) -> &[u8; 16] {
        &self.operation_id
    }

    pub const fn configuration(&self) -> Phase1MediaConfigurationFingerprint {
        self.configuration
    }

    pub const fn started_at_unix_ms(&self) -> u64 {
        self.started_at_unix_ms
    }

    fn validate(&self) -> Result<(), Phase1InboundMediaError> {
        Self::new(
            self.operation_id,
            self.configuration,
            self.started_at_unix_ms,
        )
        .map(|_| ())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Phase1InboundMediaFailure {
    operation_id: [u8; 16],
    safe_code: String,
    retryable: bool,
    failed_at_unix_ms: u64,
}

impl Phase1InboundMediaFailure {
    pub fn new(
        operation_id: [u8; 16],
        safe_code: impl Into<String>,
        retryable: bool,
        failed_at_unix_ms: u64,
    ) -> Result<Self, Phase1InboundMediaError> {
        let safe_code = safe_code.into();
        if operation_id == [0; 16]
            || failed_at_unix_ms == 0
            || safe_code.is_empty()
            || safe_code.len() > MEDIA_FAILURE_CODE_MAX_BYTES
            || !safe_code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(Phase1InboundMediaError::InvalidFailure);
        }
        Ok(Self {
            operation_id,
            safe_code,
            retryable,
            failed_at_unix_ms,
        })
    }

    pub fn safe_code(&self) -> &str {
        self.safe_code.as_str()
    }

    pub const fn retryable(&self) -> bool {
        self.retryable
    }

    fn validate(&self) -> Result<(), Phase1InboundMediaError> {
        Self::new(
            self.operation_id,
            self.safe_code.clone(),
            self.retryable,
            self.failed_at_unix_ms,
        )
        .map(|_| ())
    }
}

/// Exact-byte verification evidence. Construction requires a byte commitment,
/// binds every signed expected field, and derives the artifact identity from
/// the observed digest rather than caller input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Phase1VerifiedMediaReceipt {
    schema_version: u16,
    reference_fingerprint: [u8; 32],
    source_url: String,
    canonical_final_url: String,
    expected_sha256: String,
    observed_sha256: String,
    byte_size: u64,
    media_type: String,
    extension: String,
    width: u32,
    height: u32,
    artifact_id: Phase1MediaArtifactId,
    configuration: Phase1MediaConfigurationFingerprint,
    verified_at_unix_ms: u64,
}

impl Phase1VerifiedMediaReceipt {
    pub fn from_commitment(
        reference: &Phase1StructuralMediaReference,
        canonical_final_url: BlobUrl,
        commitment: &ByteCommitment,
        width: u32,
        height: u32,
        configuration: Phase1MediaConfigurationFingerprint,
        verified_at_unix_ms: u64,
    ) -> Result<Self, Phase1InboundMediaError> {
        reference.validate()?;
        configuration.validate()?;
        if verified_at_unix_ms == 0 {
            return Err(Phase1InboundMediaError::InvalidVerificationTime);
        }
        BlobUrl::parse(reference.source_url())
            .and_then(BlobUrl::approve)
            .map_err(|_| Phase1InboundMediaError::InvalidReference)?;
        canonical_final_url
            .clone()
            .approve()
            .map_err(|_| Phase1InboundMediaError::InvalidReference)?;
        validate_dimensions(Some(width), Some(height))?;
        let observed_sha256 = commitment.sha256().to_hex();
        let expected_sha256 = reference
            .expected_sha256()
            .ok_or(Phase1InboundMediaError::MissingDigest)?;
        if observed_sha256 != expected_sha256
            || reference
                .expected_byte_size()
                .is_some_and(|value| value != commitment.size())
            || reference
                .expected_media_type()
                .is_some_and(|value| value != commitment.media_type().to_string())
            || reference
                .expected_width()
                .is_some_and(|value| value != width)
            || reference
                .expected_height()
                .is_some_and(|value| value != height)
        {
            return Err(Phase1InboundMediaError::MetadataMismatch);
        }
        let extension = canonical_final_url
            .hash_path()
            .extension()
            .ok_or(Phase1InboundMediaError::InvalidReference)?
            .as_str()
            .to_owned();
        if canonical_extension(commitment.media_type()) != Some(extension.as_str()) {
            return Err(Phase1InboundMediaError::MetadataMismatch);
        }
        if canonical_final_url.hash_path().hash() != commitment.sha256() {
            return Err(Phase1InboundMediaError::MetadataMismatch);
        }
        let receipt = Self {
            schema_version: MEDIA_RECEIPT_SCHEMA_VERSION,
            reference_fingerprint: *reference.fingerprint(),
            source_url: reference.source_url().to_owned(),
            canonical_final_url: canonical_final_url.to_string(),
            expected_sha256: expected_sha256.to_owned(),
            observed_sha256,
            byte_size: commitment.size(),
            media_type: commitment.media_type().to_string(),
            extension,
            width,
            height,
            artifact_id: Phase1MediaArtifactId::from_sha256(commitment.sha256()),
            configuration,
            verified_at_unix_ms,
        };
        receipt.validate(reference)?;
        Ok(receipt)
    }

    fn validate(
        &self,
        reference: &Phase1StructuralMediaReference,
    ) -> Result<(), Phase1InboundMediaError> {
        self.validate_intrinsic()?;
        if self.reference_fingerprint != *reference.fingerprint()
            || self.source_url != reference.source_url()
            || reference.expected_sha256() != Some(self.expected_sha256.as_str())
        {
            return Err(Phase1InboundMediaError::CorruptReceipt);
        }
        BlobUrl::parse(reference.source_url())
            .and_then(BlobUrl::approve)
            .map_err(|_| Phase1InboundMediaError::CorruptReceipt)?;
        if reference
            .expected_byte_size()
            .is_some_and(|value| value != self.byte_size)
            || reference
                .expected_media_type()
                .is_some_and(|value| value != self.media_type)
            || reference
                .expected_width()
                .is_some_and(|value| value != self.width)
            || reference
                .expected_height()
                .is_some_and(|value| value != self.height)
        {
            return Err(Phase1InboundMediaError::CorruptReceipt);
        }
        Ok(())
    }

    fn validate_intrinsic(&self) -> Result<(), Phase1InboundMediaError> {
        if self.schema_version != MEDIA_RECEIPT_SCHEMA_VERSION
            || self.expected_sha256 != self.observed_sha256
            || self.expected_sha256 != self.artifact_id.to_hex()
            || self.byte_size == 0
            || self.verified_at_unix_ms == 0
        {
            return Err(Phase1InboundMediaError::CorruptReceipt);
        }
        self.configuration
            .validate()
            .map_err(|_| Phase1InboundMediaError::CorruptReceipt)?;
        let final_url = BlobUrl::parse(&self.canonical_final_url)
            .map_err(|_| Phase1InboundMediaError::CorruptReceipt)?;
        final_url
            .clone()
            .approve()
            .map_err(|_| Phase1InboundMediaError::CorruptReceipt)?;
        if final_url.to_string() != self.canonical_final_url
            || final_url.hash_path().hash().to_hex() != self.observed_sha256
            || final_url
                .hash_path()
                .extension()
                .is_none_or(|value| value.as_str() != self.extension)
            || !canonical_media_type(&self.media_type)
            || MediaType::parse(&self.media_type)
                .ok()
                .and_then(|value| canonical_extension(&value))
                != Some(self.extension.as_str())
            || validate_dimensions(Some(self.width), Some(self.height)).is_err()
        {
            return Err(Phase1InboundMediaError::CorruptReceipt);
        }
        Ok(())
    }

    pub const fn artifact_id(&self) -> Phase1MediaArtifactId {
        self.artifact_id
    }

    pub const fn configuration(&self) -> Phase1MediaConfigurationFingerprint {
        self.configuration
    }

    pub fn canonical_final_url(&self) -> &str {
        self.canonical_final_url.as_str()
    }

    pub fn observed_sha256(&self) -> &str {
        self.observed_sha256.as_str()
    }

    pub const fn byte_size(&self) -> u64 {
        self.byte_size
    }

    pub fn media_type(&self) -> &str {
        self.media_type.as_str()
    }

    pub fn extension(&self) -> &str {
        self.extension.as_str()
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn verified_at_unix_ms(&self) -> u64 {
        self.verified_at_unix_ms
    }
}

/// One immutable exact-byte artifact in the authenticated user's local cache.
#[cfg(feature = "mobile-social")]
#[derive(Clone, Eq, PartialEq)]
pub struct Phase1LocalMediaArtifact {
    artifact_id: Phase1MediaArtifactId,
    local_path: PathBuf,
    bytes: Vec<u8>,
    byte_size: u64,
    media_type: String,
    width: u32,
    height: u32,
}

#[cfg(feature = "mobile-social")]
impl std::fmt::Debug for Phase1LocalMediaArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Phase1LocalMediaArtifact")
            .field("artifact_id", &self.artifact_id)
            .field("local_path", &"<redacted>")
            .field("byte_size", &self.byte_size)
            .field("media_type", &self.media_type)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

#[cfg(feature = "mobile-social")]
impl Phase1LocalMediaArtifact {
    pub const fn artifact_id(&self) -> Phase1MediaArtifactId {
        self.artifact_id
    }

    pub fn local_path(&self) -> &Path {
        self.local_path.as_path()
    }

    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub const fn byte_size(&self) -> u64 {
        self.byte_size
    }

    pub fn media_type(&self) -> &str {
        self.media_type.as_str()
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "evidence")]
pub enum Phase1InboundMediaState {
    #[default]
    Unavailable,
    Pending(Phase1InboundMediaPending),
    Failed(Phase1InboundMediaFailure),
    Verified(Box<Phase1VerifiedMediaReceipt>),
}

/// Public media model: signed structure plus local retrieval evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MediaReference {
    structural: Phase1StructuralMediaReference,
    retrieval: Phase1InboundMediaState,
}

impl MediaReference {
    pub fn new(
        structural: Phase1StructuralMediaReference,
    ) -> Result<Self, Phase1InboundMediaError> {
        structural.validate()?;
        Ok(Self {
            structural,
            retrieval: Phase1InboundMediaState::Unavailable,
        })
    }

    pub(crate) fn legacy_unavailable(
        source_url: String,
        expected_sha256: Option<String>,
        expected_media_type: Option<String>,
        expected_width: Option<u32>,
        expected_height: Option<u32>,
        expected_byte_size: Option<u64>,
        alt: Option<String>,
    ) -> Result<Self, Phase1InboundMediaError> {
        Self::new(Phase1StructuralMediaReference::new(
            source_url,
            expected_sha256,
            expected_media_type,
            expected_width,
            expected_height,
            expected_byte_size,
            alt,
        )?)
    }

    pub fn structural(&self) -> &Phase1StructuralMediaReference {
        &self.structural
    }

    pub const fn retrieval(&self) -> &Phase1InboundMediaState {
        &self.retrieval
    }

    pub(crate) fn validate(&self) -> Result<(), Phase1InboundMediaError> {
        self.structural.validate()?;
        match &self.retrieval {
            Phase1InboundMediaState::Unavailable => Ok(()),
            Phase1InboundMediaState::Pending(value) => value.validate(),
            Phase1InboundMediaState::Failed(value) => value.validate(),
            Phase1InboundMediaState::Verified(value) => value.validate(&self.structural),
        }
    }

    pub(crate) fn restore(
        &mut self,
        retrieval: Phase1InboundMediaState,
        cache: &Phase1MediaCacheIndex,
    ) -> Result<(), Phase1InboundMediaError> {
        let mut candidate = self.clone();
        candidate.retrieval = retrieval;
        candidate.validate()?;
        if let Phase1InboundMediaState::Verified(receipt) = &candidate.retrieval
            && !cache.contains(receipt)
        {
            candidate.retrieval = Phase1InboundMediaState::Unavailable;
        }
        *self = candidate;
        Ok(())
    }

    pub fn begin(
        &mut self,
        pending: Phase1InboundMediaPending,
    ) -> Result<(), Phase1InboundMediaError> {
        self.structural.validate()?;
        pending.validate()?;
        self.retrieval = Phase1InboundMediaState::Pending(pending);
        Ok(())
    }

    pub fn fail(
        &mut self,
        failure: Phase1InboundMediaFailure,
    ) -> Result<(), Phase1InboundMediaError> {
        failure.validate()?;
        match &self.retrieval {
            Phase1InboundMediaState::Pending(pending)
                if pending.operation_id == failure.operation_id =>
            {
                self.retrieval = Phase1InboundMediaState::Failed(failure);
                Ok(())
            }
            _ => Err(Phase1InboundMediaError::OperationMismatch),
        }
    }

    pub fn verify(
        &mut self,
        operation_id: [u8; 16],
        receipt: Phase1VerifiedMediaReceipt,
    ) -> Result<(), Phase1InboundMediaError> {
        let Phase1InboundMediaState::Pending(pending) = &self.retrieval else {
            return Err(Phase1InboundMediaError::OperationMismatch);
        };
        if pending.operation_id != operation_id || pending.configuration != receipt.configuration {
            return Err(Phase1InboundMediaError::OperationMismatch);
        }
        receipt.validate(&self.structural)?;
        self.retrieval = Phase1InboundMediaState::Verified(Box::new(receipt));
        Ok(())
    }

    pub fn invalidate(&mut self) -> Option<Phase1MediaArtifactId> {
        let artifact = match &self.retrieval {
            Phase1InboundMediaState::Verified(receipt) => Some(receipt.artifact_id),
            _ => None,
        };
        self.retrieval = Phase1InboundMediaState::Unavailable;
        artifact
    }

    pub fn is_renderable_with(
        &self,
        cache: &Phase1MediaCacheIndex,
        configuration: Phase1MediaConfigurationFingerprint,
    ) -> bool {
        match &self.retrieval {
            Phase1InboundMediaState::Verified(receipt)
                if receipt.configuration == configuration =>
            {
                cache.contains(receipt)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Phase1MediaCachePolicy {
    max_bytes: u64,
    max_artifacts: u32,
}

impl Phase1MediaCachePolicy {
    pub fn new(max_bytes: u64, max_artifacts: u32) -> Result<Self, Phase1InboundMediaError> {
        if max_bytes == 0 || max_artifacts == 0 {
            return Err(Phase1InboundMediaError::InvalidCachePolicy);
        }
        Ok(Self {
            max_bytes,
            max_artifacts,
        })
    }

    pub const fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub const fn max_artifacts(&self) -> u32 {
        self.max_artifacts
    }
}

impl Default for Phase1MediaCachePolicy {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_artifacts: 2_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Phase1MediaCacheEntry {
    artifact_id: Phase1MediaArtifactId,
    byte_size: u64,
    media_type: String,
    extension: String,
    width: u32,
    height: u32,
    cached_at_unix_ms: u64,
    last_accessed_at_unix_ms: u64,
}

impl Phase1MediaCacheEntry {
    fn from_receipt(
        receipt: &Phase1VerifiedMediaReceipt,
        cached_at_unix_ms: u64,
    ) -> Result<Self, Phase1InboundMediaError> {
        if cached_at_unix_ms < receipt.verified_at_unix_ms {
            return Err(Phase1InboundMediaError::InvalidCacheObservation);
        }
        Ok(Self {
            artifact_id: receipt.artifact_id,
            byte_size: receipt.byte_size,
            media_type: receipt.media_type.clone(),
            extension: receipt.extension.clone(),
            width: receipt.width,
            height: receipt.height,
            cached_at_unix_ms,
            last_accessed_at_unix_ms: cached_at_unix_ms,
        })
    }

    fn matches(&self, receipt: &Phase1VerifiedMediaReceipt) -> bool {
        self.artifact_id == receipt.artifact_id
            && self.byte_size == receipt.byte_size
            && self.media_type == receipt.media_type
            && self.extension == receipt.extension
            && self.width == receipt.width
            && self.height == receipt.height
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Phase1MediaCacheIndex {
    schema_version: u16,
    configuration: Option<Phase1MediaConfigurationFingerprint>,
    entries: BTreeMap<String, Phase1MediaCacheEntry>,
}

impl Default for Phase1MediaCacheIndex {
    fn default() -> Self {
        Self {
            schema_version: MEDIA_CACHE_SCHEMA_VERSION,
            configuration: None,
            entries: BTreeMap::new(),
        }
    }
}

impl Phase1MediaCacheIndex {
    pub fn admit(
        &mut self,
        receipt: &Phase1VerifiedMediaReceipt,
        policy: Phase1MediaCachePolicy,
        cached_at_unix_ms: u64,
    ) -> Result<Vec<Phase1MediaArtifactId>, Phase1InboundMediaError> {
        self.validate()?;
        receipt.validate_intrinsic()?;
        if receipt.byte_size > policy.max_bytes {
            return Err(Phase1InboundMediaError::CacheQuotaExceeded);
        }
        if self
            .configuration
            .is_some_and(|value| value != receipt.configuration)
        {
            return Err(Phase1InboundMediaError::ConfigurationMismatch);
        }
        self.configuration = Some(receipt.configuration);
        let key = receipt.artifact_id.to_hex();
        let entry = Phase1MediaCacheEntry::from_receipt(receipt, cached_at_unix_ms)?;
        if self
            .entries
            .get(&key)
            .is_some_and(|existing| !existing.matches(receipt))
        {
            return Err(Phase1InboundMediaError::ArtifactCollision);
        }
        self.entries.insert(key.clone(), entry);
        let mut evicted = Vec::new();
        while self.entries.len() > policy.max_artifacts as usize
            || self.total_bytes()? > policy.max_bytes
        {
            let oldest = self
                .entries
                .iter()
                .filter(|(candidate, _)| candidate.as_str() != key)
                .min_by_key(|(key, entry)| (entry.last_accessed_at_unix_ms, key.as_str()))
                .map(|(key, _)| key.clone())
                .ok_or(Phase1InboundMediaError::CorruptState)?;
            let removed = self
                .entries
                .remove(&oldest)
                .ok_or(Phase1InboundMediaError::CorruptState)?;
            evicted.push(removed.artifact_id);
        }
        Ok(evicted)
    }

    pub fn contains(&self, receipt: &Phase1VerifiedMediaReceipt) -> bool {
        self.schema_version == MEDIA_CACHE_SCHEMA_VERSION
            && self.configuration == Some(receipt.configuration)
            && self
                .entries
                .get(&receipt.artifact_id.to_hex())
                .is_some_and(|entry| entry.matches(receipt))
    }

    pub fn touch(
        &mut self,
        artifact_id: Phase1MediaArtifactId,
        observed_at_unix_ms: u64,
    ) -> Result<bool, Phase1InboundMediaError> {
        if observed_at_unix_ms == 0 {
            return Err(Phase1InboundMediaError::InvalidCacheObservation);
        }
        let Some(entry) = self.entries.get_mut(&artifact_id.to_hex()) else {
            return Ok(false);
        };
        entry.last_accessed_at_unix_ms = entry.last_accessed_at_unix_ms.max(observed_at_unix_ms);
        Ok(true)
    }

    pub fn invalidate_artifact(&mut self, artifact_id: Phase1MediaArtifactId) -> bool {
        self.entries.remove(&artifact_id.to_hex()).is_some()
    }

    pub fn invalidate_configuration(
        &mut self,
        configuration: Phase1MediaConfigurationFingerprint,
    ) -> Vec<Phase1MediaArtifactId> {
        if self
            .configuration
            .is_none_or(|current| current == configuration)
        {
            self.configuration = Some(configuration);
            return Vec::new();
        }
        let removed = self
            .entries
            .values()
            .map(|entry| entry.artifact_id)
            .collect();
        self.entries.clear();
        self.configuration = Some(configuration);
        removed
    }

    pub fn artifact_count(&self) -> u32 {
        self.entries.len().try_into().unwrap_or(u32::MAX)
    }

    pub fn total_bytes(&self) -> Result<u64, Phase1InboundMediaError> {
        self.entries.values().try_fold(0_u64, |total, entry| {
            total
                .checked_add(entry.byte_size)
                .ok_or(Phase1InboundMediaError::CorruptState)
        })
    }

    fn validate(&self) -> Result<(), Phase1InboundMediaError> {
        if self.schema_version != MEDIA_CACHE_SCHEMA_VERSION
            || (self.configuration.is_none() && !self.entries.is_empty())
            || self.entries.iter().any(|(key, entry)| {
                key != &entry.artifact_id.to_hex()
                    || key != &hex::encode(entry.artifact_id.as_bytes())
                    || entry.byte_size == 0
                    || entry.cached_at_unix_ms == 0
                    || entry.last_accessed_at_unix_ms < entry.cached_at_unix_ms
                    || !canonical_media_type(&entry.media_type)
                    || entry.extension.is_empty()
                    || validate_dimensions(Some(entry.width), Some(entry.height)).is_err()
            })
        {
            return Err(Phase1InboundMediaError::CorruptState);
        }
        if self
            .configuration
            .is_some_and(|value| value.validate().is_err())
        {
            return Err(Phase1InboundMediaError::CorruptState);
        }
        self.total_bytes().map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase1MediaCacheStatus {
    pub artifacts: u32,
    pub bytes: u64,
    pub configuration: Option<Phase1MediaConfigurationFingerprint>,
}

impl Phase1MediaCacheIndex {
    pub fn status(&self) -> Result<Phase1MediaCacheStatus, Phase1InboundMediaError> {
        self.validate()?;
        Ok(Phase1MediaCacheStatus {
            artifacts: self.artifact_count(),
            bytes: self.total_bytes()?,
            configuration: self.configuration,
        })
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum Phase1InboundMediaError {
    #[error("inbound media reference is invalid")]
    InvalidReference,
    #[error("inbound media digest is invalid")]
    InvalidDigest,
    #[error("inbound media reference requires a digest")]
    MissingDigest,
    #[error("inbound media type is invalid")]
    InvalidMediaType,
    #[error("inbound media dimensions are invalid")]
    InvalidDimensions,
    #[error("inbound media byte size is invalid")]
    InvalidByteSize,
    #[error("inbound media alternative text is invalid")]
    InvalidAlt,
    #[error("inbound media metadata does not match verified bytes")]
    MetadataMismatch,
    #[error("inbound media operation is invalid")]
    InvalidOperation,
    #[error("inbound media operation identity does not match")]
    OperationMismatch,
    #[error("inbound media failure evidence is invalid")]
    InvalidFailure,
    #[error("inbound media configuration is invalid")]
    InvalidConfiguration,
    #[error("inbound media configuration changed")]
    ConfigurationMismatch,
    #[error("inbound media verification time is invalid")]
    InvalidVerificationTime,
    #[error("inbound media cache policy is invalid")]
    InvalidCachePolicy,
    #[error("inbound media cache observation is invalid")]
    InvalidCacheObservation,
    #[error("inbound media artifact exceeds cache quota")]
    CacheQuotaExceeded,
    #[error("inbound media artifact identity collides with different metadata")]
    ArtifactCollision,
    #[error("inbound media receipt is corrupt")]
    CorruptReceipt,
    #[error("inbound media state is corrupt")]
    CorruptState,
    #[error("inbound media schema version is unsupported")]
    UnsupportedSchema,
    #[error("inbound media cache directory is unavailable")]
    CacheUnavailable,
    #[error("inbound media cache filesystem operation failed")]
    CacheIo,
    #[error("inbound media cache artifact is corrupt")]
    CorruptArtifact,
}

#[cfg(feature = "mobile-social")]
pub(crate) async fn write_verified_artifact(
    directory: &Path,
    receipt: &Phase1VerifiedMediaReceipt,
    bytes: &[u8],
) -> Result<Phase1LocalMediaArtifact, Phase1InboundMediaError> {
    receipt.validate_intrinsic()?;
    if bytes.len() as u64 != receipt.byte_size
        || Sha256::digest(bytes).to_hex() != receipt.observed_sha256
    {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    ensure_cache_directory(directory).await?;
    let final_path = artifact_path(directory, receipt.artifact_id, receipt.extension.as_str())?;
    match tokio::fs::symlink_metadata(&final_path).await {
        Ok(_) => {
            let verified_bytes = verify_artifact_file(&final_path, receipt).await?;
            return Ok(local_artifact(final_path, receipt, verified_bytes));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Phase1InboundMediaError::CacheIo),
    }

    let temporary_path = directory.join(format!(
        ".{}.{}.tmp",
        receipt.artifact_id.to_hex(),
        uuid::Uuid::new_v4().simple()
    ));
    let mut temporary = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)
        .await
        .map_err(|_| Phase1InboundMediaError::CacheIo)?;
    let write_result = async {
        temporary
            .write_all(bytes)
            .await
            .map_err(|_| Phase1InboundMediaError::CacheIo)?;
        temporary
            .flush()
            .await
            .map_err(|_| Phase1InboundMediaError::CacheIo)?;
        temporary
            .sync_all()
            .await
            .map_err(|_| Phase1InboundMediaError::CacheIo)?;
        drop(temporary);
        match tokio::fs::hard_link(&temporary_path, &final_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                verify_artifact_file(&final_path, receipt).await?;
            }
            Err(_) => return Err(Phase1InboundMediaError::CacheIo),
        }
        tokio::fs::remove_file(&temporary_path)
            .await
            .map_err(|_| Phase1InboundMediaError::CacheIo)?;
        sync_cache_directory(directory).await?;
        verify_artifact_file(&final_path, receipt).await
    }
    .await;
    if write_result.is_err() {
        let _ = tokio::fs::remove_file(&temporary_path).await;
    }
    let verified_bytes = write_result?;
    Ok(local_artifact(final_path, receipt, verified_bytes))
}

#[cfg(feature = "mobile-social")]
pub(crate) async fn remove_artifact_files(
    directory: &Path,
    artifact_id: Phase1MediaArtifactId,
) -> Result<(), Phase1InboundMediaError> {
    ensure_cache_directory(directory).await?;
    for extension in MEDIA_CACHE_EXTENSIONS {
        let path = artifact_path(directory, artifact_id, extension)?;
        match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(Phase1InboundMediaError::CorruptArtifact);
            }
            Ok(_) => tokio::fs::remove_file(path)
                .await
                .map_err(|_| Phase1InboundMediaError::CacheIo)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Phase1InboundMediaError::CacheIo),
        }
    }
    sync_cache_directory(directory).await
}

#[cfg(feature = "mobile-social")]
pub(crate) async fn verified_artifact(
    directory: &Path,
    receipt: &Phase1VerifiedMediaReceipt,
) -> Result<Phase1LocalMediaArtifact, Phase1InboundMediaError> {
    receipt.validate_intrinsic()?;
    ensure_cache_directory(directory).await?;
    let path = artifact_path(directory, receipt.artifact_id, receipt.extension.as_str())?;
    let verified_bytes = verify_artifact_file(&path, receipt).await?;
    Ok(local_artifact(path, receipt, verified_bytes))
}

#[cfg(feature = "mobile-social")]
async fn ensure_cache_directory(directory: &Path) -> Result<(), Phase1InboundMediaError> {
    match tokio::fs::create_dir(directory).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(Phase1InboundMediaError::CacheIo),
    }
    let metadata = tokio::fs::symlink_metadata(directory)
        .await
        .map_err(|_| Phase1InboundMediaError::CacheIo)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(())
}

#[cfg(feature = "mobile-social")]
fn artifact_path(
    directory: &Path,
    artifact_id: Phase1MediaArtifactId,
    extension: &str,
) -> Result<PathBuf, Phase1InboundMediaError> {
    if !MEDIA_CACHE_EXTENSIONS.contains(&extension) {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(directory.join(format!("{}.{}", artifact_id.to_hex(), extension)))
}

#[cfg(feature = "mobile-social")]
async fn verify_artifact_file(
    path: &Path,
    receipt: &Phase1VerifiedMediaReceipt,
) -> Result<Vec<u8>, Phase1InboundMediaError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| Phase1InboundMediaError::CorruptArtifact)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != receipt.byte_size
    {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| Phase1InboundMediaError::CacheIo)?;
    if Sha256::digest(bytes.as_slice()).to_hex() != receipt.observed_sha256 {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(bytes)
}

#[cfg(feature = "mobile-social")]
async fn sync_cache_directory(directory: &Path) -> Result<(), Phase1InboundMediaError> {
    let directory = directory.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
        .await
        .map_err(|_| Phase1InboundMediaError::CacheIo)?
        .map_err(|_| Phase1InboundMediaError::CacheIo)
}

#[cfg(feature = "mobile-social")]
fn local_artifact(
    local_path: PathBuf,
    receipt: &Phase1VerifiedMediaReceipt,
    bytes: Vec<u8>,
) -> Phase1LocalMediaArtifact {
    Phase1LocalMediaArtifact {
        artifact_id: receipt.artifact_id,
        local_path,
        bytes,
        byte_size: receipt.byte_size,
        media_type: receipt.media_type.clone(),
        width: receipt.width,
        height: receipt.height,
    }
}

fn validate_url_text(value: &str) -> Result<(), Phase1InboundMediaError> {
    if value.is_empty()
        || value.len() > MEDIA_URL_MAX_BYTES
        || value.chars().any(|character| character.is_control())
    {
        return Err(Phase1InboundMediaError::InvalidReference);
    }
    Ok(())
}

fn canonical_media_type(value: &str) -> bool {
    MediaType::parse(value).is_ok_and(|parsed| parsed.to_string() == value)
}

fn canonical_extension(media_type: &MediaType) -> Option<&'static str> {
    match media_type.as_str() {
        "image/gif" => Some("gif"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn validate_dimensions(
    width: Option<u32>,
    height: Option<u32>,
) -> Result<(), Phase1InboundMediaError> {
    match (width, height) {
        (None, None) => Ok(()),
        (Some(width), Some(height))
            if width != 0
                && height != 0
                && width <= MEDIA_DIMENSION_MAX_EDGE
                && height <= MEDIA_DIMENSION_MAX_EDGE
                && u64::from(width) * u64::from(height) <= MEDIA_DIMENSION_MAX_PIXELS =>
        {
            Ok(())
        }
        _ => Err(Phase1InboundMediaError::InvalidDimensions),
    }
}

fn update_optional(digest: &mut Sha256Hasher, value: Option<&str>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        None => digest.update([0]),
    }
}

fn update_optional_u64(digest: &mut Sha256Hasher, value: Option<u64>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update(value.to_be_bytes());
        }
        None => digest.update([0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type ReceiptMutation = Box<dyn Fn(&mut Phase1VerifiedMediaReceipt)>;
    type CacheMutation = Box<dyn Fn(&mut Phase1MediaCacheIndex)>;

    const HASH: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn reference(alt: Option<&str>) -> Phase1StructuralMediaReference {
        Phase1StructuralMediaReference::new(
            format!("https://media.example/{HASH}.jpg"),
            Some(HASH.to_owned()),
            Some("image/jpeg".to_owned()),
            Some(2),
            Some(3),
            Some(5),
            alt.map(str::to_owned),
        )
        .expect("reference")
    }

    fn configuration(value: u8) -> Phase1MediaConfigurationFingerprint {
        Phase1MediaConfigurationFingerprint::new([value; 32]).expect("configuration")
    }

    fn receipt(
        reference: &Phase1StructuralMediaReference,
        configuration: Phase1MediaConfigurationFingerprint,
    ) -> Phase1VerifiedMediaReceipt {
        let commitment =
            ByteCommitment::from_bytes(b"hello", MediaType::parse("image/jpeg").unwrap());
        Phase1VerifiedMediaReceipt::from_commitment(
            reference,
            BlobUrl::parse(&format!("https://cdn.example/{HASH}.jpg")).unwrap(),
            &commitment,
            2,
            3,
            configuration,
            10,
        )
        .expect("receipt")
    }

    #[test]
    fn structural_reference_is_canonical_and_metadata_sensitive() {
        let first = reference(Some("Harvest"));
        let second = reference(Some("Harvest detail"));
        assert_ne!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.expected_sha256(), Some(HASH));
        assert!(
            Phase1StructuralMediaReference::new(
                format!("https://media.example/{}.jpg", "a".repeat(64)),
                Some(HASH.to_owned()),
                Some("image/jpeg".to_owned()),
                Some(2),
                Some(3),
                Some(5),
                None,
            )
            .is_err()
        );
        let interoperable = Phase1StructuralMediaReference::new(
            "https://cdn.example/harvest.jpg",
            Some(HASH.to_owned()),
            Some("image/jpeg".to_owned()),
            Some(2),
            Some(3),
            Some(5),
            None,
        )
        .expect("non-Blossom NIP-92 reference remains structural");
        assert!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &interoperable,
                BlobUrl::parse(&format!("https://cdn.example/{HASH}.jpg")).unwrap(),
                &ByteCommitment::from_bytes(b"hello", MediaType::parse("image/jpeg").unwrap()),
                2,
                3,
                configuration(1),
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn structural_reference_rejects_each_noncanonical_field_independently() {
        for source_url in [
            "",
            "ftp://media.example/file.jpg",
            "https:///file.jpg",
            "https://user@media.example/file.jpg",
            "https://user:password@media.example/file.jpg",
            "https://media.example/file.jpg\n",
        ] {
            assert!(
                Phase1StructuralMediaReference::new(
                    source_url,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .is_err(),
                "source URL should fail: {source_url:?}"
            );
        }
        assert!(
            Phase1StructuralMediaReference::new(
                format!("https://media.example/{}", "x".repeat(MEDIA_URL_MAX_BYTES)),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .is_err()
        );
        for digest in ["not-hex".to_owned(), HASH.to_ascii_uppercase()] {
            assert!(
                Phase1StructuralMediaReference::new(
                    "https://media.example/file.jpg",
                    Some(digest),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .is_err()
            );
        }
        assert!(
            Phase1StructuralMediaReference::new(
                "https://media.example/file.jpg",
                Some(HASH.to_owned()),
                Some("not a media type".to_owned()),
                None,
                None,
                None,
                None,
            )
            .is_err()
        );
        for (width, height) in [
            (Some(1), None),
            (None, Some(1)),
            (Some(0), Some(1)),
            (Some(1), Some(0)),
            (Some(MEDIA_DIMENSION_MAX_EDGE + 1), Some(1)),
            (Some(1), Some(MEDIA_DIMENSION_MAX_EDGE + 1)),
            (Some(10_001), Some(10_000)),
        ] {
            assert!(
                Phase1StructuralMediaReference::new(
                    "https://media.example/file.jpg",
                    Some(HASH.to_owned()),
                    Some("image/jpeg".to_owned()),
                    width,
                    height,
                    Some(5),
                    None,
                )
                .is_err()
            );
        }
        for alt in [
            "x".repeat(MEDIA_ALT_MAX_BYTES + 1),
            "line\nbreak".to_owned(),
        ] {
            assert!(
                Phase1StructuralMediaReference::new(
                    "https://media.example/file.jpg",
                    Some(HASH.to_owned()),
                    Some("image/jpeg".to_owned()),
                    None,
                    None,
                    Some(5),
                    Some(alt),
                )
                .is_err()
            );
        }
        assert!(
            Phase1StructuralMediaReference::new(
                "https://media.example/file.jpg",
                Some(HASH.to_owned()),
                Some("image/jpeg".to_owned()),
                None,
                None,
                Some(0),
                None,
            )
            .is_err()
        );

        let mut unsupported = reference(None);
        unsupported.schema_version = 2;
        assert_eq!(
            unsupported.validate(),
            Err(Phase1InboundMediaError::UnsupportedSchema)
        );
        let mut corrupt = reference(None);
        corrupt.fingerprint = [9; 32];
        assert_eq!(
            corrupt.validate(),
            Err(Phase1InboundMediaError::CorruptState)
        );
    }

    #[test]
    fn operation_and_failure_evidence_reject_each_invalid_field() {
        assert!(Phase1MediaConfigurationFingerprint::new([0; 32]).is_err());
        assert!(Phase1MediaConfigurationFingerprint::parse("not-hex").is_err());
        assert!(Phase1MediaConfigurationFingerprint::parse("00").is_err());
        assert!(Phase1MediaArtifactId::parse("not-hex").is_err());

        assert!(Phase1InboundMediaPending::new([0; 16], configuration(1), 1).is_err());
        assert!(Phase1InboundMediaPending::new([1; 16], configuration(1), 0).is_err());
        assert!(
            Phase1InboundMediaPending::new(
                [1; 16],
                Phase1MediaConfigurationFingerprint([0; 32]),
                1,
            )
            .is_err()
        );

        for (operation_id, code, failed_at) in [
            ([0; 16], "failed".to_owned(), 1),
            ([1; 16], "failed".to_owned(), 0),
            ([1; 16], String::new(), 1),
            ([1; 16], "x".repeat(MEDIA_FAILURE_CODE_MAX_BYTES + 1), 1),
            ([1; 16], "Not_Safe".to_owned(), 1),
        ] {
            assert!(Phase1InboundMediaFailure::new(operation_id, code, true, failed_at).is_err());
        }
        let evidence = Phase1InboundMediaFailure::new([2; 16], "retry_2", true, 9).unwrap();
        assert_eq!(evidence.safe_code(), "retry_2");
        assert!(evidence.retryable());
    }

    #[test]
    fn media_helper_vocabularies_cover_every_closed_outcome() {
        assert!(validate_url_text("https://example.test/media").is_ok());
        assert_eq!(
            validate_url_text(""),
            Err(Phase1InboundMediaError::InvalidReference)
        );
        assert_eq!(
            validate_url_text(&"x".repeat(MEDIA_URL_MAX_BYTES + 1)),
            Err(Phase1InboundMediaError::InvalidReference)
        );
        assert_eq!(
            validate_url_text("bad\nurl"),
            Err(Phase1InboundMediaError::InvalidReference)
        );
        assert!(canonical_media_type("image/jpeg"));
        assert!(!canonical_media_type("IMAGE/JPEG"));
        assert_eq!(
            canonical_extension(&MediaType::parse("image/gif").unwrap()),
            Some("gif")
        );
        assert_eq!(
            canonical_extension(&MediaType::parse("image/jpeg").unwrap()),
            Some("jpg")
        );
        assert_eq!(
            canonical_extension(&MediaType::parse("image/png").unwrap()),
            Some("png")
        );
        assert_eq!(
            canonical_extension(&MediaType::parse("image/webp").unwrap()),
            Some("webp")
        );
        assert_eq!(
            canonical_extension(&MediaType::parse("image/svg+xml").unwrap()),
            None
        );
        assert!(validate_dimensions(None, None).is_ok());
        assert!(validate_dimensions(Some(10_000), Some(10_000)).is_ok());
    }

    #[test]
    fn verified_state_requires_matching_operation_bytes_and_configuration() {
        let structural = reference(None);
        let mut media = MediaReference::new(structural.clone()).unwrap();
        let pending = Phase1InboundMediaPending::new([7; 16], configuration(3), 9).unwrap();
        media.begin(pending).unwrap();
        assert_eq!(
            media.verify([8; 16], receipt(&structural, configuration(3))),
            Err(Phase1InboundMediaError::OperationMismatch)
        );
        media
            .verify([7; 16], receipt(&structural, configuration(3)))
            .unwrap();
        assert!(matches!(
            media.retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
        media
            .restore(
                Phase1InboundMediaState::Unavailable,
                &Phase1MediaCacheIndex::default(),
            )
            .unwrap();
        assert!(matches!(
            media.retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
    }

    #[test]
    fn media_state_transitions_and_renderability_cover_every_state() {
        let structural = reference(None);
        let config = configuration(3);
        let verified = receipt(&structural, config);
        let pending = Phase1InboundMediaPending::new([7; 16], config, 9).unwrap();
        let failure = Phase1InboundMediaFailure::new([7; 16], "network", true, 10).unwrap();
        let wrong_failure = Phase1InboundMediaFailure::new([8; 16], "network", true, 10).unwrap();

        let mut media = MediaReference::new(structural.clone()).unwrap();
        assert_eq!(media.invalidate(), None);
        assert!(!media.is_renderable_with(&Phase1MediaCacheIndex::default(), config));
        assert_eq!(
            media.fail(failure.clone()),
            Err(Phase1InboundMediaError::OperationMismatch)
        );
        assert_eq!(
            media.verify([7; 16], verified.clone()),
            Err(Phase1InboundMediaError::OperationMismatch)
        );

        media.begin(pending.clone()).unwrap();
        assert_eq!(
            media.fail(wrong_failure),
            Err(Phase1InboundMediaError::OperationMismatch)
        );
        assert_eq!(
            media.verify([7; 16], receipt(&structural, configuration(4))),
            Err(Phase1InboundMediaError::OperationMismatch)
        );
        media.fail(failure).unwrap();
        assert!(matches!(
            media.retrieval(),
            Phase1InboundMediaState::Failed(_)
        ));
        assert!(media.validate().is_ok());

        media.begin(pending).unwrap();
        media.verify([7; 16], verified.clone()).unwrap();
        assert!(!media.is_renderable_with(&Phase1MediaCacheIndex::default(), config));
        let mut cache = Phase1MediaCacheIndex::default();
        cache
            .admit(&verified, Phase1MediaCachePolicy::new(5, 1).unwrap(), 10)
            .unwrap();
        assert!(media.is_renderable_with(&cache, config));
        assert!(!media.is_renderable_with(&cache, configuration(4)));
        assert_eq!(media.invalidate(), Some(verified.artifact_id()));

        media
            .restore(
                Phase1InboundMediaState::Verified(Box::new(verified.clone())),
                &Phase1MediaCacheIndex::default(),
            )
            .unwrap();
        assert!(matches!(
            media.retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
        media
            .restore(
                Phase1InboundMediaState::Verified(Box::new(verified)),
                &cache,
            )
            .unwrap();
        assert!(matches!(
            media.retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
    }

    #[test]
    fn receipt_rejects_hash_size_type_and_dimension_mismatch() {
        let expected = reference(None);
        let wrong_bytes =
            ByteCommitment::from_bytes(b"other", MediaType::parse("image/jpeg").unwrap());
        assert!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &expected,
                BlobUrl::parse(&format!("https://cdn.example/{}.jpg", wrong_bytes.sha256()))
                    .unwrap(),
                &wrong_bytes,
                2,
                3,
                configuration(1),
                1,
            )
            .is_err()
        );
        let commitment =
            ByteCommitment::from_bytes(b"hello", MediaType::parse("image/png").unwrap());
        assert!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &expected,
                BlobUrl::parse(&format!("https://cdn.example/{HASH}.jpg")).unwrap(),
                &commitment,
                2,
                3,
                configuration(1),
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn receipt_validation_rejects_each_bound_field_independently() {
        let expected = reference(None);
        let commitment =
            ByteCommitment::from_bytes(b"hello", MediaType::parse("image/jpeg").unwrap());
        let final_url = BlobUrl::parse(&format!("https://cdn.example/{HASH}.jpg")).unwrap();
        assert!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &expected,
                final_url.clone(),
                &commitment,
                2,
                3,
                configuration(1),
                0,
            )
            .is_err()
        );

        let mutations: [fn(&mut Phase1StructuralMediaReference); 4] = [
            |value: &mut Phase1StructuralMediaReference| value.expected_byte_size = Some(6),
            |value: &mut Phase1StructuralMediaReference| {
                value.expected_media_type = Some("image/png".to_owned())
            },
            |value: &mut Phase1StructuralMediaReference| value.expected_width = Some(3),
            |value: &mut Phase1StructuralMediaReference| value.expected_height = Some(4),
        ];
        for mutate in mutations {
            let mut changed = expected.clone();
            mutate(&mut changed);
            changed.fingerprint = changed.derive_fingerprint();
            assert!(
                Phase1VerifiedMediaReceipt::from_commitment(
                    &changed,
                    final_url.clone(),
                    &commitment,
                    2,
                    3,
                    configuration(1),
                    1,
                )
                .is_err()
            );
        }

        let mut without_digest = Phase1StructuralMediaReference::new(
            "https://media.example/file.jpg",
            None,
            Some("image/jpeg".to_owned()),
            Some(2),
            Some(3),
            Some(5),
            None,
        )
        .unwrap();
        without_digest.expected_sha256 = None;
        without_digest.fingerprint = without_digest.derive_fingerprint();
        assert!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &without_digest,
                final_url,
                &commitment,
                2,
                3,
                configuration(1),
                1,
            )
            .is_err()
        );

        let valid = receipt(&expected, configuration(1));
        let mut corruptions: Vec<ReceiptMutation> = vec![
            Box::new(|value| value.schema_version = 2),
            Box::new(|value| value.expected_sha256 = "a".repeat(64)),
            Box::new(|value| value.artifact_id = Phase1MediaArtifactId([8; 32])),
            Box::new(|value| value.byte_size = 0),
            Box::new(|value| value.verified_at_unix_ms = 0),
            Box::new(|value| value.configuration = Phase1MediaConfigurationFingerprint([0; 32])),
            Box::new(|value| value.canonical_final_url = "not a url".to_owned()),
            Box::new(|value| value.canonical_final_url.push_str("?changed=1")),
            Box::new(|value| value.extension = "png".to_owned()),
            Box::new(|value| value.media_type = "not a media type".to_owned()),
            Box::new(|value| value.media_type = "image/svg+xml".to_owned()),
            Box::new(|value| value.width = 0),
        ];
        for mutate in corruptions.drain(..) {
            let mut changed = valid.clone();
            mutate(&mut changed);
            assert_eq!(
                changed.validate_intrinsic(),
                Err(Phase1InboundMediaError::CorruptReceipt)
            );
        }
        assert_eq!(valid.artifact_id().to_hex(), HASH);
        assert_eq!(valid.configuration(), configuration(1));
        assert_eq!(
            valid.canonical_final_url(),
            format!("https://cdn.example/{HASH}.jpg")
        );
        assert_eq!(valid.observed_sha256(), HASH);
        assert_eq!(valid.byte_size(), 5);
        assert_eq!(valid.media_type(), "image/jpeg");
        assert_eq!(valid.extension(), "jpg");
        assert_eq!(
            (valid.width(), valid.height(), valid.verified_at_unix_ms()),
            (2, 3, 10)
        );

        let wrong_extension = BlobUrl::parse(&format!("https://cdn.example/{HASH}.png")).unwrap();
        assert_eq!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &expected,
                wrong_extension,
                &commitment,
                2,
                3,
                configuration(1),
                1,
            ),
            Err(Phase1InboundMediaError::MetadataMismatch)
        );
        let wrong_path_hash = "a".repeat(64);
        assert_eq!(
            Phase1VerifiedMediaReceipt::from_commitment(
                &expected,
                BlobUrl::parse(&format!("https://cdn.example/{wrong_path_hash}.jpg")).unwrap(),
                &commitment,
                2,
                3,
                configuration(1),
                1,
            ),
            Err(Phase1InboundMediaError::MetadataMismatch)
        );

        let reference_mutations: [fn(&mut Phase1StructuralMediaReference); 7] = [
            |value| value.fingerprint = [8; 32],
            |value| value.source_url = "https://other.example/file.jpg".to_owned(),
            |value| value.expected_sha256 = Some("a".repeat(64)),
            |value| value.expected_byte_size = Some(6),
            |value| value.expected_media_type = Some("image/png".to_owned()),
            |value| value.expected_width = Some(3),
            |value| value.expected_height = Some(4),
        ];
        for mutate in reference_mutations {
            let mut changed = expected.clone();
            mutate(&mut changed);
            assert_eq!(
                valid.validate(&changed),
                Err(Phase1InboundMediaError::CorruptReceipt)
            );
        }
    }

    #[test]
    fn cache_validation_rejects_each_invalid_field_independently() {
        assert!(Phase1MediaCachePolicy::new(0, 1).is_err());
        assert!(Phase1MediaCachePolicy::new(1, 0).is_err());
        let policy = Phase1MediaCachePolicy::default();
        assert_eq!(policy.max_bytes(), 256 * 1024 * 1024);
        assert_eq!(policy.max_artifacts(), 2_000);

        let structural = reference(None);
        let verified = receipt(&structural, configuration(4));
        assert!(Phase1MediaCacheEntry::from_receipt(&verified, 9).is_err());
        let mut cache = Phase1MediaCacheIndex::default();
        assert!(!cache.touch(verified.artifact_id(), 1).unwrap());
        assert!(cache.touch(verified.artifact_id(), 0).is_err());
        assert!(!cache.invalidate_artifact(verified.artifact_id()));
        assert!(cache.invalidate_configuration(configuration(4)).is_empty());
        cache
            .admit(&verified, Phase1MediaCachePolicy::new(5, 1).unwrap(), 10)
            .unwrap();
        assert!(cache.touch(verified.artifact_id(), 11).unwrap());
        assert!(cache.invalidate_artifact(verified.artifact_id()));

        let mut baseline = Phase1MediaCacheIndex::default();
        baseline
            .admit(&verified, Phase1MediaCachePolicy::new(5, 1).unwrap(), 10)
            .unwrap();
        let key = verified.artifact_id().to_hex();
        let mutations: Vec<CacheMutation> = vec![
            Box::new(|value| value.schema_version = 2),
            Box::new(|value| value.configuration = None),
            Box::new({
                let key = key.clone();
                move |value| value.entries.get_mut(&key).unwrap().byte_size = 0
            }),
            Box::new({
                let key = key.clone();
                move |value| value.entries.get_mut(&key).unwrap().cached_at_unix_ms = 0
            }),
            Box::new({
                let key = key.clone();
                move |value| {
                    value
                        .entries
                        .get_mut(&key)
                        .unwrap()
                        .last_accessed_at_unix_ms = 9
                }
            }),
            Box::new({
                let key = key.clone();
                move |value| value.entries.get_mut(&key).unwrap().media_type = "bad".to_owned()
            }),
            Box::new({
                let key = key.clone();
                move |value| value.entries.get_mut(&key).unwrap().extension.clear()
            }),
            Box::new({
                let key = key.clone();
                move |value| value.entries.get_mut(&key).unwrap().width = 0
            }),
            Box::new(|value| {
                let entry = value.entries.pop_first().unwrap().1;
                value.entries.insert("wrong-key".to_owned(), entry);
            }),
            Box::new(|value| {
                value.configuration = Some(Phase1MediaConfigurationFingerprint([0; 32]));
            }),
        ];
        for mutate in mutations {
            let mut changed = baseline.clone();
            mutate(&mut changed);
            assert_eq!(changed.status(), Err(Phase1InboundMediaError::CorruptState));
        }
        assert_eq!(
            baseline.admit(&verified, Phase1MediaCachePolicy::new(4, 1).unwrap(), 10),
            Err(Phase1InboundMediaError::CacheQuotaExceeded)
        );

        let mut wrong_schema = baseline.clone();
        wrong_schema.schema_version = 2;
        assert!(!wrong_schema.contains(&verified));
        let mut wrong_configuration = baseline.clone();
        wrong_configuration.configuration = Some(configuration(5));
        assert!(!wrong_configuration.contains(&verified));
        let mut missing = baseline.clone();
        missing.entries.clear();
        assert!(!missing.contains(&verified));
        let mut mismatched = baseline.clone();
        mismatched.entries.get_mut(&key).unwrap().height = 4;
        assert!(!mismatched.contains(&verified));

        let receipt_mutations: [fn(&mut Phase1VerifiedMediaReceipt); 6] = [
            |value| value.artifact_id = Phase1MediaArtifactId([8; 32]),
            |value| value.byte_size = 6,
            |value| value.media_type = "image/png".to_owned(),
            |value| value.extension = "png".to_owned(),
            |value| value.width = 3,
            |value| value.height = 4,
        ];
        let entry = baseline.entries.get(&key).unwrap();
        for mutate in receipt_mutations {
            let mut changed = verified.clone();
            mutate(&mut changed);
            assert!(!entry.matches(&changed));
        }

        let mut collision = baseline.clone();
        collision.entries.get_mut(&key).unwrap().byte_size = 4;
        assert_eq!(
            collision.admit(&verified, Phase1MediaCachePolicy::new(10, 2).unwrap(), 10),
            Err(Phase1InboundMediaError::ArtifactCollision)
        );

        let second_hash = Sha256::digest(b"world").to_hex();
        let second_reference = Phase1StructuralMediaReference::new(
            format!("https://media.example/{second_hash}.jpg"),
            Some(second_hash.clone()),
            Some("image/jpeg".to_owned()),
            Some(2),
            Some(3),
            Some(5),
            None,
        )
        .unwrap();
        let second = Phase1VerifiedMediaReceipt::from_commitment(
            &second_reference,
            BlobUrl::parse(&format!("https://cdn.example/{second_hash}.jpg")).unwrap(),
            &ByteCommitment::from_bytes(b"world", MediaType::parse("image/jpeg").unwrap()),
            2,
            3,
            configuration(4),
            11,
        )
        .unwrap();
        let mut byte_limited = baseline.clone();
        assert_eq!(
            byte_limited
                .admit(&second, Phase1MediaCachePolicy::new(8, 2).unwrap(), 11)
                .unwrap(),
            vec![verified.artifact_id()]
        );

        let mut overflow = Phase1MediaCacheIndex {
            configuration: Some(configuration(4)),
            ..Phase1MediaCacheIndex::default()
        };
        let mut first_entry = Phase1MediaCacheEntry::from_receipt(&verified, 10).unwrap();
        first_entry.byte_size = u64::MAX;
        overflow.entries.insert(key, first_entry);
        let second_key = second.artifact_id().to_hex();
        overflow.entries.insert(
            second_key,
            Phase1MediaCacheEntry::from_receipt(&second, 11).unwrap(),
        );
        assert_eq!(
            overflow.total_bytes(),
            Err(Phase1InboundMediaError::CorruptState)
        );
    }

    #[test]
    fn cache_is_content_addressed_bounded_lru_and_configuration_scoped() {
        let config = configuration(4);
        let first_reference = reference(None);
        let first = receipt(&first_reference, config);
        let second_hash = Sha256::digest(b"world").to_hex();
        let second_reference = Phase1StructuralMediaReference::new(
            format!("https://media.example/{second_hash}.jpg"),
            Some(second_hash.clone()),
            Some("image/jpeg".to_owned()),
            Some(2),
            Some(3),
            Some(5),
            None,
        )
        .unwrap();
        let second_commitment =
            ByteCommitment::from_bytes(b"world", MediaType::parse("image/jpeg").unwrap());
        let second = Phase1VerifiedMediaReceipt::from_commitment(
            &second_reference,
            BlobUrl::parse(&format!("https://cdn.example/{second_hash}.jpg")).unwrap(),
            &second_commitment,
            2,
            3,
            config,
            11,
        )
        .unwrap();
        let mut cache = Phase1MediaCacheIndex::default();
        let policy = Phase1MediaCachePolicy::new(5, 1).unwrap();
        assert!(cache.admit(&first, policy, 10).unwrap().is_empty());
        let evicted = cache.admit(&second, policy, 11).unwrap();
        assert_eq!(evicted, vec![first.artifact_id()]);
        assert!(!cache.contains(&first));
        assert!(cache.contains(&second));
        assert_eq!(cache.status().unwrap().artifacts, 1);
        assert_eq!(
            cache.admit(&second, policy, 12),
            Ok(Vec::new()),
            "idempotent cache admission remains bounded"
        );
        assert_eq!(
            cache.admit(&receipt(&first_reference, configuration(5)), policy, 13,),
            Err(Phase1InboundMediaError::ConfigurationMismatch)
        );
        assert_eq!(
            cache.invalidate_configuration(configuration(5)),
            vec![second.artifact_id()]
        );
        assert_eq!(cache.status().unwrap().artifacts, 0);
    }

    #[test]
    fn persisted_receipt_and_cache_tamper_fail_closed() {
        let structural = reference(None);
        let config = configuration(7);
        let mut media = MediaReference::new(structural.clone()).unwrap();
        media
            .begin(Phase1InboundMediaPending::new([8; 16], config, 1).unwrap())
            .unwrap();
        let verified = receipt(&structural, config);
        media.verify([8; 16], verified.clone()).unwrap();
        let mut media_value = serde_json::to_value(&media).unwrap();
        media_value["retrieval"]["evidence"]["observedSha256"] = serde_json::json!("a".repeat(64));
        let corrupt: MediaReference = serde_json::from_value(media_value).unwrap();
        assert_eq!(
            corrupt.validate(),
            Err(Phase1InboundMediaError::CorruptReceipt)
        );

        let mut cache = Phase1MediaCacheIndex::default();
        cache
            .admit(&verified, Phase1MediaCachePolicy::new(10, 1).unwrap(), 12)
            .unwrap();
        let mut cache_value = serde_json::to_value(cache).unwrap();
        let entry = cache_value["entries"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        entry["byteSize"] = serde_json::json!(0);
        let corrupt: Phase1MediaCacheIndex = serde_json::from_value(cache_value).unwrap();
        assert_eq!(corrupt.status(), Err(Phase1InboundMediaError::CorruptState));
    }

    #[cfg(feature = "mobile-social")]
    #[tokio::test]
    async fn atomic_artifact_writes_converge_and_corruption_fails_closed() {
        let bytes = b"GIF89a\x02\0\x03\0";
        let hash = Sha256::digest(bytes).to_hex();
        let structural = Phase1StructuralMediaReference::new(
            format!("https://media.example/{hash}.gif"),
            Some(hash.clone()),
            Some("image/gif".to_owned()),
            Some(2),
            Some(3),
            Some(bytes.len() as u64),
            None,
        )
        .unwrap();
        let receipt = Phase1VerifiedMediaReceipt::from_commitment(
            &structural,
            BlobUrl::parse(&format!("https://media.example/{hash}.gif")).unwrap(),
            &ByteCommitment::from_bytes(bytes, MediaType::parse("image/gif").unwrap()),
            2,
            3,
            configuration(4),
            1,
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("cache");
        let (left, right) = tokio::join!(
            write_verified_artifact(&directory, &receipt, bytes),
            write_verified_artifact(&directory, &receipt, bytes),
        );
        let left = left.unwrap();
        let right = right.unwrap();
        assert_eq!(left, right);
        assert_eq!(left.bytes(), bytes);
        assert_eq!(tokio::fs::read(left.local_path()).await.unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            1,
            "no temporary file survives a converged write"
        );
        tokio::fs::write(left.local_path(), b"GIF89a\x03\0\x03\0")
            .await
            .unwrap();
        assert_eq!(
            verified_artifact(&directory, &receipt).await,
            Err(Phase1InboundMediaError::CorruptArtifact)
        );
    }
}
