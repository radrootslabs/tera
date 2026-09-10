//! Typed local editing boundary; no publishable plan, byte handle or raw database API.
use crate::{FfiAddCommandType, FfiEventTimingKind, MOBILE_FFI_SCHEMA_VERSION, TeraAppError};
use tera_core::runtime::product_surface::{
    AddCommandType, ComposerDraft, ComposerEditSequence, ComposerError, ComposerFormInput,
    ComposerId, ComposerListEntry, ComposerMediaInput, ComposerPage, ComposerPartialForm,
    ComposerRepairReason, ComposerRevision, ComposerSaveReceipt, ComposerScope, LocalNetworkId,
    Phase1DraftEventTiming,
};

mod error;

/// Reserves only a random editing identity, without writing or acknowledging a save.
#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn composer_reserve_id() -> Result<FfiComposerIdRecord, TeraAppError> {
    Ok(FfiComposerIdRecord {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        id: hex::encode(ComposerId::generate()?.as_bytes()),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerIdRecord {
    pub schema_version: u16,
    pub id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerScopeRecord {
    pub schema_version: u16,
    pub author_public_key: String,
    pub local_network_id: String,
}

impl TryFrom<FfiComposerScopeRecord> for ComposerScope {
    type Error = TeraAppError;
    fn try_from(value: FfiComposerScopeRecord) -> Result<Self, Self::Error> {
        version(value.schema_version)?;
        if value.author_public_key.len() != 64 || !canonical_hex(&value.author_public_key) {
            return Err(TeraAppError::invalid_argument("composer_scope_invalid"));
        }
        let author = value
            .author_public_key
            .parse()
            .map_err(|_| TeraAppError::invalid_argument("composer_scope_invalid"))?;
        let context = LocalNetworkId::new(value.local_network_id)
            .map_err(|_| TeraAppError::invalid_argument("composer_scope_invalid"))?;
        Ok(Self::new(author, context))
    }
}

impl From<&ComposerScope> for FfiComposerScopeRecord {
    fn from(value: &ComposerScope) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            author_public_key: value.author().to_hex(),
            local_network_id: value.local_network().as_str().to_owned(),
        }
    }
}

/// Reference metadata only; neither a file handle nor proof of upload readiness.
#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerMediaRecord {
    pub schema_version: u16,
    pub opaque_reference: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    pub prepared_at_unix_s: u64,
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerFormRecord {
    pub schema_version: u16,
    pub command_type: FfiAddCommandType,
    pub content: String,
    pub identifier: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub event_timing: Option<FfiEventTimingKind>,
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
    pub media: Vec<FfiComposerMediaRecord>,
}

impl TryFrom<FfiComposerFormRecord> for ComposerPartialForm {
    type Error = TeraAppError;
    fn try_from(value: FfiComposerFormRecord) -> Result<Self, Self::Error> {
        version(value.schema_version)?;
        let input = ComposerFormInput {
            command_type: match value.command_type {
                FfiAddCommandType::CreateUpdate => AddCommandType::CreateUpdate,
                FfiAddCommandType::CreatePhotoUpdate => AddCommandType::CreatePhotoUpdate,
                FfiAddCommandType::CreateAsk => AddCommandType::CreateAsk,
                FfiAddCommandType::CreateEvent => AddCommandType::CreateEvent,
                FfiAddCommandType::CreateFoodAvailability => AddCommandType::CreateFoodAvailability,
            },
            content: value.content,
            identifier: value.identifier,
            title: value.title,
            summary: value.summary,
            location: value.location,
            event_timing: value.event_timing.map(|timing| match timing {
                FfiEventTimingKind::AllDay => Phase1DraftEventTiming::AllDay,
                FfiEventTimingKind::Timed => Phase1DraftEventTiming::Timed,
            }),
            event_start_date: value.event_start_date,
            event_end_date: value.event_end_date,
            event_start_unix_s: value.event_start_unix_s,
            event_end_unix_s: value.event_end_unix_s,
            event_timezone: value.event_timezone,
            price_amount: value.price_amount,
            currency: value.currency,
            unit: value.unit,
            quantity: value.quantity,
            food_published_at_unix_s: value.food_published_at_unix_s,
            food_status: value.food_status,
            media: value
                .media
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        };
        Self::new(input).map_err(Into::into)
    }
}

impl TryFrom<FfiComposerMediaRecord> for ComposerMediaInput {
    type Error = TeraAppError;
    fn try_from(value: FfiComposerMediaRecord) -> Result<Self, Self::Error> {
        version(value.schema_version)?;
        Ok(Self {
            opaque_reference: value.opaque_reference,
            sha256: value.sha256,
            media_type: value.media_type,
            byte_size: value.byte_size,
            width: value.width,
            height: value.height,
            alt: value.alt,
            prepared_at_unix_s: value.prepared_at_unix_s,
        })
    }
}

impl From<&ComposerPartialForm> for FfiComposerFormRecord {
    fn from(form: &ComposerPartialForm) -> Self {
        let value = form.input();
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: value.command_type.into(),
            content: value.content.clone(),
            identifier: value.identifier.clone(),
            title: value.title.clone(),
            summary: value.summary.clone(),
            location: value.location.clone(),
            event_timing: value.event_timing.map(|timing| match timing {
                Phase1DraftEventTiming::AllDay => FfiEventTimingKind::AllDay,
                Phase1DraftEventTiming::Timed => FfiEventTimingKind::Timed,
            }),
            event_start_date: value.event_start_date.clone(),
            event_end_date: value.event_end_date.clone(),
            event_start_unix_s: value.event_start_unix_s,
            event_end_unix_s: value.event_end_unix_s,
            event_timezone: value.event_timezone.clone(),
            price_amount: value.price_amount.clone(),
            currency: value.currency.clone(),
            unit: value.unit.clone(),
            quantity: value.quantity.clone(),
            food_published_at_unix_s: value.food_published_at_unix_s,
            food_status: value.food_status.clone(),
            media: value.media.iter().map(Into::into).collect(),
        }
    }
}

impl From<&ComposerMediaInput> for FfiComposerMediaRecord {
    fn from(value: &ComposerMediaInput) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            opaque_reference: value.opaque_reference.clone(),
            sha256: value.sha256.clone(),
            media_type: value.media_type.clone(),
            byte_size: value.byte_size,
            width: value.width,
            height: value.height,
            alt: value.alt.clone(),
            prepared_at_unix_s: value.prepared_at_unix_s,
        }
    }
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerSaveRequest {
    pub schema_version: u16,
    pub scope: FfiComposerScopeRecord,
    pub id: String,
    /// None creates a new composer; Some compares with the durable head.
    pub expected_revision: Option<u64>,
    pub edit_sequence: u64,
    pub form: FfiComposerFormRecord,
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerDraftRecord {
    pub schema_version: u16,
    pub scope: FfiComposerScopeRecord,
    pub id: String,
    pub revision: u64,
    pub edit_sequence: u64,
    pub form: FfiComposerFormRecord,
}

impl From<&ComposerDraft> for FfiComposerDraftRecord {
    fn from(value: &ComposerDraft) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            scope: value.scope().into(),
            id: hex::encode(value.id().as_bytes()),
            revision: value.revision().get(),
            edit_sequence: value.edit_sequence().get(),
            form: value.form().into(),
        }
    }
}

/// A historical owner-committed acknowledgment, not a promise that no later edit exists.
#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerSaveReceipt {
    pub schema_version: u16,
    pub draft: FfiComposerDraftRecord,
    pub replayed: bool,
}

impl From<&ComposerSaveReceipt> for FfiComposerSaveReceipt {
    fn from(value: &ComposerSaveReceipt) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft: value.draft().into(),
            replayed: value.is_replay(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerSummaryRecord {
    pub schema_version: u16,
    pub id: String,
    pub revision: u64,
    pub edit_sequence: u64,
    pub command_type: FfiAddCommandType,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiComposerRepairReason {
    UnsupportedSchema,
    CorruptRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiComposerListEntry {
    Draft {
        summary: FfiComposerSummaryRecord,
    },
    /// An opaque storage locator, including possibly invalid ID bytes. Never an editing ID.
    Repair {
        draft_key: String,
        revision: u64,
        reason: FfiComposerRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiComposerPageRecord {
    pub schema_version: u16,
    pub scope: FfiComposerScopeRecord,
    pub entries: Vec<FfiComposerListEntry>,
    pub next_cursor: Option<String>,
}

impl From<&ComposerPage> for FfiComposerPageRecord {
    fn from(value: &ComposerPage) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            scope: value.scope().into(),
            entries: value
                .entries()
                .iter()
                .map(|entry| match entry {
                    ComposerListEntry::Draft(summary) => FfiComposerListEntry::Draft {
                        summary: FfiComposerSummaryRecord {
                            schema_version: MOBILE_FFI_SCHEMA_VERSION,
                            id: hex::encode(summary.id().as_bytes()),
                            revision: summary.revision().get(),
                            edit_sequence: summary.edit_sequence().get(),
                            command_type: summary.command_type().into(),
                            created_at_unix_ms: summary.created_at_unix_ms(),
                            updated_at_unix_ms: summary.updated_at_unix_ms(),
                        },
                    },
                    ComposerListEntry::Repair {
                        draft_key,
                        revision,
                        reason,
                    } => FfiComposerListEntry::Repair {
                        draft_key: hex::encode(draft_key),
                        revision: *revision,
                        reason: match reason {
                            ComposerRepairReason::UnsupportedSchema => {
                                FfiComposerRepairReason::UnsupportedSchema
                            }
                            ComposerRepairReason::CorruptRecord => {
                                FfiComposerRepairReason::CorruptRecord
                            }
                        },
                    },
                })
                .collect(),
            next_cursor: value.next_cursor().map(str::to_owned),
        }
    }
}

pub(crate) fn version(value: u16) -> Result<(), TeraAppError> {
    if value != MOBILE_FFI_SCHEMA_VERSION {
        return Err(TeraAppError::invalid_argument(
            "composer_schema_unsupported",
        ));
    }
    Ok(())
}

fn canonical_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn id(value: &str) -> Result<ComposerId, TeraAppError> {
    if value.len() != 32 || !canonical_hex(value) {
        return Err(ComposerError::InvalidIdentity.into());
    }
    ComposerId::new(crate::decode_id(value, "composer_id_invalid")?).map_err(Into::into)
}

impl FfiComposerSaveRequest {
    pub(crate) fn validate_identity(
        &self,
    ) -> Result<(ComposerId, Option<ComposerRevision>, ComposerEditSequence), TeraAppError> {
        version(self.schema_version)?;
        Ok((
            id(&self.id)?,
            self.expected_revision
                .map(ComposerRevision::new)
                .transpose()?,
            ComposerEditSequence::new(self.edit_sequence)?,
        ))
    }
}

// Editing payloads must not leak into diagnostic formatting.
macro_rules! redacted_debug {
    ($($name:ty),+ $(,)?) => {$(
        impl std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        }
    )+};
}
redacted_debug!(
    FfiComposerMediaRecord,
    FfiComposerFormRecord,
    FfiComposerSaveRequest,
    FfiComposerDraftRecord,
    FfiComposerSaveReceipt
);
