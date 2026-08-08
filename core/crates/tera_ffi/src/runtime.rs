use std::sync::Arc;

use radroots_mobile_core::runtime::product_surface::LocalNetworkRelayPolicy;
use radroots_mobile_core::runtime::product_surface::TodayPageRequest;

use crate::dto::PreparedMedia;
use crate::signer::HostSignerAdapter;
use crate::subscription::SubscriptionHub;
use crate::{
    FfiAddDraftInput, FfiAddSchemaRecord, FfiBlossomUploadInput, FfiCapabilityRecord,
    FfiCardAddParityRecord, FfiDraftStatusRecord, FfiIdentityStatusRecord, FfiLocalNetworkRecord,
    FfiMeRecord, FfiQueuePolicyRecord, FfiRelayStatusReportRecord, FfiRetractionDraftInput,
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

#[cfg_attr(not(coverage_nightly), uniffi::export)]
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

    pub fn sdk_blossom_profile(&self) -> Result<Option<String>, RadrootsAppError> {
        self.inner.sdk_blossom_profile().map_err(Into::into)
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

    pub fn configure_public_blossom(&self, origins: Vec<String>) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_public_blossom(origins)
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        Ok(())
    }

    pub fn configure_simulator_blossom(
        &self,
        origins: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_simulator_blossom(origins)
            .map_err(RadrootsAppError::from)?;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        Ok(())
    }

    pub fn configure_device_blossom(&self, origins: Vec<String>) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_device_blossom(origins)
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
        input.command_and_media(authored_at_unix_s).map(|_| ())
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
        let (command, media, form) = input.command_media_and_form(authored_at_unix_s)?;
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
