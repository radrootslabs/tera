use std::sync::Arc;

use radroots_mobile_core::runtime::product_surface::{
    LocalNetworkRelayPolicy, Phase1AddIntent, Phase1ExistingDraft, Phase1MediaCachePolicy,
    Phase1QueueIntent, Phase1ReviseIntent, ReplaceMobileSettings, TodayPageRequest,
    phase1_new_addressable_identifier, phase1_operation_now_unix_ms,
};

use crate::dto::PreparedMedia;
use crate::operations::{
    FfiIdentityCommandRecord, FfiMediaCacheStatusRecord, FfiMediaOperation,
    FfiMobileSettingsRecord, FfiProfileMetadataInputRecord, FfiProfileStatusRecord,
    FfiReplaceSettingsRecord, FfiRevisionInputRecord, FfiRevisionStatusRecord,
    FfiSettingsTransitionRecord, FfiVerifiedMediaArtifactRecord, decode_artifact_id,
    decode_configuration, decode_reference_fingerprint,
};
use crate::signer::HostSignerAdapter;
use crate::subscription::SubscriptionHub;
use crate::{
    FfiAddDraftInput, FfiAddSchemaRecord, FfiBlossomConfigurationRecord,
    FfiBlossomEndpointAuthority, FfiBlossomEvidenceRecord, FfiBlossomHostKind,
    FfiBlossomUploadInput, FfiBlossomUploadIntent, FfiCapabilityRecord, FfiCardAddParityRecord,
    FfiDraftStatusRecord, FfiIdentityStatusRecord, FfiLocalNetworkRecord, FfiMeRecord,
    FfiQueuePolicyRecord, FfiRelayStatusReportRecord, FfiRetractionDraftInput,
    FfiRuntimeChangeKind, FfiRuntimeInfoRecord, FfiSearchResultRecord, FfiShutdownRecord,
    FfiStorageStatusRecord, FfiSubscriptionHandle, FfiTodayPageRecord, FfiTodayProjectionUpdate,
    FfiTodayRefreshRecord, FfiTodaySyncRecord, RadrootsAppError, RadrootsHostSigner,
    RadrootsRuntimeObserver, add_schemas, decode_id,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum ProtectedDataAvailability {
    Available,
    Unavailable,
}

impl From<ProtectedDataAvailability>
    for radroots_mobile_core::runtime::store::ProtectedDataAvailability
{
    fn from(value: ProtectedDataAvailability) -> Self {
        match value {
            ProtectedDataAvailability::Available => Self::Available,
            ProtectedDataAvailability::Unavailable => Self::Unavailable,
        }
    }
}

/// Native boundary object delegating all product behavior to the ordinary Rust core.
#[derive(uniffi::Object)]
pub struct RadrootsRuntime {
    inner: radroots_mobile_core::RadrootsRuntime,
    has_host_signer: bool,
    subscriptions: Arc<SubscriptionHub>,
}

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl RadrootsRuntime {
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub async fn new(
        application_support_directory: String,
        public_key_hex: String,
        source_generation_hex: String,
        source_generation_created_at_unix_ms: u64,
        protected_data: ProtectedDataAvailability,
    ) -> Result<Self, RadrootsAppError> {
        build_runtime(
            application_support_directory,
            public_key_hex,
            source_generation_hex,
            source_generation_created_at_unix_ms,
            protected_data,
            None,
        )
        .await
    }

    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub async fn with_host_signer(
        application_support_directory: String,
        public_key_hex: String,
        source_generation_hex: String,
        source_generation_created_at_unix_ms: u64,
        protected_data: ProtectedDataAvailability,
        host_signer: Box<dyn RadrootsHostSigner>,
    ) -> Result<Self, RadrootsAppError> {
        build_runtime(
            application_support_directory,
            public_key_hex,
            source_generation_hex,
            source_generation_created_at_unix_ms,
            protected_data,
            Some(host_signer),
        )
        .await
    }

    pub async fn shutdown(&self) -> Result<FfiShutdownRecord, RadrootsAppError> {
        let result = self
            .inner
            .shutdown()
            .await
            .map(Into::into)
            .map_err(Into::into);
        if result.is_ok() {
            self.subscriptions
                .notify(FfiRuntimeChangeKind::Lifecycle, None);
            self.subscriptions.close();
        }
        result
    }

    pub fn uptime_millis(&self) -> i64 {
        self.inner.uptime_millis()
    }

    pub fn info(&self) -> FfiRuntimeInfoRecord {
        self.inner.info().into()
    }

    pub fn info_json(&self) -> String {
        self.inner.info_json()
    }

    pub fn set_app_info_platform(
        &self,
        platform: Option<String>,
        bundle_id: Option<String>,
        version: Option<String>,
        build_number: Option<String>,
        build_sha: Option<String>,
    ) {
        self.inner
            .set_app_info_platform(platform, bundle_id, version, build_number, build_sha);
    }

    pub fn identity_status(&self) -> Result<FfiIdentityStatusRecord, RadrootsAppError> {
        let public_key = self
            .inner
            .authenticated_store_public_key_hex()
            .ok_or_else(|| {
                RadrootsAppError::failure(
                    "identity_unavailable",
                    "identity",
                    true,
                    &["unlock_identity"],
                    "The active identity is unavailable.",
                )
            })?;
        Ok(FfiIdentityStatusRecord {
            schema_version: crate::MOBILE_FFI_SCHEMA_VERSION,
            public_key,
            host_signer_configured: self.has_host_signer,
        })
    }

    pub fn sdk_capabilities(&self) -> Vec<FfiCapabilityRecord> {
        self.inner
            .sdk_capabilities()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub async fn sdk_storage_status(&self) -> Result<FfiStorageStatusRecord, RadrootsAppError> {
        self.inner
            .sdk_storage_status()
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn sdk_relay_status(&self) -> Result<Option<FfiRelayStatusReportRecord>, RadrootsAppError> {
        self.inner
            .sdk_relay_status()
            .map(|value| value.map(Into::into))
            .map_err(Into::into)
    }

    pub fn sdk_blossom_configuration(
        &self,
    ) -> Result<Option<FfiBlossomConfigurationRecord>, RadrootsAppError> {
        self.inner
            .sdk_blossom_configuration()
            .map(|value| value.map(Into::into))
            .map_err(Into::into)
    }

    pub fn sdk_blossom_evidence(
        &self,
    ) -> Result<Option<FfiBlossomEvidenceRecord>, RadrootsAppError> {
        self.inner
            .sdk_blossom_evidence()
            .map(|value| value.map(Into::into))
            .map_err(Into::into)
    }

    pub async fn probe_blossom(&self) -> Result<FfiBlossomEvidenceRecord, RadrootsAppError> {
        let evidence = self
            .inner
            .probe_blossom()
            .await
            .map(Into::into)
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        Ok(evidence)
    }

    pub fn subscribe_changes(
        &self,
        observer: Box<dyn RadrootsRuntimeObserver>,
    ) -> Result<Arc<FfiSubscriptionHandle>, RadrootsAppError> {
        self.subscriptions.subscribe(observer)
    }

    pub fn configure_public_relays(
        &self,
        writable_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_public_relays(writable_relays)
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Relay, None);
        Ok(())
    }

    pub fn configure_simulator_relays(
        &self,
        loopback_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_simulator_relays(loopback_relays)
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Relay, None);
        Ok(())
    }

    pub fn configure_device_relays(
        &self,
        writable_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_device_relays(writable_relays)
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Relay, None);
        Ok(())
    }

    pub fn configure_blossom(
        &self,
        host_kind: FfiBlossomHostKind,
        endpoint_authority: FfiBlossomEndpointAuthority,
        primary_origin: String,
        fallback_origins: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_blossom(
                host_kind.into(),
                endpoint_authority.into(),
                primary_origin,
                fallback_origins,
            )
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        Ok(())
    }

    pub fn phase1_card_add_parity(&self) -> Vec<FfiCardAddParityRecord> {
        self.inner
            .phase1_card_add_parity()
            .into_iter()
            .map(|value| FfiCardAddParityRecord {
                schema_version: crate::MOBILE_FFI_SCHEMA_VERSION,
                card_type: value.card_type.into(),
                command_type: value.add_command_type.into(),
            })
            .collect()
    }

    pub fn phase1_add_schemas(&self) -> Vec<FfiAddSchemaRecord> {
        add_schemas()
    }

    pub fn phase1_local_network(
        &self,
        context: FfiLocalNetworkRecord,
    ) -> Result<FfiLocalNetworkRecord, RadrootsAppError> {
        self.local_network(context).map(Into::into)
    }

    pub async fn phase1_today_page(
        &self,
        context: FfiLocalNetworkRecord,
        limit: u16,
        as_of_unix_s: Option<u64>,
        cursor: Option<String>,
    ) -> Result<FfiTodayPageRecord, RadrootsAppError> {
        if as_of_unix_s.is_some() == cursor.is_some() {
            return Err(RadrootsAppError::invalid_argument(
                "invalid_today_page_request",
            ));
        }
        let context = self.local_network(context)?;
        let request = match cursor {
            Some(cursor) => TodayPageRequest::after(limit, cursor),
            None => TodayPageRequest::first(
                limit,
                as_of_unix_s
                    .ok_or_else(|| RadrootsAppError::invalid_argument("today_as_of_required"))?,
            ),
        };
        self.inner
            .phase1_today_page(&context, request)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn phase1_refresh_today(
        &self,
        context: FfiLocalNetworkRecord,
        now_unix_s: u64,
        update: FfiTodayProjectionUpdate,
    ) -> Result<FfiTodayRefreshRecord, RadrootsAppError> {
        let context = self.local_network(context)?;
        let receipt = self
            .inner
            .phase1_refresh_today(&context, now_unix_s, update.into())
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Today, None);
        Ok(receipt.into())
    }

    pub async fn phase1_sync_today(
        &self,
        context: FfiLocalNetworkRecord,
        now_unix_s: u64,
        update: FfiTodayProjectionUpdate,
    ) -> Result<FfiTodaySyncRecord, RadrootsAppError> {
        let context = self.local_network(context)?;
        let receipt = self
            .inner
            .phase1_sync_today(&context, now_unix_s, update.into())
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Today, None);
        Ok(receipt.into())
    }

    pub async fn phase1_search(
        &self,
        context: FfiLocalNetworkRecord,
        query: String,
        limit: u16,
        as_of_unix_s: u64,
    ) -> Result<Vec<FfiSearchResultRecord>, RadrootsAppError> {
        let context = self.local_network(context)?;
        self.inner
            .phase1_search(&context, &query, limit, as_of_unix_s)
            .await
            .map(|results| results.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub async fn phase1_me(
        &self,
        context: FfiLocalNetworkRecord,
        as_of_unix_s: u64,
    ) -> Result<FfiMeRecord, RadrootsAppError> {
        let context = self.local_network(context)?;
        let public_key = self
            .inner
            .authenticated_store_public_key_hex()
            .ok_or_else(|| {
                RadrootsAppError::failure(
                    "identity_unavailable",
                    "identity",
                    true,
                    &["unlock_identity"],
                    "The active identity is unavailable.",
                )
            })?;
        self.inner
            .phase1_me(&context, &public_key, as_of_unix_s)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn phase1_validate_add_draft(
        &self,
        input: FfiAddDraftInput,
        authored_at_unix_s: u64,
    ) -> Result<(), RadrootsAppError> {
        let blossom = self
            .inner
            .sdk_blossom_slot()
            .map_err(RadrootsAppError::from)?;
        input
            .command_and_media(authored_at_unix_s, blossom.as_ref())
            .map(|_| ())
    }

    /// Saves one new or existing Add form while Rust owns all identity and
    /// timestamp policy. Addressable identifiers are generated when omitted.
    pub async fn phase1_save_add_intent(
        &self,
        mut input: FfiAddDraftInput,
        existing_draft_id: Option<String>,
        expected_revision: Option<u64>,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        if input.identifier.is_none()
            && matches!(
                input.command_type,
                crate::FfiAddCommandType::CreateEvent
                    | crate::FfiAddCommandType::CreateFoodAvailability
            )
        {
            input.identifier = Some(phase1_new_addressable_identifier());
        }
        let authored_at_unix_s =
            phase1_operation_now_unix_ms().map_err(RadrootsAppError::from)? / 1_000;
        let blossom = self
            .inner
            .sdk_blossom_slot()
            .map_err(RadrootsAppError::from)?;
        let (command, media, form) =
            input.command_media_and_form(authored_at_unix_s, blossom.as_ref())?;
        let existing = match (existing_draft_id, expected_revision) {
            (Some(draft_id), Some(revision)) => Some(
                Phase1ExistingDraft::new(decode_id(&draft_id, "invalid_draft_id")?, revision)
                    .map_err(RadrootsAppError::from)?,
            ),
            (None, None) => None,
            _ => {
                return Err(RadrootsAppError::invalid_argument("invalid_existing_draft"));
            }
        };
        let status = self
            .inner
            .phase1_save_add_intent(
                Phase1AddIntent::new(command, media, form, existing)
                    .map_err(RadrootsAppError::from)?,
            )
            .await
            .map_err(RadrootsAppError::from)?;
        let draft_id = hex::encode(status.draft().draft_id().as_bytes());
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_save_draft(
        &self,
        draft_id: String,
        input: FfiAddDraftInput,
        authored_at_unix_s: u64,
        expected_revision: Option<u64>,
        persisted_at_unix_ms: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let blossom = self
            .inner
            .sdk_blossom_slot()
            .map_err(RadrootsAppError::from)?;
        let (command, media, form) =
            input.command_media_and_form(authored_at_unix_s, blossom.as_ref())?;
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_save_draft_with_form(
                decoded_id,
                command,
                authored_at_unix_s,
                media,
                form,
                expected_revision,
                persisted_at_unix_ms,
            )
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_save_retraction_draft(
        &self,
        draft_id: String,
        input: FfiRetractionDraftInput,
        authored_at_unix_s: u64,
        persisted_at_unix_ms: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        if input.schema_version != crate::MOBILE_FFI_SCHEMA_VERSION {
            return Err(RadrootsAppError::invalid_argument(
                "unsupported_schema_version",
            ));
        }
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let command_type = match input.command_type {
            crate::FfiAddCommandType::CreateUpdate => {
                radroots_mobile_core::runtime::product_surface::AddCommandType::CreateUpdate
            }
            crate::FfiAddCommandType::CreatePhotoUpdate => {
                radroots_mobile_core::runtime::product_surface::AddCommandType::CreatePhotoUpdate
            }
            crate::FfiAddCommandType::CreateAsk => {
                radroots_mobile_core::runtime::product_surface::AddCommandType::CreateAsk
            }
            crate::FfiAddCommandType::CreateEvent => {
                radroots_mobile_core::runtime::product_surface::AddCommandType::CreateEvent
            }
            crate::FfiAddCommandType::CreateFoodAvailability => {
                radroots_mobile_core::runtime::product_surface::AddCommandType::CreateFoodAvailability
            }
        };
        let card_id =
            radroots_mobile_core::runtime::product_surface::CardId::parse(&input.target_card_id)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_card_id"))?;
        let status = self
            .inner
            .phase1_save_retraction_draft(
                decoded_id,
                command_type,
                card_id,
                &input.target_event_id,
                input.target_kind,
                input.target_address.as_deref(),
                &input.reason,
                authored_at_unix_s,
                persisted_at_unix_ms,
            )
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_draft_status(
        &self,
        draft_id: String,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        self.inner
            .phase1_draft_status(decode_id(&draft_id, "invalid_draft_id")?)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn phase1_draft_heads(
        &self,
        limit: u16,
    ) -> Result<Vec<FfiDraftStatusRecord>, RadrootsAppError> {
        self.inner
            .phase1_draft_heads(limit)
            .await
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub async fn phase1_queue_draft(
        &self,
        draft_id: String,
        expected_revision: u64,
        policy: FfiQueuePolicyRecord,
        queued_at_unix_ms: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_queue_draft(
                decoded_id,
                expected_revision,
                policy.try_into()?,
                queued_at_unix_ms,
            )
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    /// Queues with the Rust-owned active relay and settlement policy.
    pub async fn phase1_queue_add_intent(
        &self,
        draft_id: String,
        expected_revision: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let intent = Phase1QueueIntent::new(decoded_id, expected_revision)
            .map_err(RadrootsAppError::from)?;
        let status = self
            .inner
            .phase1_queue_add_intent(intent)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_recover_draft_queue(
        &self,
        draft_id: String,
        recovered_at_unix_ms: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_recover_draft_queue(decoded_id, recovered_at_unix_ms)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_recover_add_intent(
        &self,
        draft_id: String,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_recover_add_intent(decoded_id)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_sign_queued_draft(
        &self,
        draft_id: String,
        expected_revision: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_sign_queued_draft(decoded_id, expected_revision)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_advance_draft(
        &self,
        draft_id: String,
        expected_revision: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_advance_draft(decoded_id, expected_revision)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_upload_draft_media(
        &self,
        input: FfiBlossomUploadInput,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        if input.schema_version != crate::MOBILE_FFI_SCHEMA_VERSION {
            return Err(RadrootsAppError::invalid_argument(
                "unsupported_schema_version",
            ));
        }
        let draft_id = decode_id(&input.draft_id, "invalid_draft_id")?;
        let operation_id = decode_id(&input.operation_id, "invalid_operation_id")?;
        let artifact_id = decode_id(&input.artifact_id, "invalid_artifact_id")?;
        let media = PreparedMedia::try_from(input.media)?;
        let request = media.upload_request(input.verified_at_unix_ms)?;
        let content = radroots_blossom::authorization::AuthorizationContent::parse(
            &input.authorization_content,
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_blossom_authorization"))?;
        let status = self
            .inner
            .phase1_upload_draft_media(
                draft_id,
                input.expected_revision,
                request,
                content,
                input.authorization_created_at_unix_s,
                input.authorization_lifetime_seconds,
                operation_id,
                artifact_id,
                input.signing_deadline_unix_ms,
                input.signing_cancellation.core(),
                radroots_sdk::transport::BlossomCancellation::default(),
                input.updated_at_unix_ms,
            )
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Media, Some(input.draft_id.clone()));
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(input.draft_id));
        Ok(status.into())
    }

    /// Runs a Rust-planned BUD-11/BUD-02/BUD-01 upload attempt. The host
    /// supplies only the selected bounded file handle and draft revision.
    pub async fn phase1_upload_add_media_intent(
        &self,
        input: FfiBlossomUploadIntent,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        if input.schema_version != crate::MOBILE_FFI_SCHEMA_VERSION {
            return Err(RadrootsAppError::invalid_argument(
                "unsupported_schema_version",
            ));
        }
        let draft_id = decode_id(&input.draft_id, "invalid_draft_id")?;
        let intent = PreparedMedia::try_from(input.media)?
            .into_upload_intent(draft_id, input.expected_revision)?;
        let status = self
            .inner
            .phase1_upload_add_media_intent(intent)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Media, Some(input.draft_id.clone()));
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(input.draft_id));
        Ok(status.into())
    }

    /// Persists the upload transition before returning an immutable native
    /// background-transfer job.
    pub async fn phase1_prepare_add_media_background(
        &self,
        input: crate::FfiBlossomUploadIntent,
    ) -> Result<crate::FfiNativeUploadJobRecord, RadrootsAppError> {
        if input.schema_version != crate::MOBILE_FFI_SCHEMA_VERSION {
            return Err(RadrootsAppError::invalid_argument(
                "unsupported_schema_version",
            ));
        }
        let draft_id = decode_id(&input.draft_id, "invalid_draft_id")?;
        let intent = PreparedMedia::try_from(input.media)?
            .into_upload_intent(draft_id, input.expected_revision)?;
        let (status, job) = self
            .inner
            .phase1_prepare_native_upload(intent)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Media, Some(input.draft_id.clone()));
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(input.draft_id));
        Ok(crate::FfiNativeUploadJobRecord {
            schema_version: crate::MOBILE_FFI_SCHEMA_VERSION,
            operation_id: hex::encode(job.operation_id()),
            draft: status.into(),
            remote_url: job.remote_url().to_owned(),
            authorization_header: job.authorization_header().to_owned(),
            expected_sha256: job.expected_sha256().to_owned(),
            media_type: job.media_type().to_owned(),
            byte_size: job.byte_size(),
        })
    }

    /// Accepts only bounded native HTTP evidence; Rust performs descriptor and
    /// exact-byte retrieval verification before advancing durable state.
    pub async fn phase1_complete_add_media_background(
        &self,
        input: crate::FfiNativeUploadCompletionInput,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        if input.schema_version != crate::MOBILE_FFI_SCHEMA_VERSION
            || input.response_body.len() > 16_384
        {
            return Err(RadrootsAppError::invalid_argument(
                "invalid_native_upload_completion",
            ));
        }
        let draft_id = decode_id(&input.draft_id, "invalid_draft_id")?;
        let intent = PreparedMedia::try_from(input.media)?
            .into_upload_intent(draft_id, input.expected_revision)?;
        let status = self
            .inner
            .phase1_complete_native_upload(
                intent,
                input.status_code,
                input.response_media_type.as_deref(),
                input.response_content_encoding.as_deref(),
                input.response_body.as_slice(),
            )
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Media, Some(input.draft_id.clone()));
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(input.draft_id));
        Ok(status.into())
    }

    pub async fn phase1_cancel_draft(
        &self,
        draft_id: String,
        expected_revision: u64,
        cancelled_at_unix_ms: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_cancel_draft(decoded_id, expected_revision, cancelled_at_unix_ms)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_cancel_add_intent(
        &self,
        draft_id: String,
        expected_revision: u64,
    ) -> Result<FfiDraftStatusRecord, RadrootsAppError> {
        let decoded_id = decode_id(&draft_id, "invalid_draft_id")?;
        let status = self
            .inner
            .phase1_cancel_add_intent(decoded_id, expected_revision)
            .await
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        Ok(status.into())
    }

    pub async fn phase1_settings(&self) -> Result<FfiMobileSettingsRecord, RadrootsAppError> {
        self.inner
            .phase1_settings()
            .await
            .map(|settings| (&settings).into())
            .map_err(Into::into)
    }

    pub async fn phase1_apply_settings_to_runtime(
        &self,
    ) -> Result<FfiMobileSettingsRecord, RadrootsAppError> {
        let settings = self.inner.phase1_settings().await?;
        self.inner
            .configure_relay_preferences(settings.relays())
            .map_err(RadrootsAppError::from)?;
        self.inner
            .configure_blossom_preferences(settings.blossom())
            .map_err(RadrootsAppError::from)?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Settings, None);
        Ok((&settings).into())
    }

    pub async fn phase1_replace_settings(
        &self,
        input: FfiReplaceSettingsRecord,
    ) -> Result<FfiSettingsTransitionRecord, RadrootsAppError> {
        let current = self.inner.phase1_settings().await?;
        let expected_revision = input.expected_revision;
        let next = input.apply(current)?;
        let transition = self
            .inner
            .phase1_replace_settings(ReplaceMobileSettings::new(expected_revision, next)?)
            .await?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Settings, None);
        Ok(transition.into())
    }

    pub async fn phase1_apply_identity_command(
        &self,
        expected_revision: u64,
        command: FfiIdentityCommandRecord,
    ) -> Result<FfiSettingsTransitionRecord, RadrootsAppError> {
        let transition = self
            .inner
            .phase1_apply_identity_command(expected_revision, command.try_into()?)
            .await?;
        let identity_id = transition
            .settings
            .identity()
            .active_identity_id()
            .map(str::to_owned);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Identity, identity_id);
        Ok(transition.into())
    }

    pub async fn phase1_save_profile_metadata(
        &self,
        input: FfiProfileMetadataInputRecord,
    ) -> Result<FfiProfileStatusRecord, RadrootsAppError> {
        let blossom = self
            .inner
            .sdk_blossom_slot()
            .map_err(RadrootsAppError::from)?;
        let status = self
            .inner
            .phase1_save_profile_metadata(input.command(blossom.as_ref())?)
            .await?;
        let operation_id = hex::encode(status.draft().draft_id().as_bytes());
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Profile, Some(operation_id));
        Ok(status.into())
    }

    pub async fn phase1_profile_status(
        &self,
        operation_id: String,
    ) -> Result<FfiProfileStatusRecord, RadrootsAppError> {
        self.inner
            .phase1_profile_status(decode_id(&operation_id, "invalid_operation_id")?)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn phase1_advance_profile(
        &self,
        operation_id: String,
    ) -> Result<FfiProfileStatusRecord, RadrootsAppError> {
        let status = self
            .inner
            .phase1_advance_profile(decode_id(&operation_id, "invalid_operation_id")?)
            .await?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Profile, Some(operation_id));
        Ok(status.into())
    }

    pub async fn phase1_cancel_profile(
        &self,
        operation_id: String,
        expected_revision: u64,
    ) -> Result<FfiProfileStatusRecord, RadrootsAppError> {
        let status = self
            .inner
            .phase1_cancel_profile(
                decode_id(&operation_id, "invalid_operation_id")?,
                expected_revision,
            )
            .await?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Profile, Some(operation_id));
        Ok(status.into())
    }

    pub async fn phase1_save_revision_intent(
        &self,
        mut input: FfiRevisionInputRecord,
    ) -> Result<FfiRevisionStatusRecord, RadrootsAppError> {
        let target = input.target()?;
        if input.replacement.identifier.is_none()
            && matches!(
                input.replacement.command_type,
                crate::FfiAddCommandType::CreateEvent
                    | crate::FfiAddCommandType::CreateFoodAvailability
            )
        {
            input.replacement.identifier = Some(phase1_new_addressable_identifier());
        }
        let authored_at_unix_s = phase1_operation_now_unix_ms()? / 1_000;
        let blossom = self
            .inner
            .sdk_blossom_slot()
            .map_err(RadrootsAppError::from)?;
        let (command, media, form) = input
            .replacement
            .command_media_and_form(authored_at_unix_s, blossom.as_ref())?;
        let status = self
            .inner
            .phase1_save_revision_intent(Phase1ReviseIntent::new(target, command, media, form)?)
            .await?;
        let operation_id = hex::encode(status.replacement().draft().draft_id().as_bytes());
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(operation_id));
        Ok(status.into())
    }

    pub async fn phase1_revision_status(
        &self,
        operation_id: String,
    ) -> Result<FfiRevisionStatusRecord, RadrootsAppError> {
        self.inner
            .phase1_revision_status(decode_id(&operation_id, "invalid_operation_id")?)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn phase1_advance_revision(
        &self,
        operation_id: String,
    ) -> Result<FfiRevisionStatusRecord, RadrootsAppError> {
        let status = self
            .inner
            .phase1_advance_revision(decode_id(&operation_id, "invalid_operation_id")?)
            .await?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(operation_id));
        Ok(status.into())
    }

    pub async fn phase1_cancel_revision(
        &self,
        operation_id: String,
    ) -> Result<FfiRevisionStatusRecord, RadrootsAppError> {
        let status = self
            .inner
            .phase1_cancel_revision(decode_id(&operation_id, "invalid_operation_id")?)
            .await?;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(operation_id));
        Ok(status.into())
    }

    pub async fn phase1_retrieve_media(
        &self,
        context: FfiLocalNetworkRecord,
        reference_fingerprint: String,
        operation: Arc<FfiMediaOperation>,
    ) -> Result<FfiVerifiedMediaArtifactRecord, RadrootsAppError> {
        let context = self.local_network(context)?;
        operation.claim()?;
        let operation_id = operation.operation_id();
        let settings = self.inner.phase1_settings().await?;
        let policy = Phase1MediaCachePolicy::new(
            settings.local_storage().media_cache_bytes(),
            settings.local_storage().media_cache_artifacts(),
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_cache_policy"))?;
        let artifact = self
            .inner
            .phase1_retrieve_media(
                &context,
                decode_reference_fingerprint(&reference_fingerprint)?,
                operation.id(),
                policy,
                operation.cancellation(),
            )
            .await
            .map_err(|error| {
                RadrootsAppError::from(error).with_operation_id(operation_id.clone())
            })?;
        self.subscriptions.notify(
            FfiRuntimeChangeKind::Media,
            Some(artifact.artifact_id().to_hex()),
        );
        Ok(FfiVerifiedMediaArtifactRecord::from_artifact(
            artifact,
            Some(operation_id),
        ))
    }

    pub async fn phase1_verified_media_artifact(
        &self,
        context: FfiLocalNetworkRecord,
        artifact_id: String,
    ) -> Result<Option<FfiVerifiedMediaArtifactRecord>, RadrootsAppError> {
        let context = self.local_network(context)?;
        self.inner
            .phase1_verified_media_artifact(
                &context,
                decode_artifact_id(&artifact_id)?,
                phase1_operation_now_unix_ms()?,
            )
            .await
            .map(|value| {
                value.map(|artifact| FfiVerifiedMediaArtifactRecord::from_artifact(artifact, None))
            })
            .map_err(Into::into)
    }

    pub async fn phase1_media_cache_status(
        &self,
        context: FfiLocalNetworkRecord,
    ) -> Result<FfiMediaCacheStatusRecord, RadrootsAppError> {
        let context = self.local_network(context)?;
        self.inner
            .phase1_media_cache_status(&context)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn phase1_invalidate_media_artifact(
        &self,
        context: FfiLocalNetworkRecord,
        artifact_id: String,
    ) -> Result<bool, RadrootsAppError> {
        let context = self.local_network(context)?;
        let changed = self
            .inner
            .phase1_invalidate_media_artifact(&context, decode_artifact_id(&artifact_id)?)
            .await?;
        if changed {
            self.subscriptions
                .notify(FfiRuntimeChangeKind::Media, Some(artifact_id));
        }
        Ok(changed)
    }

    pub async fn phase1_invalidate_media_configuration(
        &self,
        context: FfiLocalNetworkRecord,
        configuration_fingerprint: String,
    ) -> Result<Vec<String>, RadrootsAppError> {
        let context = self.local_network(context)?;
        let removed = self
            .inner
            .phase1_invalidate_media_configuration(
                &context,
                decode_configuration(&configuration_fingerprint)?,
            )
            .await?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        Ok(removed.into_iter().map(|value| value.to_hex()).collect())
    }
}

impl RadrootsRuntime {
    fn local_network(
        &self,
        context: FfiLocalNetworkRecord,
    ) -> Result<radroots_mobile_core::runtime::product_surface::LocalNetwork, RadrootsAppError>
    {
        let profile = self.inner.sdk_relay_status()?.ok_or_else(|| {
            RadrootsAppError::failure(
                "relay_profile_unavailable",
                "relay",
                true,
                &["configure_relay"],
                "The relay profile is unavailable.",
            )
        })?;
        let relay_policy = match profile.profile.as_str() {
            "public" => LocalNetworkRelayPolicy::Public,
            "simulator_local" => LocalNetworkRelayPolicy::Simulator,
            "device_development" => LocalNetworkRelayPolicy::Device,
            _ => {
                return Err(RadrootsAppError::failure(
                    "relay_profile_unsupported",
                    "relay",
                    false,
                    &["configure_relay"],
                    "The relay profile is unsupported.",
                ));
            }
        };
        context.try_into_with_relay_policy(relay_policy)
    }
}

async fn build_runtime(
    application_support_directory: String,
    public_key_hex: String,
    source_generation_hex: String,
    source_generation_created_at_unix_ms: u64,
    protected_data: ProtectedDataAvailability,
    host_signer: Option<Box<dyn RadrootsHostSigner>>,
) -> Result<RadrootsRuntime, RadrootsAppError> {
    let store = radroots_mobile_core::runtime::store::MobileUserStoreConfig::from_encoded(
        application_support_directory,
        public_key_hex.as_str(),
        source_generation_hex.as_str(),
        source_generation_created_at_unix_ms,
        protected_data.into(),
    )?;
    let has_host_signer = host_signer.is_some();
    let mut builder = radroots_mobile_core::runtime::builder::RuntimeBuilder::new(store);
    if let Some(host_signer) = host_signer {
        builder = builder.signer(Arc::new(HostSignerAdapter::new(host_signer)));
    }
    builder
        .build()
        .await
        .map(|inner| RadrootsRuntime {
            inner,
            has_host_signer,
            subscriptions: SubscriptionHub::new(),
        })
        .map_err(Into::into)
}
