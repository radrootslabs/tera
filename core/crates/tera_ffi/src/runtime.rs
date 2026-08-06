use radroots_mobile_core::runtime::{
    info::RuntimeInfo,
    key_management::{NostrHostCustodyIdentity, NostrIdentityRecord, NostrIdentitySnapshot},
    nostr::{NostrConnectionStatus, NostrPostEventMetadata, NostrProfileEventMetadata},
    product_surface::{
        ActiveContext, AddAction, AuthorityAction, AuthorityDomain, AuthorityGate,
        ObjectPageSummary, OutboxItem, OutboxRetryDecision, ProofProvenanceArtifact, PrototypePath,
        RouteExecutionFlow, SearchResultSummary, StewardshipAccessItem, TodayCard, WorkflowActor,
    },
    sdk::{SdkCapabilityRecord, SdkShutdownRecord, SdkStorageStatusRecord},
};

use crate::RadrootsAppError;

/// Native boundary object delegating all behavior to the ordinary Rust core.
#[derive(uniffi::Object)]
pub struct RadrootsRuntime {
    inner: radroots_mobile_core::RadrootsRuntime,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub fn new() -> Result<Self, RadrootsAppError> {
        radroots_mobile_core::RadrootsRuntime::new()
            .map(|inner| Self { inner })
            .map_err(Into::into)
    }

    pub async fn shutdown(&self) -> Result<SdkShutdownRecord, RadrootsAppError> {
        self.inner.shutdown().await.map_err(Into::into)
    }

    pub fn uptime_millis(&self) -> i64 {
        self.inner.uptime_millis()
    }

    pub fn info(&self) -> RuntimeInfo {
        self.inner.info()
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

    pub fn sdk_capabilities(&self) -> Vec<SdkCapabilityRecord> {
        self.inner.sdk_capabilities()
    }

    pub async fn sdk_storage_status(&self) -> Result<SdkStorageStatusRecord, RadrootsAppError> {
        self.inner.sdk_storage_status().await.map_err(Into::into)
    }

    pub fn nostr_identity_has_selected_signing_identity(&self) -> bool {
        self.inner.nostr_identity_has_selected_signing_identity()
    }

    pub fn nostr_identity_selected_npub(&self) -> Option<String> {
        self.inner.nostr_identity_selected_npub()
    }

    pub fn nostr_identity_list(&self) -> Result<Vec<NostrIdentityRecord>, RadrootsAppError> {
        self.inner.nostr_identity_list().map_err(Into::into)
    }

    pub fn nostr_identity_list_ids(&self) -> Result<Vec<String>, RadrootsAppError> {
        self.inner.nostr_identity_list_ids().map_err(Into::into)
    }

    pub fn nostr_identity_snapshot(&self) -> Result<NostrIdentitySnapshot, RadrootsAppError> {
        self.inner.nostr_identity_snapshot().map_err(Into::into)
    }

    pub fn nostr_identity_validate_host_custody_secret(
        &self,
        secret_key: String,
    ) -> Result<NostrHostCustodyIdentity, RadrootsAppError> {
        self.inner
            .nostr_identity_validate_host_custody_secret(secret_key)
            .map_err(Into::into)
    }

    pub fn nostr_identity_restore_host_custody_secret(
        &self,
        secret_key: String,
        label: Option<String>,
        make_selected: bool,
    ) -> Result<NostrIdentityRecord, RadrootsAppError> {
        self.inner
            .nostr_identity_restore_host_custody_secret(secret_key, label, make_selected)
            .map_err(Into::into)
    }

    pub fn nostr_identity_select(&self, identity_id: String) -> Result<(), RadrootsAppError> {
        self.inner
            .nostr_identity_select(identity_id)
            .map_err(Into::into)
    }

    pub fn nostr_identity_remove(&self, identity_id: String) -> Result<(), RadrootsAppError> {
        self.inner
            .nostr_identity_remove(identity_id)
            .map_err(Into::into)
    }

    pub fn nostr_identity_lock_host_custody_runtime(&self) -> Result<(), RadrootsAppError> {
        self.inner
            .nostr_identity_lock_host_custody_runtime()
            .map_err(Into::into)
    }

    pub fn nostr_identity_reset_host_custody_runtime(&self) -> Result<(), RadrootsAppError> {
        self.inner
            .nostr_identity_reset_host_custody_runtime()
            .map_err(Into::into)
    }

    pub fn nostr_set_default_relays(&self, relays: Vec<String>) -> Result<(), RadrootsAppError> {
        self.inner
            .nostr_set_default_relays(relays)
            .map_err(Into::into)
    }

    pub fn nostr_connect_if_key_present(&self) -> Result<(), RadrootsAppError> {
        self.inner
            .nostr_connect_if_key_present()
            .map_err(Into::into)
    }

    pub async fn nostr_connection_status(&self) -> Result<NostrConnectionStatus, RadrootsAppError> {
        self.inner
            .nostr_connection_status()
            .await
            .map_err(Into::into)
    }

    pub async fn nostr_profile_for_self(
        &self,
    ) -> Result<Option<NostrProfileEventMetadata>, RadrootsAppError> {
        self.inner
            .nostr_profile_for_self()
            .await
            .map_err(Into::into)
    }

    pub async fn nostr_post_profile(
        &self,
        name: Option<String>,
        display_name: Option<String>,
        nip05: Option<String>,
        about: Option<String>,
    ) -> Result<String, RadrootsAppError> {
        self.inner
            .nostr_post_profile(name, display_name, nip05, about)
            .await
            .map_err(Into::into)
    }

    pub async fn nostr_post_text_note(&self, content: String) -> Result<String, RadrootsAppError> {
        self.inner
            .nostr_post_text_note(content)
            .await
            .map_err(Into::into)
    }

    pub async fn nostr_fetch_text_notes(
        &self,
        limit: u16,
        since_unix: Option<u64>,
    ) -> Result<Vec<NostrPostEventMetadata>, RadrootsAppError> {
        self.inner
            .nostr_fetch_text_notes(limit, since_unix)
            .await
            .map_err(Into::into)
    }

    pub async fn nostr_post_reply(
        &self,
        parent_event_id_hex: String,
        parent_author_hex: String,
        content: String,
        root_event_id_hex: Option<String>,
    ) -> Result<String, RadrootsAppError> {
        self.inner
            .nostr_post_reply(
                parent_event_id_hex,
                parent_author_hex,
                content,
                root_event_id_hex,
            )
            .await
            .map_err(Into::into)
    }

    pub fn phase1_active_contexts(&self) -> Vec<ActiveContext> {
        self.inner.phase1_active_contexts()
    }

    pub fn phase1_today_cards(&self, context_id: Option<String>) -> Vec<TodayCard> {
        self.inner.phase1_today_cards(context_id)
    }

    pub fn phase1_add_actions(&self, context_id: Option<String>) -> Vec<AddAction> {
        self.inner.phase1_add_actions(context_id)
    }

    pub fn phase1_object_page_summaries(
        &self,
        context_id: Option<String>,
    ) -> Vec<ObjectPageSummary> {
        self.inner.phase1_object_page_summaries(context_id)
    }

    pub fn phase1_outbox_snapshot(&self) -> Vec<OutboxItem> {
        self.inner.phase1_outbox_snapshot()
    }

    pub fn phase1_search_results(
        &self,
        query: Option<String>,
        context_id: Option<String>,
    ) -> Vec<SearchResultSummary> {
        self.inner.phase1_search_results(query, context_id)
    }

    pub fn phase1_prototype_paths(&self) -> Vec<PrototypePath> {
        self.inner.phase1_prototype_paths()
    }

    pub fn phase1_route_execution_flows(
        &self,
        context_id: Option<String>,
    ) -> Vec<RouteExecutionFlow> {
        self.inner.phase1_route_execution_flows(context_id)
    }

    pub fn phase1_proof_provenance_artifacts(
        &self,
        context_id: Option<String>,
    ) -> Vec<ProofProvenanceArtifact> {
        self.inner.phase1_proof_provenance_artifacts(context_id)
    }

    pub fn phase1_stewardship_access_items(
        &self,
        context_id: Option<String>,
    ) -> Vec<StewardshipAccessItem> {
        self.inner.phase1_stewardship_access_items(context_id)
    }

    pub fn phase1_outbox_retry_decision(&self, item: OutboxItem) -> OutboxRetryDecision {
        self.inner.phase1_outbox_retry_decision(item)
    }

    pub fn phase1_check_authority(
        &self,
        actor: WorkflowActor,
        context: ActiveContext,
        domain: AuthorityDomain,
        action: AuthorityAction,
    ) -> AuthorityGate {
        self.inner
            .phase1_check_authority(actor, context, domain, action)
    }
}
