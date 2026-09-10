use radroots_blossom::{MediaType, Sha256};
use serde::{Deserialize, Serialize};

use super::ComposerError;
use crate::runtime::product_surface::{AddCommandType, Phase1DraftEventTiming};

pub const COMPOSER_CONTENT_MAX_BYTES: usize = 65_535;
pub const COMPOSER_TEXT_MAX_BYTES: usize = 1_024;
pub const COMPOSER_MEDIA_MAX: usize = 20;
/// Bounds decoding before JSON allocation and stays below the shared draft payload cap.
pub const COMPOSER_FORM_MAX_BYTES: usize = 1024 * 1024;
const MEDIA_REFERENCE_MAX_BYTES: usize = 256;
const MEDIA_FILE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Untrusted local media metadata. It proves neither byte ownership nor upload readiness.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerMediaInput {
    pub opaque_reference: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    pub prepared_at_unix_s: u64,
}

impl std::fmt::Debug for ComposerMediaInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComposerMediaInput")
            .finish_non_exhaustive()
    }
}

impl ComposerMediaInput {
    fn validate(&self) -> Result<(), ComposerError> {
        let reference = &self.opaque_reference;
        if reference.len() <= "media:".len()
            || reference.len() > MEDIA_REFERENCE_MAX_BYTES
            || !reference.starts_with("media:")
            || !reference["media:".len()..].bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
            || self.sha256.len() != 64
            || self.media_type.len() > 128
            || self.byte_size == 0
            || self.byte_size > MEDIA_FILE_MAX_BYTES
            || self.alt.len() > COMPOSER_TEXT_MAX_BYTES
            || self.prepared_at_unix_s == 0
            || self.prepared_at_unix_s.checked_mul(1000).is_none()
        {
            return Err(ComposerError::InvalidForm);
        }
        Sha256::from_hex(&self.sha256).map_err(|_| ComposerError::InvalidForm)?;
        MediaType::parse(&self.media_type).map_err(|_| ComposerError::InvalidForm)?;
        radroots_sdk::transport::BlossomImageDimensions::new(self.width, self.height)
            .map_err(|_| ComposerError::InvalidForm)?;
        Ok(())
    }
}

/// Untrusted editing input. Validate it before retaining it as a composer form.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerFormInput {
    pub command_type: AddCommandType,
    pub content: String,
    pub identifier: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub event_timing: Option<Phase1DraftEventTiming>,
    pub event_start_date: Option<String>,
    pub event_end_date: Option<String>,
    pub event_start_unix_s: Option<u64>,
    pub event_end_unix_s: Option<u64>,
    pub event_timezone: Option<String>,
    pub price_amount: Option<String>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub quantity: Option<String>,
    pub food_published_at_unix_s: Option<u64>,
    pub food_status: Option<String>,
    pub media: Vec<ComposerMediaInput>,
}

impl std::fmt::Debug for ComposerFormInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComposerFormInput")
            .field("command_type", &self.command_type)
            .field("media_count", &self.media.len())
            .finish_non_exhaustive()
    }
}

impl ComposerFormInput {
    pub fn empty(command_type: AddCommandType) -> Self {
        Self {
            command_type,
            content: String::new(),
            identifier: None,
            title: None,
            summary: None,
            location: None,
            event_timing: None,
            event_start_date: None,
            event_end_date: None,
            event_start_unix_s: None,
            event_end_unix_s: None,
            event_timezone: None,
            price_amount: None,
            currency: None,
            unit: None,
            quantity: None,
            food_published_at_unix_s: None,
            food_status: None,
            media: Vec::new(),
        }
    }

    fn validate(&self) -> Result<(), ComposerError> {
        if self.content.len() > COMPOSER_CONTENT_MAX_BYTES || self.media.len() > COMPOSER_MEDIA_MAX
        {
            return Err(ComposerError::InvalidForm);
        }
        for text in [
            &self.identifier,
            &self.title,
            &self.summary,
            &self.location,
            &self.event_start_date,
            &self.event_end_date,
            &self.event_timezone,
            &self.price_amount,
            &self.currency,
            &self.unit,
            &self.quantity,
            &self.food_status,
        ]
        .into_iter()
        .flatten()
        {
            if text.len() > COMPOSER_TEXT_MAX_BYTES {
                return Err(ComposerError::InvalidForm);
            }
        }
        for media in &self.media {
            media.validate()?;
        }
        Ok(())
    }
}

/// Immutable validated editing data, deliberately not a publishable event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ComposerFormInput", into = "ComposerFormInput")]
pub struct ComposerPartialForm(ComposerFormInput);

impl ComposerPartialForm {
    pub fn new(input: ComposerFormInput) -> Result<Self, ComposerError> {
        input.validate()?;
        Ok(Self(input))
    }

    pub fn input(&self) -> &ComposerFormInput {
        &self.0
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, ComposerError> {
        if bytes.is_empty() || bytes.len() > COMPOSER_FORM_MAX_BYTES {
            return Err(ComposerError::InvalidRepresentation);
        }
        serde_json::from_slice(bytes).map_err(|_| ComposerError::InvalidRepresentation)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, ComposerError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ComposerError::InvalidRepresentation)?;
        if bytes.len() > COMPOSER_FORM_MAX_BYTES {
            return Err(ComposerError::InvalidRepresentation);
        }
        Ok(bytes)
    }
}

impl TryFrom<ComposerFormInput> for ComposerPartialForm {
    type Error = ComposerError;
    fn try_from(value: ComposerFormInput) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ComposerPartialForm> for ComposerFormInput {
    fn from(value: ComposerPartialForm) -> Self {
        value.0
    }
}
