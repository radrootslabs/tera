//! UniFFI converter ownership for ordinary Rust DTOs defined by mobile core.

use radroots_mobile_core::SdkErrorRecord;
use radroots_mobile_core::runtime::app_info::*;
use radroots_mobile_core::runtime::info::*;
use radroots_mobile_core::runtime::key_management::*;
use radroots_mobile_core::runtime::nostr::*;
use radroots_mobile_core::runtime::product_surface::*;
use radroots_mobile_core::runtime::sdk::*;

#[uniffi::remote(Record)]
pub struct SdkErrorRecord {
    pub schema_version: u16,
    pub code: String,
    pub class: String,
    pub retryable: bool,
    pub recovery_actions: Vec<String>,
    pub operation_id: Option<String>,
    pub capability_id: Option<String>,
    pub message: String,
}

#[uniffi::remote(Record)]
pub struct AppInfoPlatform {
    pub platform: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<String>,
    pub build_sha: Option<String>,
}
#[uniffi::remote(Record)]
pub struct RuntimeBuildInfo {
    pub crate_name: String,
    pub crate_version: String,
    pub rustc: Option<String>,
    pub profile: Option<String>,
    pub lib_revision: Option<String>,
    pub consumer_revision: Option<String>,
    pub build_time_unix: Option<u64>,
}

#[uniffi::remote(Record)]
pub struct AppInfo {
    pub build: RuntimeBuildInfo,
    pub started_unix_ms: i64,
    pub uptime_millis: i64,
    pub shutting_down: bool,
    pub platform: Option<AppInfoPlatform>,
}

#[uniffi::remote(Record)]
pub struct RuntimeInfo {
    pub app: AppInfo,
    pub sdk: RuntimeBuildInfo,
    pub sdk_closed: bool,
}
#[uniffi::remote(Record)]
pub struct SdkCapabilityRecord {
    pub id: String,
    pub compiled: bool,
    pub configured: bool,
    pub availability: String,
    pub maturity: String,
}

#[uniffi::remote(Record)]
pub struct SdkStorageStatusRecord {
    pub backend: String,
    pub open_mode: String,
    pub shutdown: String,
    pub integrity: String,
}

#[uniffi::remote(Record)]
pub struct SdkShutdownRecord {
    pub state: String,
    pub already_closed: bool,
}
#[uniffi::remote(Record)]
pub struct NostrIdentityRecord {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
    pub label: Option<String>,
    pub is_selected: bool,
}

#[uniffi::remote(Record)]
pub struct NostrIdentitySnapshot {
    pub has_selected_signing_identity: bool,
    pub selected_identity_id: Option<String>,
    pub selected_npub: Option<String>,
    pub identities: Vec<NostrIdentityRecord>,
}

#[uniffi::remote(Record)]
pub struct NostrHostCustodyIdentity {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
}
#[uniffi::remote(Enum)]
pub enum NostrLight {
    Red,
    Yellow,
    Green,
}

#[uniffi::remote(Record)]
pub struct NostrConnectionStatus {
    pub light: NostrLight,
    pub configured: bool,
    pub source_available: bool,
    pub sink_available: bool,
    pub last_error: Option<String>,
}

#[uniffi::remote(Record)]
pub struct NostrProfile {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
    pub website: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub lud06: Option<String>,
    pub lud16: Option<String>,
    pub bot: Option<String>,
}

#[uniffi::remote(Record)]
pub struct NostrProfileEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub profile: NostrProfile,
}

#[uniffi::remote(Record)]
pub struct NostrPost {
    pub content: String,
}

#[uniffi::remote(Record)]
pub struct NostrPostEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub post: NostrPost,
}
#[uniffi::remote(Enum)]
pub enum ContextType {
    Regional,
    Network,
    Farm,
    Buyer,
    Route,
    RoutePartner,
    PickupPoint,
    TraceRecords,
    Hub,
    NetworkSteward,
}

#[uniffi::remote(Enum)]
pub enum WorkflowActor {
    NetworkMember,
    ProducerAdmin,
    FarmTeamMember,
    HubOperator,
    TraceLead,
    BuyerSourcingLead,
    BuyerReceiver,
    PickupPointCoordinator,
    RouteCoordinator,
    RoutePartner,
    NetworkSteward,
}

#[uniffi::remote(Enum)]
pub enum VisibilityClass {
    LocalDraft,
    FarmPrivate,
    WorkspacePrivate,
    NetworkVisible,
    RouteScoped,
    BuyerScoped,
    PublicCommunity,
    PublicProvenance,
    SecretNeverShared,
}

#[uniffi::remote(Enum)]
pub enum AuthorityDomain {
    #[serde(rename = "Relay/group access")]
    RelayGroupAccess,
    #[serde(rename = "Farm/workspace operations authority")]
    FarmWorkspaceOperations,
    #[serde(rename = "Buyer workspace authority")]
    BuyerWorkspace,
    #[serde(rename = "Route coordination authority")]
    RouteCoordination,
    #[serde(rename = "Route execution authority")]
    RouteExecution,
    #[serde(rename = "Receipt authority")]
    Receipt,
    #[serde(rename = "Trace/proof authority")]
    TraceProof,
    #[serde(rename = "Public publishing authority")]
    PublicPublishing,
    #[serde(rename = "Network stewardship authority")]
    NetworkStewardship,
}

#[uniffi::remote(Enum)]
pub enum AuthorityAction {
    Submit,
    Publish,
    Share,
    Assign,
    Approve,
    Correct,
    Close,
    Retry,
    Search,
    NavigateRelatedObject,
}

#[uniffi::remote(Enum)]
pub enum ObjectKind {
    Region,
    Network,
    Farm,
    BuyerWorkspace,
    Route,
    RoutePartner,
    PickupPoint,
    Hub,
    Food,
    Ask,
    Event,
    Place,
    Task,
    Proof,
    Exception,
    Provenance,
    Update,
    AccessMembership,
    BuyerPacket,
    RouteStop,
    Draft,
    OutboxItem,
    MemberInvite,
    Correction,
}

#[uniffi::remote(Enum)]
pub enum ObjectPageFamily {
    Network,
    NetworkRoute,
    FarmWorkspace,
    FarmPublicProfile,
    BuyerWorkspace,
    PickupPointPlace,
    Food,
    Event,
    RouteStop,
    Proof,
    BuyerPacket,
    PublicProvenance,
    Exception,
}

#[uniffi::remote(Enum)]
pub enum TodayCardType {
    Route,
    Food,
    Ask,
    Event,
    Place,
    Task,
    Proof,
    Exception,
    Provenance,
    Update,
    AccessMembership,
    SyncOutbox,
}

#[uniffi::remote(Enum)]
pub enum AddActionType {
    Photo,
    Note,
    Ask,
    Scan,
    Food,
    Harvest,
    BuyerRequest,
    BuyerCommitment,
    RouteNeed,
    RouteStop,
    PickupEvent,
    Place,
    Proof,
    Exception,
    PublicUpdate,
    Provenance,
    MemberInvite,
    Correction,
}

#[uniffi::remote(Enum)]
pub enum AddFlowState {
    NotStarted,
    Draft,
    Editing,
    ValidationFailed,
    ReadyToSubmit,
    Submitted,
    Queued,
    Syncing,
    NeedsApproval,
    Approved,
    Published,
    Shared,
    Confirmed,
    Failed,
    Conflict,
    Discarded,
}

#[uniffi::remote(Enum)]
pub enum OutboxBehavior {
    LocalOnly,
    QueueWhenOffline,
    RequireOnline,
    PublishWhenAuthorized,
    ShareWhenAuthorized,
}

#[uniffi::remote(Enum)]
pub enum PrototypePathKind {
    ProducerFoodToRoute,
    BuyerCommitmentToRoute,
    RouteCoordinatorAssignment,
}

#[uniffi::remote(Enum)]
pub enum RouteExecutionFlowKind {
    RoutePartnerAssignedStops,
    BuyerReceiptConfirmation,
    ExceptionRecovery,
}

#[uniffi::remote(Enum)]
pub enum RouteExecutionStepKind {
    AssignedRoute,
    PickupConfirmation,
    DropoffConfirmation,
    ReceiptConfirmation,
    ExceptionReport,
    RecoveryAction,
}

#[uniffi::remote(Enum)]
pub enum ProofProvenanceArtifactKind {
    ProofCompleteness,
    BuyerPacketDraft,
    BuyerPacketShared,
    PublicProvenancePreview,
}

#[uniffi::remote(Enum)]
pub enum ProofProvenanceReviewState {
    MissingProof,
    Complete,
    Draft,
    ReadyToShare,
    Shared,
    RedactionRequired,
    ReadyToPublish,
    Published,
}

#[uniffi::remote(Enum)]
pub enum StewardshipAccessItemKind {
    AccessRequestReview,
    RoleApproval,
    RoutePartnerInvite,
    RoutePoolMetadata,
    PublicModeration,
    InviteAcceptance,
    RequestAccess,
    AccessDenied,
    GroupManagementDeferred,
}

#[uniffi::remote(Enum)]
pub enum OutboxState {
    NotQueued,
    Draft,
    Queued,
    Syncing,
    AwaitingAuthority,
    Published,
    Shared,
    Failed,
    Conflict,
    Discarded,
}

#[uniffi::remote(Enum)]
pub enum SyncState {
    Unknown,
    Online,
    Offline,
    Syncing,
    Synced,
    Stale,
    Failed,
}

#[uniffi::remote(Record)]
pub struct ObjectRef {
    pub object_type: ObjectKind,
    pub object_id: String,
    pub display_label: String,
}

#[uniffi::remote(Record)]
pub struct EventRef {
    pub event_id: String,
    pub relay_url: Option<String>,
    pub kind: Option<u32>,
}

#[uniffi::remote(Record)]
pub struct ActiveContext {
    pub context_type: ContextType,
    pub context_ref: ObjectRef,
    pub actor: WorkflowActor,
    pub display_label: String,
    pub visibility_scope: VisibilityClass,
}

#[uniffi::remote(Record)]
pub struct AuthorityGate {
    pub domain: AuthorityDomain,
    pub action: AuthorityAction,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub is_required: bool,
    pub is_allowed: bool,
    pub reason: Option<String>,
}

#[uniffi::remote(Record)]
pub struct ObjectPageSummary {
    pub object_ref: ObjectRef,
    pub family: ObjectPageFamily,
    pub primary_context: ActiveContext,
    pub title: String,
    pub subtitle: Option<String>,
    pub visibility: VisibilityClass,
    pub visibility_label: String,
    pub required_authority: AuthorityGate,
    pub sync_state: SyncState,
}

#[uniffi::remote(Record)]
pub struct TodayCardAction {
    pub id: String,
    pub label: String,
    pub action_type: Option<AddActionType>,
    pub target_object: Option<ObjectRef>,
}

#[uniffi::remote(Record)]
pub struct TodayCard {
    pub id: String,
    pub card_type: TodayCardType,
    pub source_object_refs: Vec<ObjectRef>,
    pub source_event_refs: Vec<EventRef>,
    pub primary_context: ActiveContext,
    pub actor: WorkflowActor,
    pub visibility: VisibilityClass,
    pub visibility_label: String,
    pub title: String,
    pub status_line: String,
    pub detail_lines: Vec<String>,
    pub pills: Vec<String>,
    pub primary_action: TodayCardAction,
    pub secondary_action: Option<TodayCardAction>,
    pub ranking_reason: String,
    pub ranking_features: Vec<String>,
    pub sync_state: SyncState,
    pub outbox_state: OutboxState,
    pub is_stale: bool,
    pub is_offline: bool,
}

#[uniffi::remote(Record)]
pub struct RelatedObjectRequirement {
    pub object_type: ObjectKind,
    pub relationship_label: String,
    pub is_required: bool,
}

#[uniffi::remote(Record)]
pub struct ValidationRequirement {
    pub id: String,
    pub label: String,
    pub is_blocking: bool,
}

#[uniffi::remote(Record)]
pub struct AddAction {
    pub action_type: AddActionType,
    pub display_label: String,
    pub allowed_context_types: Vec<ContextType>,
    pub required_authority: AuthorityGate,
    pub default_visibility: VisibilityClass,
    pub allowed_visibility_options: Vec<VisibilityClass>,
    pub created_or_updated_object_type: ObjectKind,
    pub related_object_requirements: Vec<RelatedObjectRequirement>,
    pub validation_requirements: Vec<ValidationRequirement>,
    pub supports_offline: bool,
    pub supports_draft: bool,
    pub outbox_behavior: OutboxBehavior,
    pub primary_submit_label: String,
    pub completion_state: AddFlowState,
}

#[uniffi::remote(Record)]
pub struct OutboxItem {
    pub id: String,
    pub action_type: AddActionType,
    pub context: ActiveContext,
    pub object_refs: Vec<ObjectRef>,
    pub event_refs: Vec<EventRef>,
    pub visibility: VisibilityClass,
    pub authority_gate: AuthorityGate,
    pub flow_state: AddFlowState,
    pub outbox_state: OutboxState,
    pub sync_state: SyncState,
    pub queued_at_unix: Option<u64>,
    pub last_attempt_at_unix: Option<u64>,
    pub retry_count: u32,
    pub last_error: Option<String>,
}

#[uniffi::remote(Record)]
pub struct OutboxRetryDecision {
    pub item_id: String,
    pub is_retryable: bool,
    pub authority_gate: AuthorityGate,
    pub reason: Option<String>,
}

#[uniffi::remote(Record)]
pub struct SearchResultSummary {
    pub id: String,
    pub object_ref: ObjectRef,
    pub primary_context: ActiveContext,
    pub title: String,
    pub subtitle: Option<String>,
    pub visibility: VisibilityClass,
    pub visibility_label: String,
    pub required_authority: AuthorityGate,
    pub sync_state: SyncState,
}

#[uniffi::remote(Record)]
pub struct PrototypePathStep {
    pub id: String,
    pub label: String,
    pub context: ActiveContext,
    pub action_type: Option<AddActionType>,
    pub object_ref: Option<ObjectRef>,
    pub authority_gate: AuthorityGate,
    pub visibility: VisibilityClass,
    pub outbox_state: OutboxState,
    pub sync_state: SyncState,
}

#[uniffi::remote(Record)]
pub struct PrototypePath {
    pub id: String,
    pub kind: PrototypePathKind,
    pub title: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub steps: Vec<PrototypePathStep>,
}

#[uniffi::remote(Record)]
pub struct RouteExecutionStep {
    pub id: String,
    pub kind: RouteExecutionStepKind,
    pub label: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub route_ref: ObjectRef,
    pub object_ref: Option<ObjectRef>,
    pub required_authority: AuthorityGate,
    pub visibility: VisibilityClass,
    pub supports_offline: bool,
    pub supports_partial_receipt: bool,
    pub uses_receipt_token: bool,
    pub outbox_state: OutboxState,
    pub sync_state: SyncState,
    pub detail_lines: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct RouteExecutionFlow {
    pub id: String,
    pub kind: RouteExecutionFlowKind,
    pub title: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub route_ref: ObjectRef,
    pub steps: Vec<RouteExecutionStep>,
}

#[uniffi::remote(Record)]
pub struct ProofProvenanceArtifact {
    pub id: String,
    pub kind: ProofProvenanceArtifactKind,
    pub review_state: ProofProvenanceReviewState,
    pub title: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub object_ref: ObjectRef,
    pub source_object_refs: Vec<ObjectRef>,
    pub required_authority: AuthorityGate,
    pub visibility: VisibilityClass,
    pub is_public_preview: bool,
    pub requires_redaction_review: bool,
    pub can_publish: bool,
    pub public_summary_lines: Vec<String>,
    pub redacted_field_labels: Vec<String>,
    pub outbox_state: OutboxState,
    pub sync_state: SyncState,
}

#[uniffi::remote(Record)]
pub struct StewardshipAccessItem {
    pub id: String,
    pub kind: StewardshipAccessItemKind,
    pub title: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub target_ref: ObjectRef,
    pub required_authority: AuthorityGate,
    pub visibility: VisibilityClass,
    pub is_admin_lite: bool,
    pub is_phase_2_deferred: bool,
    pub grants_private_access: bool,
    pub outbox_state: OutboxState,
    pub sync_state: SyncState,
    pub detail_lines: Vec<String>,
}
