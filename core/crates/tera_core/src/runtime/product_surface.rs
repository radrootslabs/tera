//! Phase 1 product-surface contracts for native Radroots clients.
//!
//! These DTOs are the shared Rust vocabulary for the iOS `Today | Add`
//! surface. They deliberately model workflow actors, visibility, authority,
//! Today cards, Add actions, object pages, and outbox state separately from
//! low-level Nostr protocol roles or legacy trade/listing APIs.

use serde::{Deserialize, Serialize};

use super::RadrootsRuntime;

/// Phase 1 workflow actors are product authority roles. Low-level protocol
/// roles such as Farmer, Buyer, Seller, and Service are compatibility roles and
/// are not sufficient authority for Phase 1 workflows.
pub const WORKFLOW_ACTOR_COMPATIBILITY_NOTE: &str = "Phase 1 workflow actors are product authority roles; low-level protocol \
     roles such as Farmer, Buyer, Seller, and Service are compatibility roles \
     and are not sufficient authority.";

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum OutboxBehavior {
    LocalOnly,
    QueueWhenOffline,
    RequireOnline,
    PublishWhenAuthorized,
    ShareWhenAuthorized,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PrototypePathKind {
    ProducerFoodToRoute,
    BuyerCommitmentToRoute,
    RouteCoordinatorAssignment,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RouteExecutionFlowKind {
    RoutePartnerAssignedStops,
    BuyerReceiptConfirmation,
    ExceptionRecovery,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RouteExecutionStepKind {
    AssignedRoute,
    PickupConfirmation,
    DropoffConfirmation,
    ReceiptConfirmation,
    ExceptionReport,
    RecoveryAction,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ProofProvenanceArtifactKind {
    ProofCompleteness,
    BuyerPacketDraft,
    BuyerPacketShared,
    PublicProvenancePreview,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SyncState {
    Unknown,
    Online,
    Offline,
    Syncing,
    Synced,
    Stale,
    Failed,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRef {
    pub object_type: ObjectKind,
    pub object_id: String,
    pub display_label: String,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRef {
    pub event_id: String,
    pub relay_url: Option<String>,
    pub kind: Option<u32>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveContext {
    pub context_type: ContextType,
    pub context_ref: ObjectRef,
    pub actor: WorkflowActor,
    pub display_label: String,
    pub visibility_scope: VisibilityClass,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorityGate {
    pub domain: AuthorityDomain,
    pub action: AuthorityAction,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub is_required: bool,
    pub is_allowed: bool,
    pub reason: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayCardAction {
    pub id: String,
    pub label: String,
    pub action_type: Option<AddActionType>,
    pub target_object: Option<ObjectRef>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedObjectRequirement {
    pub object_type: ObjectKind,
    pub relationship_label: String,
    pub is_required: bool,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRequirement {
    pub id: String,
    pub label: String,
    pub is_blocking: bool,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxRetryDecision {
    pub item_id: String,
    pub is_retryable: bool,
    pub authority_gate: AuthorityGate,
    pub reason: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrototypePath {
    pub id: String,
    pub kind: PrototypePathKind,
    pub title: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub steps: Vec<PrototypePathStep>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteExecutionFlow {
    pub id: String,
    pub kind: RouteExecutionFlowKind,
    pub title: String,
    pub actor: WorkflowActor,
    pub context: ActiveContext,
    pub route_ref: ObjectRef,
    pub steps: Vec<RouteExecutionStep>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub const CANONICAL_CONTEXT_TYPES: [ContextType; 10] = [
    ContextType::Regional,
    ContextType::Network,
    ContextType::Farm,
    ContextType::Buyer,
    ContextType::Route,
    ContextType::RoutePartner,
    ContextType::PickupPoint,
    ContextType::TraceRecords,
    ContextType::Hub,
    ContextType::NetworkSteward,
];

pub const CANONICAL_WORKFLOW_ACTORS: [WorkflowActor; 11] = [
    WorkflowActor::NetworkMember,
    WorkflowActor::ProducerAdmin,
    WorkflowActor::FarmTeamMember,
    WorkflowActor::HubOperator,
    WorkflowActor::TraceLead,
    WorkflowActor::BuyerSourcingLead,
    WorkflowActor::BuyerReceiver,
    WorkflowActor::PickupPointCoordinator,
    WorkflowActor::RouteCoordinator,
    WorkflowActor::RoutePartner,
    WorkflowActor::NetworkSteward,
];

pub const CANONICAL_VISIBILITY_CLASSES: [VisibilityClass; 9] = [
    VisibilityClass::LocalDraft,
    VisibilityClass::FarmPrivate,
    VisibilityClass::WorkspacePrivate,
    VisibilityClass::NetworkVisible,
    VisibilityClass::RouteScoped,
    VisibilityClass::BuyerScoped,
    VisibilityClass::PublicCommunity,
    VisibilityClass::PublicProvenance,
    VisibilityClass::SecretNeverShared,
];

pub const CANONICAL_AUTHORITY_DOMAINS: [AuthorityDomain; 9] = [
    AuthorityDomain::RelayGroupAccess,
    AuthorityDomain::FarmWorkspaceOperations,
    AuthorityDomain::BuyerWorkspace,
    AuthorityDomain::RouteCoordination,
    AuthorityDomain::RouteExecution,
    AuthorityDomain::Receipt,
    AuthorityDomain::TraceProof,
    AuthorityDomain::PublicPublishing,
    AuthorityDomain::NetworkStewardship,
];

pub const CANONICAL_TODAY_CARD_TYPES: [TodayCardType; 12] = [
    TodayCardType::Route,
    TodayCardType::Food,
    TodayCardType::Ask,
    TodayCardType::Event,
    TodayCardType::Place,
    TodayCardType::Task,
    TodayCardType::Proof,
    TodayCardType::Exception,
    TodayCardType::Provenance,
    TodayCardType::Update,
    TodayCardType::AccessMembership,
    TodayCardType::SyncOutbox,
];

pub const TODAY_CARD_RANKING_PRIORITY: [TodayCardType; 12] = [
    TodayCardType::Exception,
    TodayCardType::SyncOutbox,
    TodayCardType::Route,
    TodayCardType::Proof,
    TodayCardType::Food,
    TodayCardType::Task,
    TodayCardType::Provenance,
    TodayCardType::Ask,
    TodayCardType::Event,
    TodayCardType::Place,
    TodayCardType::Update,
    TodayCardType::AccessMembership,
];

pub const CANONICAL_ADD_ACTION_TYPES: [AddActionType; 18] = [
    AddActionType::Photo,
    AddActionType::Note,
    AddActionType::Ask,
    AddActionType::Scan,
    AddActionType::Food,
    AddActionType::Harvest,
    AddActionType::BuyerRequest,
    AddActionType::BuyerCommitment,
    AddActionType::RouteNeed,
    AddActionType::RouteStop,
    AddActionType::PickupEvent,
    AddActionType::Place,
    AddActionType::Proof,
    AddActionType::Exception,
    AddActionType::PublicUpdate,
    AddActionType::Provenance,
    AddActionType::MemberInvite,
    AddActionType::Correction,
];

pub const CANONICAL_ADD_FLOW_STATES: [AddFlowState; 16] = [
    AddFlowState::NotStarted,
    AddFlowState::Draft,
    AddFlowState::Editing,
    AddFlowState::ValidationFailed,
    AddFlowState::ReadyToSubmit,
    AddFlowState::Submitted,
    AddFlowState::Queued,
    AddFlowState::Syncing,
    AddFlowState::NeedsApproval,
    AddFlowState::Approved,
    AddFlowState::Published,
    AddFlowState::Shared,
    AddFlowState::Confirmed,
    AddFlowState::Failed,
    AddFlowState::Conflict,
    AddFlowState::Discarded,
];

pub const CANONICAL_OBJECT_PAGE_FAMILIES: [ObjectPageFamily; 13] = [
    ObjectPageFamily::Network,
    ObjectPageFamily::NetworkRoute,
    ObjectPageFamily::FarmWorkspace,
    ObjectPageFamily::FarmPublicProfile,
    ObjectPageFamily::BuyerWorkspace,
    ObjectPageFamily::PickupPointPlace,
    ObjectPageFamily::Food,
    ObjectPageFamily::Event,
    ObjectPageFamily::RouteStop,
    ObjectPageFamily::Proof,
    ObjectPageFamily::BuyerPacket,
    ObjectPageFamily::PublicProvenance,
    ObjectPageFamily::Exception,
];

pub const CANONICAL_OUTBOX_STATES: [OutboxState; 10] = [
    OutboxState::NotQueued,
    OutboxState::Draft,
    OutboxState::Queued,
    OutboxState::Syncing,
    OutboxState::AwaitingAuthority,
    OutboxState::Published,
    OutboxState::Shared,
    OutboxState::Failed,
    OutboxState::Conflict,
    OutboxState::Discarded,
];

pub const CANONICAL_SYNC_STATES: [SyncState; 7] = [
    SyncState::Unknown,
    SyncState::Online,
    SyncState::Offline,
    SyncState::Syncing,
    SyncState::Synced,
    SyncState::Stale,
    SyncState::Failed,
];

pub const CANONICAL_ROUTE_EXECUTION_FLOW_KINDS: [RouteExecutionFlowKind; 3] = [
    RouteExecutionFlowKind::RoutePartnerAssignedStops,
    RouteExecutionFlowKind::BuyerReceiptConfirmation,
    RouteExecutionFlowKind::ExceptionRecovery,
];

pub const CANONICAL_ROUTE_EXECUTION_STEP_KINDS: [RouteExecutionStepKind; 6] = [
    RouteExecutionStepKind::AssignedRoute,
    RouteExecutionStepKind::PickupConfirmation,
    RouteExecutionStepKind::DropoffConfirmation,
    RouteExecutionStepKind::ReceiptConfirmation,
    RouteExecutionStepKind::ExceptionReport,
    RouteExecutionStepKind::RecoveryAction,
];

pub const CANONICAL_PROOF_PROVENANCE_ARTIFACT_KINDS: [ProofProvenanceArtifactKind; 4] = [
    ProofProvenanceArtifactKind::ProofCompleteness,
    ProofProvenanceArtifactKind::BuyerPacketDraft,
    ProofProvenanceArtifactKind::BuyerPacketShared,
    ProofProvenanceArtifactKind::PublicProvenancePreview,
];

pub const CANONICAL_PROOF_PROVENANCE_REVIEW_STATES: [ProofProvenanceReviewState; 8] = [
    ProofProvenanceReviewState::MissingProof,
    ProofProvenanceReviewState::Complete,
    ProofProvenanceReviewState::Draft,
    ProofProvenanceReviewState::ReadyToShare,
    ProofProvenanceReviewState::Shared,
    ProofProvenanceReviewState::RedactionRequired,
    ProofProvenanceReviewState::ReadyToPublish,
    ProofProvenanceReviewState::Published,
];

pub const CANONICAL_STEWARDSHIP_ACCESS_ITEM_KINDS: [StewardshipAccessItemKind; 9] = [
    StewardshipAccessItemKind::AccessRequestReview,
    StewardshipAccessItemKind::RoleApproval,
    StewardshipAccessItemKind::RoutePartnerInvite,
    StewardshipAccessItemKind::RoutePoolMetadata,
    StewardshipAccessItemKind::PublicModeration,
    StewardshipAccessItemKind::InviteAcceptance,
    StewardshipAccessItemKind::RequestAccess,
    StewardshipAccessItemKind::AccessDenied,
    StewardshipAccessItemKind::GroupManagementDeferred,
];

fn object_ref(
    object_type: ObjectKind,
    object_id: impl Into<String>,
    label: impl Into<String>,
) -> ObjectRef {
    ObjectRef {
        object_type,
        object_id: object_id.into(),
        display_label: label.into(),
    }
}

fn context_fixture_parts(
    context_type: ContextType,
) -> (
    ObjectKind,
    &'static str,
    &'static str,
    WorkflowActor,
    VisibilityClass,
) {
    match context_type {
        ContextType::Regional => (
            ObjectKind::Region,
            "region_floripa",
            "Floripa regional food network",
            WorkflowActor::NetworkMember,
            VisibilityClass::PublicCommunity,
        ),
        ContextType::Network => (
            ObjectKind::Network,
            "network_floripa",
            "Floripa local food network",
            WorkflowActor::NetworkMember,
            VisibilityClass::NetworkVisible,
        ),
        ContextType::Farm => (
            ObjectKind::Farm,
            "farm_floripa_001",
            "Floripa Farm",
            WorkflowActor::ProducerAdmin,
            VisibilityClass::FarmPrivate,
        ),
        ContextType::Buyer => (
            ObjectKind::BuyerWorkspace,
            "buyer_workspace_001",
            "Kitchen buyer workspace",
            WorkflowActor::BuyerSourcingLead,
            VisibilityClass::BuyerScoped,
        ),
        ContextType::Route => (
            ObjectKind::Route,
            "route_thursday_001",
            "Thursday network loop",
            WorkflowActor::RouteCoordinator,
            VisibilityClass::RouteScoped,
        ),
        ContextType::RoutePartner => (
            ObjectKind::RoutePartner,
            "route_partner_001",
            "Assigned route partner",
            WorkflowActor::RoutePartner,
            VisibilityClass::RouteScoped,
        ),
        ContextType::PickupPoint => (
            ObjectKind::PickupPoint,
            "pickup_point_001",
            "Neighborhood pickup point",
            WorkflowActor::PickupPointCoordinator,
            VisibilityClass::NetworkVisible,
        ),
        ContextType::TraceRecords => (
            ObjectKind::Proof,
            "trace_records_001",
            "Trace and records",
            WorkflowActor::TraceLead,
            VisibilityClass::WorkspacePrivate,
        ),
        ContextType::Hub => (
            ObjectKind::Hub,
            "hub_001",
            "Floripa hub",
            WorkflowActor::HubOperator,
            VisibilityClass::WorkspacePrivate,
        ),
        ContextType::NetworkSteward => (
            ObjectKind::AccessMembership,
            "network_stewardship_001",
            "Network stewardship",
            WorkflowActor::NetworkSteward,
            VisibilityClass::WorkspacePrivate,
        ),
    }
}

fn context_for_type(context_type: ContextType) -> ActiveContext {
    let (object_type, object_id, label, actor, visibility_scope) =
        context_fixture_parts(context_type);
    ActiveContext {
        context_type,
        context_ref: object_ref(object_type, object_id, label),
        actor,
        display_label: label.to_string(),
        visibility_scope,
    }
}

fn authority_allowed(actor: WorkflowActor, domain: AuthorityDomain) -> bool {
    matches!(
        (actor, domain),
        (
            WorkflowActor::NetworkMember,
            AuthorityDomain::RelayGroupAccess
        ) | (
            WorkflowActor::ProducerAdmin,
            AuthorityDomain::FarmWorkspaceOperations
        ) | (WorkflowActor::ProducerAdmin, AuthorityDomain::TraceProof)
            | (
                WorkflowActor::ProducerAdmin,
                AuthorityDomain::PublicPublishing
            )
            | (
                WorkflowActor::FarmTeamMember,
                AuthorityDomain::FarmWorkspaceOperations
            )
            | (
                WorkflowActor::HubOperator,
                AuthorityDomain::FarmWorkspaceOperations
            )
            | (WorkflowActor::HubOperator, AuthorityDomain::RouteExecution)
            | (WorkflowActor::TraceLead, AuthorityDomain::TraceProof)
            | (
                WorkflowActor::BuyerSourcingLead,
                AuthorityDomain::BuyerWorkspace
            )
            | (WorkflowActor::BuyerReceiver, AuthorityDomain::Receipt)
            | (
                WorkflowActor::PickupPointCoordinator,
                AuthorityDomain::Receipt
            )
            | (
                WorkflowActor::PickupPointCoordinator,
                AuthorityDomain::RouteExecution
            )
            | (
                WorkflowActor::RouteCoordinator,
                AuthorityDomain::RouteCoordination
            )
            | (WorkflowActor::RoutePartner, AuthorityDomain::RouteExecution)
            | (
                WorkflowActor::NetworkSteward,
                AuthorityDomain::RelayGroupAccess
            )
            | (
                WorkflowActor::NetworkSteward,
                AuthorityDomain::PublicPublishing
            )
            | (
                WorkflowActor::NetworkSteward,
                AuthorityDomain::NetworkStewardship
            )
    )
}

pub fn fixture_authority_gate(
    actor: WorkflowActor,
    context: ActiveContext,
    domain: AuthorityDomain,
    action: AuthorityAction,
) -> AuthorityGate {
    let is_allowed = authority_allowed(actor, domain);
    AuthorityGate {
        domain,
        action,
        actor,
        context,
        is_required: true,
        is_allowed,
        reason: if is_allowed {
            None
        } else {
            Some("fixture authority denies this actor/domain pair".to_string())
        },
    }
}

pub fn fixture_active_contexts() -> Vec<ActiveContext> {
    CANONICAL_CONTEXT_TYPES
        .into_iter()
        .map(context_for_type)
        .collect()
}

fn context_by_object_id(context_id: Option<String>) -> Option<ActiveContext> {
    let context_id = context_id?;
    fixture_active_contexts()
        .into_iter()
        .find(|context| context.context_ref.object_id == context_id)
}

fn card_action_for(card_type: TodayCardType) -> Option<AddActionType> {
    match card_type {
        TodayCardType::Route => Some(AddActionType::RouteStop),
        TodayCardType::Food => Some(AddActionType::Food),
        TodayCardType::Ask => Some(AddActionType::Ask),
        TodayCardType::Event => Some(AddActionType::PickupEvent),
        TodayCardType::Place => Some(AddActionType::Place),
        TodayCardType::Task => Some(AddActionType::Note),
        TodayCardType::Proof => Some(AddActionType::Proof),
        TodayCardType::Exception => Some(AddActionType::Exception),
        TodayCardType::Provenance => Some(AddActionType::Provenance),
        TodayCardType::Update => Some(AddActionType::PublicUpdate),
        TodayCardType::AccessMembership => Some(AddActionType::MemberInvite),
        TodayCardType::SyncOutbox => None,
    }
}

fn object_kind_for_card(card_type: TodayCardType) -> ObjectKind {
    match card_type {
        TodayCardType::Route => ObjectKind::Route,
        TodayCardType::Food => ObjectKind::Food,
        TodayCardType::Ask => ObjectKind::Ask,
        TodayCardType::Event => ObjectKind::Event,
        TodayCardType::Place => ObjectKind::Place,
        TodayCardType::Task => ObjectKind::Task,
        TodayCardType::Proof => ObjectKind::Proof,
        TodayCardType::Exception => ObjectKind::Exception,
        TodayCardType::Provenance => ObjectKind::Provenance,
        TodayCardType::Update => ObjectKind::Update,
        TodayCardType::AccessMembership => ObjectKind::AccessMembership,
        TodayCardType::SyncOutbox => ObjectKind::OutboxItem,
    }
}

fn ranking_reason_for(card_type: TodayCardType) -> &'static str {
    match card_type {
        TodayCardType::Exception => "blocking exception",
        TodayCardType::SyncOutbox => "required sync action",
        TodayCardType::Route => "time-window route operation",
        TodayCardType::Proof => "route readiness and proof gap",
        TodayCardType::Food => "commitment window",
        TodayCardType::Task => "assigned task",
        TodayCardType::Provenance => "provenance candidate",
        TodayCardType::Ask => "food availability and ask",
        TodayCardType::Event => "event window",
        TodayCardType::Place => "place context",
        TodayCardType::Update => "community update",
        TodayCardType::AccessMembership => "network access state",
    }
}

fn ranking_features_for(card_type: TodayCardType) -> Vec<String> {
    match card_type {
        TodayCardType::Exception => vec!["blocking".to_string(), "recovery".to_string()],
        TodayCardType::SyncOutbox => vec!["outbox".to_string(), "retry".to_string()],
        TodayCardType::Route => vec!["time_window".to_string(), "route".to_string()],
        TodayCardType::Proof => vec!["proof_gap".to_string(), "trace".to_string()],
        TodayCardType::Food => vec!["commitment".to_string(), "availability".to_string()],
        TodayCardType::Task => vec!["assigned".to_string(), "work".to_string()],
        TodayCardType::Provenance => vec!["candidate".to_string(), "public_review".to_string()],
        TodayCardType::Ask => vec!["ask".to_string(), "network_need".to_string()],
        TodayCardType::Event => vec!["event".to_string(), "calendar".to_string()],
        TodayCardType::Place => vec!["place".to_string(), "local_context".to_string()],
        TodayCardType::Update => vec!["update".to_string(), "community".to_string()],
        TodayCardType::AccessMembership => vec!["access".to_string(), "membership".to_string()],
    }
}

fn status_line_for(card_type: TodayCardType) -> &'static str {
    match card_type {
        TodayCardType::Exception => "Needs review",
        TodayCardType::SyncOutbox => "Retry required",
        TodayCardType::Route => "Route window open",
        TodayCardType::Proof => "Proof gap detected",
        TodayCardType::Food => "Commitment window active",
        TodayCardType::Task => "Assigned work ready",
        TodayCardType::Provenance => "Candidate ready for review",
        TodayCardType::Ask => "Network need available",
        TodayCardType::Event => "Upcoming gathering",
        TodayCardType::Place => "Place context updated",
        TodayCardType::Update => "Community update ready",
        TodayCardType::AccessMembership => "Membership state available",
    }
}

fn detail_lines_for(card_type: TodayCardType) -> Vec<String> {
    match card_type {
        TodayCardType::Exception => vec![
            "Resolve the blocker before related route work continues.".to_string(),
            "Recovery path remains scoped to the active context.".to_string(),
        ],
        TodayCardType::SyncOutbox => {
            vec!["Queued work will re-check context, visibility, and authority.".to_string()]
        }
        TodayCardType::Route => {
            vec!["Review stops, timing, proof gaps, and assigned route partner state.".to_string()]
        }
        TodayCardType::Proof => {
            vec!["Trace record needs proof completion before publication.".to_string()]
        }
        TodayCardType::Food => {
            vec!["Food availability is connected to commitments and routes.".to_string()]
        }
        TodayCardType::Task => vec!["Complete the assigned task from this context.".to_string()],
        TodayCardType::Provenance => {
            vec!["Public provenance preview requires redaction review.".to_string()]
        }
        TodayCardType::Ask => vec!["Respond to a scoped network ask.".to_string()],
        TodayCardType::Event => vec!["Gathering context is visible to the network.".to_string()],
        TodayCardType::Place => {
            vec!["Place details are ready for local network review.".to_string()]
        }
        TodayCardType::Update => vec!["Share a context-aware network update.".to_string()],
        TodayCardType::AccessMembership => {
            vec!["Review member access for this context.".to_string()]
        }
    }
}

pub fn fixture_today_cards(context_id: Option<String>) -> Vec<TodayCard> {
    let contexts = fixture_active_contexts();
    let filter_context = context_by_object_id(context_id);
    TODAY_CARD_RANKING_PRIORITY
        .into_iter()
        .enumerate()
        .filter_map(|(index, card_type)| {
            let context = filter_context
                .clone()
                .unwrap_or_else(|| contexts[index % contexts.len()].clone());
            let object_kind = object_kind_for_card(card_type);
            let object_id = format!("phase1_{:?}_001", object_kind).to_lowercase();
            let object = object_ref(object_kind, object_id, format!("{:?} fixture", card_type));
            let action_type = card_action_for(card_type);
            Some(TodayCard {
                id: format!("today_{:?}_001", card_type).to_lowercase(),
                card_type,
                source_object_refs: vec![object.clone()],
                source_event_refs: Vec::new(),
                primary_context: context.clone(),
                actor: context.actor,
                visibility: context.visibility_scope,
                visibility_label: format!("{:?}", context.visibility_scope),
                title: format!("{:?} fixture card", card_type),
                status_line: status_line_for(card_type).to_string(),
                detail_lines: detail_lines_for(card_type),
                pills: vec![
                    format!("{:?}", card_type),
                    ranking_reason_for(card_type).to_string(),
                ],
                primary_action: TodayCardAction {
                    id: format!("primary_{:?}", card_type).to_lowercase(),
                    label: action_type
                        .map(|action| format!("{:?}", action))
                        .unwrap_or_else(|| "Review".to_string()),
                    action_type,
                    target_object: Some(object),
                },
                secondary_action: None,
                ranking_reason: ranking_reason_for(card_type).to_string(),
                ranking_features: ranking_features_for(card_type),
                sync_state: if matches!(card_type, TodayCardType::SyncOutbox) {
                    SyncState::Failed
                } else {
                    SyncState::Online
                },
                outbox_state: if matches!(card_type, TodayCardType::SyncOutbox) {
                    OutboxState::Failed
                } else {
                    OutboxState::NotQueued
                },
                is_stale: matches!(card_type, TodayCardType::Proof),
                is_offline: matches!(card_type, TodayCardType::SyncOutbox),
            })
        })
        .collect()
}

fn add_action_object_kind(action_type: AddActionType) -> ObjectKind {
    match action_type {
        AddActionType::Photo | AddActionType::Note | AddActionType::PublicUpdate => {
            ObjectKind::Update
        }
        AddActionType::Ask | AddActionType::BuyerRequest | AddActionType::RouteNeed => {
            ObjectKind::Ask
        }
        AddActionType::Scan | AddActionType::Proof => ObjectKind::Proof,
        AddActionType::Food | AddActionType::Harvest | AddActionType::BuyerCommitment => {
            ObjectKind::Food
        }
        AddActionType::RouteStop | AddActionType::PickupEvent => ObjectKind::RouteStop,
        AddActionType::Place => ObjectKind::Place,
        AddActionType::Exception => ObjectKind::Exception,
        AddActionType::Provenance => ObjectKind::Provenance,
        AddActionType::MemberInvite => ObjectKind::AccessMembership,
        AddActionType::Correction => ObjectKind::Correction,
    }
}

fn add_action_authority(action_type: AddActionType) -> (AuthorityDomain, AuthorityAction) {
    match action_type {
        AddActionType::PublicUpdate | AddActionType::Provenance => {
            (AuthorityDomain::PublicPublishing, AuthorityAction::Publish)
        }
        AddActionType::BuyerRequest | AddActionType::BuyerCommitment => {
            (AuthorityDomain::BuyerWorkspace, AuthorityAction::Submit)
        }
        AddActionType::RouteNeed | AddActionType::RouteStop | AddActionType::PickupEvent => {
            (AuthorityDomain::RouteCoordination, AuthorityAction::Submit)
        }
        AddActionType::Proof | AddActionType::Scan => {
            (AuthorityDomain::TraceProof, AuthorityAction::Submit)
        }
        AddActionType::MemberInvite => (AuthorityDomain::RelayGroupAccess, AuthorityAction::Share),
        AddActionType::Correction => (AuthorityDomain::TraceProof, AuthorityAction::Correct),
        _ => (
            AuthorityDomain::FarmWorkspaceOperations,
            AuthorityAction::Submit,
        ),
    }
}

fn default_visibility_for_action(action_type: AddActionType) -> VisibilityClass {
    match action_type {
        AddActionType::PublicUpdate => VisibilityClass::PublicCommunity,
        AddActionType::Provenance => VisibilityClass::PublicProvenance,
        AddActionType::BuyerRequest | AddActionType::BuyerCommitment => {
            VisibilityClass::BuyerScoped
        }
        AddActionType::RouteNeed | AddActionType::RouteStop | AddActionType::PickupEvent => {
            VisibilityClass::RouteScoped
        }
        AddActionType::Photo | AddActionType::Note | AddActionType::Scan => {
            VisibilityClass::LocalDraft
        }
        _ => VisibilityClass::FarmPrivate,
    }
}

pub fn fixture_add_actions(context_id: Option<String>) -> Vec<AddAction> {
    let context =
        context_by_object_id(context_id).unwrap_or_else(|| context_for_type(ContextType::Farm));
    CANONICAL_ADD_ACTION_TYPES
        .into_iter()
        .map(|action_type| {
            let (domain, action) = add_action_authority(action_type);
            AddAction {
                action_type,
                display_label: format!("{:?}", action_type),
                allowed_context_types: vec![context.context_type],
                required_authority: fixture_authority_gate(
                    context.actor,
                    context.clone(),
                    domain,
                    action,
                ),
                default_visibility: default_visibility_for_action(action_type),
                allowed_visibility_options: vec![
                    VisibilityClass::LocalDraft,
                    default_visibility_for_action(action_type),
                ],
                created_or_updated_object_type: add_action_object_kind(action_type),
                related_object_requirements: vec![RelatedObjectRequirement {
                    object_type: context.context_ref.object_type,
                    relationship_label: "primary context".to_string(),
                    is_required: true,
                }],
                validation_requirements: vec![ValidationRequirement {
                    id: "fixture_required_fields".to_string(),
                    label: "Required fields are present".to_string(),
                    is_blocking: true,
                }],
                supports_offline: true,
                supports_draft: true,
                outbox_behavior: OutboxBehavior::QueueWhenOffline,
                primary_submit_label: "Submit".to_string(),
                completion_state: AddFlowState::ReadyToSubmit,
            }
        })
        .collect()
}

fn object_kind_for_page(family: ObjectPageFamily) -> ObjectKind {
    match family {
        ObjectPageFamily::Network => ObjectKind::Network,
        ObjectPageFamily::NetworkRoute => ObjectKind::Route,
        ObjectPageFamily::FarmWorkspace | ObjectPageFamily::FarmPublicProfile => ObjectKind::Farm,
        ObjectPageFamily::BuyerWorkspace => ObjectKind::BuyerWorkspace,
        ObjectPageFamily::PickupPointPlace => ObjectKind::PickupPoint,
        ObjectPageFamily::Food => ObjectKind::Food,
        ObjectPageFamily::Event => ObjectKind::Event,
        ObjectPageFamily::RouteStop => ObjectKind::RouteStop,
        ObjectPageFamily::Proof => ObjectKind::Proof,
        ObjectPageFamily::BuyerPacket => ObjectKind::BuyerPacket,
        ObjectPageFamily::PublicProvenance => ObjectKind::Provenance,
        ObjectPageFamily::Exception => ObjectKind::Exception,
    }
}

pub fn fixture_object_page_summaries(context_id: Option<String>) -> Vec<ObjectPageSummary> {
    let context =
        context_by_object_id(context_id).unwrap_or_else(|| context_for_type(ContextType::Network));
    CANONICAL_OBJECT_PAGE_FAMILIES
        .into_iter()
        .map(|family| {
            let object_kind = object_kind_for_page(family);
            ObjectPageSummary {
                object_ref: object_ref(
                    object_kind,
                    format!("phase1_{:?}_page_001", family).to_lowercase(),
                    format!("{:?} fixture page", family),
                ),
                family,
                primary_context: context.clone(),
                title: format!("{:?} fixture page", family),
                subtitle: Some("fixture-backed object summary".to_string()),
                visibility: context.visibility_scope,
                visibility_label: format!("{:?}", context.visibility_scope),
                required_authority: fixture_authority_gate(
                    context.actor,
                    context.clone(),
                    AuthorityDomain::RelayGroupAccess,
                    AuthorityAction::NavigateRelatedObject,
                ),
                sync_state: SyncState::Online,
            }
        })
        .collect()
}

fn visibility_allows_search_result(visibility: VisibilityClass) -> bool {
    matches!(
        visibility,
        VisibilityClass::NetworkVisible
            | VisibilityClass::PublicCommunity
            | VisibilityClass::PublicProvenance
    )
}

fn object_kind_allows_search_result(object_kind: ObjectKind, visibility: VisibilityClass) -> bool {
    match object_kind {
        ObjectKind::BuyerPacket | ObjectKind::RouteStop => false,
        ObjectKind::Proof => visibility == VisibilityClass::PublicProvenance,
        _ => true,
    }
}

pub fn fixture_search_results(
    query: Option<String>,
    context_id: Option<String>,
) -> Vec<SearchResultSummary> {
    let normalized_query = query.unwrap_or_default().trim().to_lowercase();
    fixture_object_page_summaries(context_id)
        .into_iter()
        .filter(|page| page.required_authority.is_allowed)
        .filter(|page| visibility_allows_search_result(page.visibility))
        .filter(|page| {
            object_kind_allows_search_result(page.object_ref.object_type, page.visibility)
        })
        .filter(|page| {
            normalized_query.is_empty()
                || page.title.to_lowercase().contains(&normalized_query)
                || page
                    .object_ref
                    .display_label
                    .to_lowercase()
                    .contains(&normalized_query)
                || format!("{:?}", page.family)
                    .to_lowercase()
                    .contains(&normalized_query)
        })
        .map(|page| SearchResultSummary {
            id: format!("search_{}", page.object_ref.object_id),
            object_ref: page.object_ref,
            primary_context: page.primary_context,
            title: page.title,
            subtitle: page.subtitle,
            visibility: page.visibility,
            visibility_label: page.visibility_label,
            required_authority: page.required_authority,
            sync_state: page.sync_state,
        })
        .collect()
}

fn prototype_path_step(
    id: impl Into<String>,
    label: impl Into<String>,
    context: ActiveContext,
    action_type: Option<AddActionType>,
    object_ref: Option<ObjectRef>,
    domain: AuthorityDomain,
    action: AuthorityAction,
    visibility: VisibilityClass,
    outbox_state: OutboxState,
    sync_state: SyncState,
) -> PrototypePathStep {
    PrototypePathStep {
        id: id.into(),
        label: label.into(),
        context: context.clone(),
        action_type,
        object_ref,
        authority_gate: fixture_authority_gate(context.actor, context, domain, action),
        visibility,
        outbox_state,
        sync_state,
    }
}

pub fn fixture_prototype_paths() -> Vec<PrototypePath> {
    let farm = context_for_type(ContextType::Farm);
    let buyer = context_for_type(ContextType::Buyer);
    let route = context_for_type(ContextType::Route);
    let route_partner = context_for_type(ContextType::RoutePartner);
    let food_ref = object_ref(ObjectKind::Food, "food_harvest_001", "Summer squash lot");
    let route_ref = object_ref(
        ObjectKind::Route,
        "route_thursday_001",
        "Thursday network loop",
    );
    let buyer_request_ref = object_ref(
        ObjectKind::BuyerPacket,
        "buyer_commitment_001",
        "Kitchen commitment packet",
    );
    let exception_ref = object_ref(
        ObjectKind::Exception,
        "route_blocker_001",
        "Missing pickup confirmation",
    );

    vec![
        PrototypePath {
            id: "producer_food_to_route".to_string(),
            kind: PrototypePathKind::ProducerFoodToRoute,
            title: "Producer food to route".to_string(),
            actor: farm.actor,
            context: farm.clone(),
            steps: vec![
                prototype_path_step(
                    "producer_today",
                    "Farm Today",
                    farm.clone(),
                    None,
                    Some(farm.context_ref.clone()),
                    AuthorityDomain::FarmWorkspaceOperations,
                    AuthorityAction::Search,
                    farm.visibility_scope,
                    OutboxState::NotQueued,
                    SyncState::Online,
                ),
                prototype_path_step(
                    "producer_add_food",
                    "Add Food",
                    farm.clone(),
                    Some(AddActionType::Food),
                    Some(food_ref.clone()),
                    AuthorityDomain::FarmWorkspaceOperations,
                    AuthorityAction::Submit,
                    VisibilityClass::NetworkVisible,
                    OutboxState::Draft,
                    SyncState::Offline,
                ),
                prototype_path_step(
                    "producer_add_to_route",
                    "Add to Route",
                    farm.clone(),
                    Some(AddActionType::RouteNeed),
                    Some(route_ref.clone()),
                    AuthorityDomain::FarmWorkspaceOperations,
                    AuthorityAction::Share,
                    VisibilityClass::RouteScoped,
                    OutboxState::Queued,
                    SyncState::Syncing,
                ),
                prototype_path_step(
                    "producer_food_page",
                    "Food page",
                    farm.clone(),
                    None,
                    Some(food_ref.clone()),
                    AuthorityDomain::FarmWorkspaceOperations,
                    AuthorityAction::NavigateRelatedObject,
                    VisibilityClass::NetworkVisible,
                    OutboxState::Shared,
                    SyncState::Synced,
                ),
                prototype_path_step(
                    "producer_route_page",
                    "Route page",
                    route.clone(),
                    None,
                    Some(route_ref.clone()),
                    AuthorityDomain::RouteCoordination,
                    AuthorityAction::NavigateRelatedObject,
                    VisibilityClass::RouteScoped,
                    OutboxState::NotQueued,
                    SyncState::Online,
                ),
            ],
        },
        PrototypePath {
            id: "buyer_commitment_to_route".to_string(),
            kind: PrototypePathKind::BuyerCommitmentToRoute,
            title: "Buyer commitment to route".to_string(),
            actor: buyer.actor,
            context: buyer.clone(),
            steps: vec![
                prototype_path_step(
                    "buyer_today",
                    "Buyer Today",
                    buyer.clone(),
                    None,
                    Some(buyer.context_ref.clone()),
                    AuthorityDomain::BuyerWorkspace,
                    AuthorityAction::Search,
                    buyer.visibility_scope,
                    OutboxState::NotQueued,
                    SyncState::Online,
                ),
                prototype_path_step(
                    "buyer_request",
                    "Add Buyer Request",
                    buyer.clone(),
                    Some(AddActionType::BuyerRequest),
                    Some(buyer_request_ref.clone()),
                    AuthorityDomain::BuyerWorkspace,
                    AuthorityAction::Submit,
                    VisibilityClass::BuyerScoped,
                    OutboxState::Draft,
                    SyncState::Offline,
                ),
                prototype_path_step(
                    "buyer_commitment",
                    "Confirm Commitment",
                    buyer.clone(),
                    Some(AddActionType::BuyerCommitment),
                    Some(buyer_request_ref),
                    AuthorityDomain::BuyerWorkspace,
                    AuthorityAction::Approve,
                    VisibilityClass::BuyerScoped,
                    OutboxState::Queued,
                    SyncState::Syncing,
                ),
                prototype_path_step(
                    "buyer_route",
                    "Route",
                    route.clone(),
                    None,
                    Some(route_ref.clone()),
                    AuthorityDomain::RouteCoordination,
                    AuthorityAction::NavigateRelatedObject,
                    VisibilityClass::RouteScoped,
                    OutboxState::Shared,
                    SyncState::Synced,
                ),
            ],
        },
        PrototypePath {
            id: "route_coordinator_assignment".to_string(),
            kind: PrototypePathKind::RouteCoordinatorAssignment,
            title: "Route coordinator assignment".to_string(),
            actor: route.actor,
            context: route.clone(),
            steps: vec![
                prototype_path_step(
                    "route_today",
                    "Route Today",
                    route.clone(),
                    None,
                    Some(route_ref.clone()),
                    AuthorityDomain::RouteCoordination,
                    AuthorityAction::Search,
                    VisibilityClass::RouteScoped,
                    OutboxState::NotQueued,
                    SyncState::Online,
                ),
                prototype_path_step(
                    "route_page",
                    "Route page",
                    route.clone(),
                    None,
                    Some(route_ref),
                    AuthorityDomain::RouteCoordination,
                    AuthorityAction::NavigateRelatedObject,
                    VisibilityClass::RouteScoped,
                    OutboxState::NotQueued,
                    SyncState::Online,
                ),
                prototype_path_step(
                    "route_resolve_blocker",
                    "Resolve blocker",
                    route.clone(),
                    Some(AddActionType::Exception),
                    Some(exception_ref),
                    AuthorityDomain::RouteCoordination,
                    AuthorityAction::Close,
                    VisibilityClass::RouteScoped,
                    OutboxState::Conflict,
                    SyncState::Failed,
                ),
                prototype_path_step(
                    "route_assign_partner",
                    "Assign RoutePartner",
                    route,
                    Some(AddActionType::RouteNeed),
                    Some(route_partner.context_ref.clone()),
                    AuthorityDomain::RouteCoordination,
                    AuthorityAction::Assign,
                    VisibilityClass::RouteScoped,
                    OutboxState::Queued,
                    SyncState::Syncing,
                ),
            ],
        },
    ]
}

struct RouteExecutionStepFixture {
    id: &'static str,
    kind: RouteExecutionStepKind,
    label: &'static str,
    actor: WorkflowActor,
    context: ActiveContext,
    object_ref: Option<ObjectRef>,
    domain: AuthorityDomain,
    action: AuthorityAction,
    visibility: VisibilityClass,
    supports_offline: bool,
    supports_partial_receipt: bool,
    uses_receipt_token: bool,
    outbox_state: OutboxState,
    sync_state: SyncState,
    detail_lines: Vec<&'static str>,
}

fn route_execution_step(
    route_ref: ObjectRef,
    fixture: RouteExecutionStepFixture,
) -> RouteExecutionStep {
    RouteExecutionStep {
        id: fixture.id.to_string(),
        kind: fixture.kind,
        label: fixture.label.to_string(),
        actor: fixture.actor,
        context: fixture.context.clone(),
        route_ref,
        object_ref: fixture.object_ref,
        required_authority: fixture_authority_gate(
            fixture.actor,
            fixture.context,
            fixture.domain,
            fixture.action,
        ),
        visibility: fixture.visibility,
        supports_offline: fixture.supports_offline,
        supports_partial_receipt: fixture.supports_partial_receipt,
        uses_receipt_token: fixture.uses_receipt_token,
        outbox_state: fixture.outbox_state,
        sync_state: fixture.sync_state,
        detail_lines: fixture
            .detail_lines
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn all_route_execution_flows() -> Vec<RouteExecutionFlow> {
    let route = context_for_type(ContextType::Route);
    let route_partner = context_for_type(ContextType::RoutePartner);
    let buyer_receiver = ActiveContext {
        context_type: ContextType::Buyer,
        context_ref: object_ref(
            ObjectKind::BuyerWorkspace,
            "buyer_receiver_workspace_001",
            "Kitchen receiving workspace",
        ),
        actor: WorkflowActor::BuyerReceiver,
        display_label: "Kitchen receiving workspace".to_string(),
        visibility_scope: VisibilityClass::BuyerScoped,
    };
    let route_ref = object_ref(
        ObjectKind::Route,
        "route_thursday_001",
        "Thursday network loop",
    );
    let stop_pickup_ref = object_ref(
        ObjectKind::RouteStop,
        "route_stop_pickup_001",
        "Floripa Farm pickup",
    );
    let stop_dropoff_ref = object_ref(
        ObjectKind::RouteStop,
        "route_stop_dropoff_001",
        "Kitchen drop-off",
    );
    let pickup_proof_ref = object_ref(
        ObjectKind::Proof,
        "proof_pickup_001",
        "Pickup confirmation proof",
    );
    let dropoff_proof_ref = object_ref(
        ObjectKind::Proof,
        "proof_dropoff_001",
        "Drop-off confirmation proof",
    );
    let receipt_ref = object_ref(
        ObjectKind::Proof,
        "receipt_kitchen_001",
        "Kitchen receipt confirmation",
    );
    let exception_ref = object_ref(
        ObjectKind::Exception,
        "exception_short_case_001",
        "Short case divergence",
    );

    vec![
        RouteExecutionFlow {
            id: "route_partner_assigned_stops".to_string(),
            kind: RouteExecutionFlowKind::RoutePartnerAssignedStops,
            title: "Assigned route stops".to_string(),
            actor: WorkflowActor::RoutePartner,
            context: route_partner.clone(),
            route_ref: route_ref.clone(),
            steps: vec![
                route_execution_step(
                    route_ref.clone(),
                    RouteExecutionStepFixture {
                        id: "assigned_route",
                        kind: RouteExecutionStepKind::AssignedRoute,
                        label: "Assigned route",
                        actor: WorkflowActor::RoutePartner,
                        context: route_partner.clone(),
                        object_ref: Some(route_ref.clone()),
                        domain: AuthorityDomain::RouteExecution,
                        action: AuthorityAction::Search,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: true,
                        supports_partial_receipt: false,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::NotQueued,
                        sync_state: SyncState::Online,
                        detail_lines: vec![
                            "RoutePartner sees the assigned route and assigned stops only.",
                            "Buyer packet and private buyer workspace data are not exposed.",
                        ],
                    },
                ),
                route_execution_step(
                    route_ref.clone(),
                    RouteExecutionStepFixture {
                        id: "pickup_confirmation",
                        kind: RouteExecutionStepKind::PickupConfirmation,
                        label: "Confirm pickup",
                        actor: WorkflowActor::RoutePartner,
                        context: route_partner.clone(),
                        object_ref: Some(pickup_proof_ref),
                        domain: AuthorityDomain::RouteExecution,
                        action: AuthorityAction::Submit,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: true,
                        supports_partial_receipt: false,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::Queued,
                        sync_state: SyncState::Offline,
                        detail_lines: vec![
                            "Photo, note, scan, or signature proof can queue offline.",
                            "The stop remains scoped to the assigned route.",
                        ],
                    },
                ),
                route_execution_step(
                    route_ref.clone(),
                    RouteExecutionStepFixture {
                        id: "dropoff_confirmation",
                        kind: RouteExecutionStepKind::DropoffConfirmation,
                        label: "Confirm drop-off",
                        actor: WorkflowActor::RoutePartner,
                        context: route_partner,
                        object_ref: Some(dropoff_proof_ref),
                        domain: AuthorityDomain::RouteExecution,
                        action: AuthorityAction::Submit,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: true,
                        supports_partial_receipt: false,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::Syncing,
                        sync_state: SyncState::Syncing,
                        detail_lines: vec![
                            "Drop-off proof syncs when relay connectivity returns.",
                            "Receipt confirmation remains separate from route execution.",
                        ],
                    },
                ),
                route_execution_step(
                    route_ref.clone(),
                    RouteExecutionStepFixture {
                        id: "assigned_pickup_stop",
                        kind: RouteExecutionStepKind::AssignedRoute,
                        label: "Pickup stop",
                        actor: WorkflowActor::RoutePartner,
                        context: context_for_type(ContextType::RoutePartner),
                        object_ref: Some(stop_pickup_ref),
                        domain: AuthorityDomain::RouteExecution,
                        action: AuthorityAction::NavigateRelatedObject,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: true,
                        supports_partial_receipt: false,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::NotQueued,
                        sync_state: SyncState::Synced,
                        detail_lines: vec!["Assigned stop detail is available offline."],
                    },
                ),
                route_execution_step(
                    route_ref.clone(),
                    RouteExecutionStepFixture {
                        id: "assigned_dropoff_stop",
                        kind: RouteExecutionStepKind::AssignedRoute,
                        label: "Drop-off stop",
                        actor: WorkflowActor::RoutePartner,
                        context: context_for_type(ContextType::RoutePartner),
                        object_ref: Some(stop_dropoff_ref),
                        domain: AuthorityDomain::RouteExecution,
                        action: AuthorityAction::NavigateRelatedObject,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: true,
                        supports_partial_receipt: false,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::NotQueued,
                        sync_state: SyncState::Synced,
                        detail_lines: vec!["Assigned stop detail is available offline."],
                    },
                ),
            ],
        },
        RouteExecutionFlow {
            id: "buyer_receipt_confirmation".to_string(),
            kind: RouteExecutionFlowKind::BuyerReceiptConfirmation,
            title: "Buyer receipt confirmation".to_string(),
            actor: WorkflowActor::BuyerReceiver,
            context: buyer_receiver.clone(),
            route_ref: route_ref.clone(),
            steps: vec![route_execution_step(
                route_ref.clone(),
                RouteExecutionStepFixture {
                    id: "receiver_receipt",
                    kind: RouteExecutionStepKind::ReceiptConfirmation,
                    label: "Confirm full or partial receipt",
                    actor: WorkflowActor::BuyerReceiver,
                    context: buyer_receiver,
                    object_ref: Some(receipt_ref),
                    domain: AuthorityDomain::Receipt,
                    action: AuthorityAction::Submit,
                    visibility: VisibilityClass::BuyerScoped,
                    supports_offline: true,
                    supports_partial_receipt: true,
                    uses_receipt_token: true,
                    outbox_state: OutboxState::Queued,
                    sync_state: SyncState::Offline,
                    detail_lines: vec![
                        "BuyerReceiver can confirm full or partial receipt.",
                        "A scoped receipt token can be used without exposing buyer workspace data.",
                    ],
                },
            )],
        },
        RouteExecutionFlow {
            id: "route_exception_recovery".to_string(),
            kind: RouteExecutionFlowKind::ExceptionRecovery,
            title: "Exception recovery".to_string(),
            actor: WorkflowActor::RoutePartner,
            context: context_for_type(ContextType::RoutePartner),
            route_ref: route_ref.clone(),
            steps: vec![
                route_execution_step(
                    route_ref.clone(),
                    RouteExecutionStepFixture {
                        id: "report_exception",
                        kind: RouteExecutionStepKind::ExceptionReport,
                        label: "Report divergence",
                        actor: WorkflowActor::RoutePartner,
                        context: context_for_type(ContextType::RoutePartner),
                        object_ref: Some(exception_ref.clone()),
                        domain: AuthorityDomain::RouteExecution,
                        action: AuthorityAction::Submit,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: true,
                        supports_partial_receipt: false,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::Conflict,
                        sync_state: SyncState::Failed,
                        detail_lines: vec![
                            "Short, damaged, late, or missing-item divergence becomes an exception card.",
                            "The exception stays route-scoped until resolved or escalated.",
                        ],
                    },
                ),
                route_execution_step(
                    route_ref,
                    RouteExecutionStepFixture {
                        id: "resolve_exception",
                        kind: RouteExecutionStepKind::RecoveryAction,
                        label: "Resolve recovery path",
                        actor: WorkflowActor::RouteCoordinator,
                        context: route,
                        object_ref: Some(exception_ref),
                        domain: AuthorityDomain::RouteCoordination,
                        action: AuthorityAction::Close,
                        visibility: VisibilityClass::RouteScoped,
                        supports_offline: false,
                        supports_partial_receipt: true,
                        uses_receipt_token: false,
                        outbox_state: OutboxState::AwaitingAuthority,
                        sync_state: SyncState::Online,
                        detail_lines: vec![
                            "RouteCoordinator chooses correction, partial receipt, replacement, or closure.",
                            "Recovery keeps route execution, receipt, and buyer data boundaries separate.",
                        ],
                    },
                ),
            ],
        },
    ]
}

pub fn fixture_route_execution_flows(context_id: Option<String>) -> Vec<RouteExecutionFlow> {
    let Some(context_id) = context_id else {
        return all_route_execution_flows();
    };
    let matching: Vec<RouteExecutionFlow> = all_route_execution_flows()
        .into_iter()
        .filter(|flow| {
            flow.context.context_ref.object_id == context_id
                || flow
                    .steps
                    .iter()
                    .any(|step| step.context.context_ref.object_id == context_id)
        })
        .collect();
    if matching.is_empty() {
        all_route_execution_flows()
    } else {
        matching
    }
}

struct ProofProvenanceArtifactFixture {
    id: &'static str,
    kind: ProofProvenanceArtifactKind,
    review_state: ProofProvenanceReviewState,
    title: &'static str,
    actor: WorkflowActor,
    context: ActiveContext,
    object_ref: ObjectRef,
    source_object_refs: Vec<ObjectRef>,
    domain: AuthorityDomain,
    action: AuthorityAction,
    visibility: VisibilityClass,
    is_public_preview: bool,
    requires_redaction_review: bool,
    can_publish: bool,
    public_summary_lines: Vec<&'static str>,
    redacted_field_labels: Vec<&'static str>,
    outbox_state: OutboxState,
    sync_state: SyncState,
}

fn proof_provenance_artifact(fixture: ProofProvenanceArtifactFixture) -> ProofProvenanceArtifact {
    ProofProvenanceArtifact {
        id: fixture.id.to_string(),
        kind: fixture.kind,
        review_state: fixture.review_state,
        title: fixture.title.to_string(),
        actor: fixture.actor,
        context: fixture.context.clone(),
        object_ref: fixture.object_ref,
        source_object_refs: fixture.source_object_refs,
        required_authority: fixture_authority_gate(
            fixture.actor,
            fixture.context,
            fixture.domain,
            fixture.action,
        ),
        visibility: fixture.visibility,
        is_public_preview: fixture.is_public_preview,
        requires_redaction_review: fixture.requires_redaction_review,
        can_publish: fixture.can_publish,
        public_summary_lines: fixture
            .public_summary_lines
            .into_iter()
            .map(str::to_string)
            .collect(),
        redacted_field_labels: fixture
            .redacted_field_labels
            .into_iter()
            .map(str::to_string)
            .collect(),
        outbox_state: fixture.outbox_state,
        sync_state: fixture.sync_state,
    }
}

fn private_provenance_redaction_labels() -> Vec<&'static str> {
    vec![
        "private trace JSON",
        "private buyer details",
        "private evidence",
        "private route stops",
        "worker notes",
    ]
}

fn all_proof_provenance_artifacts() -> Vec<ProofProvenanceArtifact> {
    let trace = context_for_type(ContextType::TraceRecords);
    let farm = context_for_type(ContextType::Farm);
    let buyer = context_for_type(ContextType::Buyer);
    let proof_ref = object_ref(
        ObjectKind::Proof,
        "proof_route_loop_001",
        "Route loop proof set",
    );
    let producer_proof_ref = object_ref(
        ObjectKind::Proof,
        "proof_farm_lot_001",
        "Farm lot proof set",
    );
    let buyer_packet_ref = object_ref(
        ObjectKind::BuyerPacket,
        "buyer_packet_kitchen_001",
        "Kitchen buyer packet",
    );
    let public_provenance_ref = object_ref(
        ObjectKind::Provenance,
        "public_provenance_squash_001",
        "Summer squash provenance preview",
    );
    let food_ref = object_ref(ObjectKind::Food, "food_harvest_001", "Summer squash lot");
    let route_ref = object_ref(
        ObjectKind::Route,
        "route_thursday_001",
        "Thursday network loop",
    );
    let receipt_ref = object_ref(
        ObjectKind::Proof,
        "receipt_kitchen_001",
        "Kitchen receipt confirmation",
    );

    vec![
        proof_provenance_artifact(ProofProvenanceArtifactFixture {
            id: "trace_proof_completeness",
            kind: ProofProvenanceArtifactKind::ProofCompleteness,
            review_state: ProofProvenanceReviewState::MissingProof,
            title: "Trace proof completeness",
            actor: WorkflowActor::TraceLead,
            context: trace.clone(),
            object_ref: proof_ref.clone(),
            source_object_refs: vec![route_ref.clone(), receipt_ref.clone()],
            domain: AuthorityDomain::TraceProof,
            action: AuthorityAction::Approve,
            visibility: VisibilityClass::WorkspacePrivate,
            is_public_preview: false,
            requires_redaction_review: false,
            can_publish: false,
            public_summary_lines: vec![
                "Internal proof set needs one receipt confirmation before publication review.",
            ],
            redacted_field_labels: Vec::new(),
            outbox_state: OutboxState::NotQueued,
            sync_state: SyncState::Online,
        }),
        proof_provenance_artifact(ProofProvenanceArtifactFixture {
            id: "producer_proof_completeness",
            kind: ProofProvenanceArtifactKind::ProofCompleteness,
            review_state: ProofProvenanceReviewState::Complete,
            title: "Producer proof completeness",
            actor: WorkflowActor::ProducerAdmin,
            context: farm.clone(),
            object_ref: producer_proof_ref.clone(),
            source_object_refs: vec![food_ref.clone(), route_ref.clone()],
            domain: AuthorityDomain::TraceProof,
            action: AuthorityAction::Approve,
            visibility: VisibilityClass::FarmPrivate,
            is_public_preview: false,
            requires_redaction_review: false,
            can_publish: false,
            public_summary_lines: vec![
                "Authorized producer review confirms farm lot proof completeness.",
            ],
            redacted_field_labels: Vec::new(),
            outbox_state: OutboxState::Shared,
            sync_state: SyncState::Synced,
        }),
        proof_provenance_artifact(ProofProvenanceArtifactFixture {
            id: "buyer_packet_draft",
            kind: ProofProvenanceArtifactKind::BuyerPacketDraft,
            review_state: ProofProvenanceReviewState::Draft,
            title: "Buyer packet draft",
            actor: WorkflowActor::BuyerSourcingLead,
            context: buyer.clone(),
            object_ref: buyer_packet_ref.clone(),
            source_object_refs: vec![food_ref.clone(), receipt_ref.clone()],
            domain: AuthorityDomain::BuyerWorkspace,
            action: AuthorityAction::Submit,
            visibility: VisibilityClass::BuyerScoped,
            is_public_preview: false,
            requires_redaction_review: false,
            can_publish: false,
            public_summary_lines: vec!["Buyer packet draft is scoped to the buyer workspace."],
            redacted_field_labels: Vec::new(),
            outbox_state: OutboxState::Draft,
            sync_state: SyncState::Offline,
        }),
        proof_provenance_artifact(ProofProvenanceArtifactFixture {
            id: "buyer_packet_shared",
            kind: ProofProvenanceArtifactKind::BuyerPacketShared,
            review_state: ProofProvenanceReviewState::Shared,
            title: "Buyer packet shared",
            actor: WorkflowActor::BuyerSourcingLead,
            context: buyer,
            object_ref: buyer_packet_ref,
            source_object_refs: vec![producer_proof_ref.clone(), receipt_ref.clone()],
            domain: AuthorityDomain::BuyerWorkspace,
            action: AuthorityAction::Share,
            visibility: VisibilityClass::BuyerScoped,
            is_public_preview: false,
            requires_redaction_review: false,
            can_publish: false,
            public_summary_lines: vec!["Buyer packet has been shared with authorized receivers."],
            redacted_field_labels: Vec::new(),
            outbox_state: OutboxState::Shared,
            sync_state: SyncState::Synced,
        }),
        proof_provenance_artifact(ProofProvenanceArtifactFixture {
            id: "public_provenance_redaction_review",
            kind: ProofProvenanceArtifactKind::PublicProvenancePreview,
            review_state: ProofProvenanceReviewState::RedactionRequired,
            title: "Public provenance redaction review",
            actor: WorkflowActor::ProducerAdmin,
            context: farm.clone(),
            object_ref: public_provenance_ref.clone(),
            source_object_refs: vec![food_ref.clone(), producer_proof_ref.clone()],
            domain: AuthorityDomain::PublicPublishing,
            action: AuthorityAction::Publish,
            visibility: VisibilityClass::PublicProvenance,
            is_public_preview: true,
            requires_redaction_review: true,
            can_publish: false,
            public_summary_lines: vec![
                "Summer squash was grown by Floripa Farm for the Thursday network loop.",
                "Public preview includes farm, food, harvest window, and network-level route summary.",
            ],
            redacted_field_labels: private_provenance_redaction_labels(),
            outbox_state: OutboxState::AwaitingAuthority,
            sync_state: SyncState::Online,
        }),
        proof_provenance_artifact(ProofProvenanceArtifactFixture {
            id: "public_provenance_ready",
            kind: ProofProvenanceArtifactKind::PublicProvenancePreview,
            review_state: ProofProvenanceReviewState::ReadyToPublish,
            title: "Public provenance ready",
            actor: WorkflowActor::ProducerAdmin,
            context: farm,
            object_ref: public_provenance_ref,
            source_object_refs: vec![food_ref, producer_proof_ref],
            domain: AuthorityDomain::PublicPublishing,
            action: AuthorityAction::Publish,
            visibility: VisibilityClass::PublicProvenance,
            is_public_preview: true,
            requires_redaction_review: false,
            can_publish: true,
            public_summary_lines: vec![
                "Summer squash provenance is ready for public community publication.",
                "Preview includes only redacted farm, food, harvest window, and network summary fields.",
            ],
            redacted_field_labels: private_provenance_redaction_labels(),
            outbox_state: OutboxState::Queued,
            sync_state: SyncState::Syncing,
        }),
    ]
}

pub fn fixture_proof_provenance_artifacts(
    context_id: Option<String>,
) -> Vec<ProofProvenanceArtifact> {
    let Some(context_id) = context_id else {
        return all_proof_provenance_artifacts();
    };
    let matching: Vec<ProofProvenanceArtifact> = all_proof_provenance_artifacts()
        .into_iter()
        .filter(|artifact| artifact.context.context_ref.object_id == context_id)
        .collect();
    if matching.is_empty() {
        all_proof_provenance_artifacts()
    } else {
        matching
    }
}

struct StewardshipAccessItemFixture {
    id: &'static str,
    kind: StewardshipAccessItemKind,
    title: &'static str,
    actor: WorkflowActor,
    context: ActiveContext,
    target_ref: ObjectRef,
    domain: AuthorityDomain,
    action: AuthorityAction,
    visibility: VisibilityClass,
    is_admin_lite: bool,
    is_phase_2_deferred: bool,
    grants_private_access: bool,
    outbox_state: OutboxState,
    sync_state: SyncState,
    detail_lines: Vec<&'static str>,
}

fn stewardship_access_item(fixture: StewardshipAccessItemFixture) -> StewardshipAccessItem {
    StewardshipAccessItem {
        id: fixture.id.to_string(),
        kind: fixture.kind,
        title: fixture.title.to_string(),
        actor: fixture.actor,
        context: fixture.context.clone(),
        target_ref: fixture.target_ref,
        required_authority: fixture_authority_gate(
            fixture.actor,
            fixture.context,
            fixture.domain,
            fixture.action,
        ),
        visibility: fixture.visibility,
        is_admin_lite: fixture.is_admin_lite,
        is_phase_2_deferred: fixture.is_phase_2_deferred,
        grants_private_access: fixture.grants_private_access,
        outbox_state: fixture.outbox_state,
        sync_state: fixture.sync_state,
        detail_lines: fixture
            .detail_lines
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn all_stewardship_access_items() -> Vec<StewardshipAccessItem> {
    let steward = context_for_type(ContextType::NetworkSteward);
    let network = context_for_type(ContextType::Network);
    let route = context_for_type(ContextType::Route);
    let member = context_for_type(ContextType::Regional);
    let access_request_ref = object_ref(
        ObjectKind::AccessMembership,
        "access_request_farm_team_001",
        "Farm team access request",
    );
    let role_ref = object_ref(
        ObjectKind::AccessMembership,
        "role_route_partner_candidate_001",
        "Route partner candidate role",
    );
    let route_partner_ref = object_ref(
        ObjectKind::RoutePartner,
        "route_partner_invite_001",
        "Thursday route partner invite",
    );
    let route_pool_ref = object_ref(
        ObjectKind::Route,
        "route_pool_metadata_001",
        "Route pool metadata",
    );
    let public_update_ref = object_ref(
        ObjectKind::Update,
        "community_update_review_001",
        "Community update moderation",
    );
    let invite_ref = object_ref(
        ObjectKind::MemberInvite,
        "member_invite_001",
        "Network invite",
    );
    let request_ref = object_ref(
        ObjectKind::AccessMembership,
        "access_request_self_001",
        "Request network access",
    );
    let denied_ref = object_ref(
        ObjectKind::Farm,
        "private_farm_denied_001",
        "Private farm context",
    );
    let group_ref = object_ref(
        ObjectKind::AccessMembership,
        "phase2_group_management_001",
        "Group management",
    );

    vec![
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "steward_review_access_request",
            kind: StewardshipAccessItemKind::AccessRequestReview,
            title: "Review access request",
            actor: WorkflowActor::NetworkSteward,
            context: steward.clone(),
            target_ref: access_request_ref,
            domain: AuthorityDomain::RelayGroupAccess,
            action: AuthorityAction::Approve,
            visibility: VisibilityClass::WorkspacePrivate,
            is_admin_lite: true,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::Queued,
            sync_state: SyncState::Online,
            detail_lines: vec![
                "NetworkSteward reviews scoped membership requests.",
                "Approval grants role context, not private workspace access.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "steward_approve_role",
            kind: StewardshipAccessItemKind::RoleApproval,
            title: "Approve role",
            actor: WorkflowActor::NetworkSteward,
            context: steward.clone(),
            target_ref: role_ref,
            domain: AuthorityDomain::RelayGroupAccess,
            action: AuthorityAction::Approve,
            visibility: VisibilityClass::WorkspacePrivate,
            is_admin_lite: true,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::Shared,
            sync_state: SyncState::Synced,
            detail_lines: vec![
                "Role approval remains constrained to the requested context.",
                "Private farm, buyer, route, proof, and trace data require their own authorities.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "steward_invite_route_partner",
            kind: StewardshipAccessItemKind::RoutePartnerInvite,
            title: "Invite RoutePartner",
            actor: WorkflowActor::NetworkSteward,
            context: steward.clone(),
            target_ref: route_partner_ref,
            domain: AuthorityDomain::RelayGroupAccess,
            action: AuthorityAction::Share,
            visibility: VisibilityClass::NetworkVisible,
            is_admin_lite: true,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::Queued,
            sync_state: SyncState::Syncing,
            detail_lines: vec![
                "RoutePartner invite can be issued without route stop details.",
                "Assignment still belongs to route coordination.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "steward_route_pool_metadata",
            kind: StewardshipAccessItemKind::RoutePoolMetadata,
            title: "Set route pool metadata",
            actor: WorkflowActor::NetworkSteward,
            context: steward.clone(),
            target_ref: route_pool_ref,
            domain: AuthorityDomain::NetworkStewardship,
            action: AuthorityAction::Assign,
            visibility: VisibilityClass::NetworkVisible,
            is_admin_lite: true,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::Draft,
            sync_state: SyncState::Offline,
            detail_lines: vec![
                "Stewardship metadata describes route pool availability.",
                "Concrete route assignment remains route-coordinator authority.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "steward_public_moderation",
            kind: StewardshipAccessItemKind::PublicModeration,
            title: "Moderate public/community state",
            actor: WorkflowActor::NetworkSteward,
            context: steward.clone(),
            target_ref: public_update_ref,
            domain: AuthorityDomain::PublicPublishing,
            action: AuthorityAction::Correct,
            visibility: VisibilityClass::PublicCommunity,
            is_admin_lite: true,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::AwaitingAuthority,
            sync_state: SyncState::Online,
            detail_lines: vec![
                "Public/community moderation is allowed where publishing authority permits it.",
                "Moderation never exposes private proof, buyer, farm, or route-stop data.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "member_accept_invite",
            kind: StewardshipAccessItemKind::InviteAcceptance,
            title: "Accept invite",
            actor: WorkflowActor::NetworkMember,
            context: network.clone(),
            target_ref: invite_ref,
            domain: AuthorityDomain::RelayGroupAccess,
            action: AuthorityAction::Submit,
            visibility: VisibilityClass::NetworkVisible,
            is_admin_lite: false,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::Queued,
            sync_state: SyncState::Online,
            detail_lines: vec![
                "Invite acceptance connects membership without full group management.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "member_request_access",
            kind: StewardshipAccessItemKind::RequestAccess,
            title: "Request access",
            actor: WorkflowActor::NetworkMember,
            context: member,
            target_ref: request_ref,
            domain: AuthorityDomain::RelayGroupAccess,
            action: AuthorityAction::Submit,
            visibility: VisibilityClass::NetworkVisible,
            is_admin_lite: false,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::Draft,
            sync_state: SyncState::Offline,
            detail_lines: vec!["Access requests are queued as explicit membership work."],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "member_access_denied",
            kind: StewardshipAccessItemKind::AccessDenied,
            title: "Access denied",
            actor: WorkflowActor::NetworkMember,
            context: network,
            target_ref: denied_ref,
            domain: AuthorityDomain::FarmWorkspaceOperations,
            action: AuthorityAction::Search,
            visibility: VisibilityClass::FarmPrivate,
            is_admin_lite: false,
            is_phase_2_deferred: false,
            grants_private_access: false,
            outbox_state: OutboxState::NotQueued,
            sync_state: SyncState::Online,
            detail_lines: vec![
                "Denied state preserves the blocked context label without revealing private data.",
            ],
        }),
        stewardship_access_item(StewardshipAccessItemFixture {
            id: "phase2_group_management_deferred",
            kind: StewardshipAccessItemKind::GroupManagementDeferred,
            title: "Group management deferred",
            actor: WorkflowActor::NetworkSteward,
            context: route,
            target_ref: group_ref,
            domain: AuthorityDomain::NetworkStewardship,
            action: AuthorityAction::Approve,
            visibility: VisibilityClass::WorkspacePrivate,
            is_admin_lite: true,
            is_phase_2_deferred: true,
            grants_private_access: false,
            outbox_state: OutboxState::NotQueued,
            sync_state: SyncState::Unknown,
            detail_lines: vec![
                "Full group creation and management are explicitly deferred to Phase 2.",
            ],
        }),
    ]
}

pub fn fixture_stewardship_access_items(context_id: Option<String>) -> Vec<StewardshipAccessItem> {
    let Some(context_id) = context_id else {
        return all_stewardship_access_items();
    };
    let matching: Vec<StewardshipAccessItem> = all_stewardship_access_items()
        .into_iter()
        .filter(|item| item.context.context_ref.object_id == context_id)
        .collect();
    if matching.is_empty() {
        all_stewardship_access_items()
    } else {
        matching
    }
}

pub fn fixture_outbox_items() -> Vec<OutboxItem> {
    let context = context_for_type(ContextType::Farm);
    CANONICAL_OUTBOX_STATES
        .into_iter()
        .enumerate()
        .map(|(index, outbox_state)| OutboxItem {
            id: format!("outbox_fixture_{index:02}"),
            action_type: AddActionType::PublicUpdate,
            context: context.clone(),
            object_refs: vec![object_ref(
                ObjectKind::Update,
                format!("draft_update_{index:02}"),
                "Public update draft",
            )],
            event_refs: Vec::new(),
            visibility: VisibilityClass::PublicCommunity,
            authority_gate: fixture_authority_gate(
                context.actor,
                context.clone(),
                AuthorityDomain::PublicPublishing,
                AuthorityAction::Retry,
            ),
            flow_state: if matches!(outbox_state, OutboxState::Draft) {
                AddFlowState::Draft
            } else {
                AddFlowState::Queued
            },
            outbox_state,
            sync_state: CANONICAL_SYNC_STATES[index % CANONICAL_SYNC_STATES.len()],
            queued_at_unix: Some(1_799_971_200 + index as u64),
            last_attempt_at_unix: None,
            retry_count: index as u32,
            last_error: if matches!(outbox_state, OutboxState::Failed) {
                Some("fixture failure".to_string())
            } else {
                None
            },
        })
        .collect()
}

fn outbox_state_allows_retry(state: OutboxState) -> bool {
    matches!(state, OutboxState::Failed | OutboxState::Conflict)
}

fn outbox_visibility_allows_retry(visibility: VisibilityClass) -> bool {
    !matches!(
        visibility,
        VisibilityClass::LocalDraft | VisibilityClass::SecretNeverShared
    )
}

pub fn fixture_outbox_retry_decision(item: OutboxItem) -> OutboxRetryDecision {
    let authority_gate = fixture_authority_gate(
        item.context.actor,
        item.context.clone(),
        item.authority_gate.domain,
        AuthorityAction::Retry,
    );
    let state_allows_retry = outbox_state_allows_retry(item.outbox_state);
    let visibility_allows_retry = outbox_visibility_allows_retry(item.visibility);
    let is_retryable = state_allows_retry && visibility_allows_retry && authority_gate.is_allowed;
    let reason = if is_retryable {
        None
    } else if !state_allows_retry {
        Some(format!(
            "{:?} is not a retryable outbox state",
            item.outbox_state
        ))
    } else if !visibility_allows_retry {
        Some(format!(
            "{:?} visibility cannot be retried",
            item.visibility
        ))
    } else {
        authority_gate.reason.clone()
    };

    OutboxRetryDecision {
        item_id: item.id,
        is_retryable,
        authority_gate,
        reason,
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    pub fn phase1_active_contexts(&self) -> Vec<ActiveContext> {
        let _ = self;
        fixture_active_contexts()
    }

    pub fn phase1_today_cards(&self, context_id: Option<String>) -> Vec<TodayCard> {
        let _ = self;
        fixture_today_cards(context_id)
    }

    pub fn phase1_add_actions(&self, context_id: Option<String>) -> Vec<AddAction> {
        let _ = self;
        fixture_add_actions(context_id)
    }

    pub fn phase1_object_page_summaries(
        &self,
        context_id: Option<String>,
    ) -> Vec<ObjectPageSummary> {
        let _ = self;
        fixture_object_page_summaries(context_id)
    }

    pub fn phase1_outbox_snapshot(&self) -> Vec<OutboxItem> {
        let _ = self;
        fixture_outbox_items()
    }

    pub fn phase1_search_results(
        &self,
        query: Option<String>,
        context_id: Option<String>,
    ) -> Vec<SearchResultSummary> {
        let _ = self;
        fixture_search_results(query, context_id)
    }

    pub fn phase1_prototype_paths(&self) -> Vec<PrototypePath> {
        let _ = self;
        fixture_prototype_paths()
    }

    pub fn phase1_route_execution_flows(
        &self,
        context_id: Option<String>,
    ) -> Vec<RouteExecutionFlow> {
        let _ = self;
        fixture_route_execution_flows(context_id)
    }

    pub fn phase1_proof_provenance_artifacts(
        &self,
        context_id: Option<String>,
    ) -> Vec<ProofProvenanceArtifact> {
        let _ = self;
        fixture_proof_provenance_artifacts(context_id)
    }

    pub fn phase1_stewardship_access_items(
        &self,
        context_id: Option<String>,
    ) -> Vec<StewardshipAccessItem> {
        let _ = self;
        fixture_stewardship_access_items(context_id)
    }

    pub fn phase1_outbox_retry_decision(&self, item: OutboxItem) -> OutboxRetryDecision {
        let _ = self;
        fixture_outbox_retry_decision(item)
    }

    pub fn phase1_check_authority(
        &self,
        actor: WorkflowActor,
        context: ActiveContext,
        domain: AuthorityDomain,
        action: AuthorityAction,
    ) -> AuthorityGate {
        let _ = self;
        fixture_authority_gate(actor, context, domain, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_ref(object_type: ObjectKind, object_id: &str, display_label: &str) -> ObjectRef {
        ObjectRef {
            object_type,
            object_id: object_id.to_string(),
            display_label: display_label.to_string(),
        }
    }

    fn active_context() -> ActiveContext {
        ActiveContext {
            context_type: ContextType::Farm,
            context_ref: object_ref(ObjectKind::Farm, "farm_123", "Root & Rad Farm"),
            actor: WorkflowActor::ProducerAdmin,
            display_label: "Root & Rad Farm".to_string(),
            visibility_scope: VisibilityClass::FarmPrivate,
        }
    }

    fn authority_gate() -> AuthorityGate {
        AuthorityGate {
            domain: AuthorityDomain::FarmWorkspaceOperations,
            action: AuthorityAction::Submit,
            actor: WorkflowActor::ProducerAdmin,
            context: active_context(),
            is_required: true,
            is_allowed: true,
            reason: None,
        }
    }

    #[test]
    fn canonical_vocabularies_match_phase_1_spec_counts() {
        assert_eq!(CANONICAL_CONTEXT_TYPES.len(), 10);
        assert_eq!(CANONICAL_WORKFLOW_ACTORS.len(), 11);
        assert_eq!(CANONICAL_VISIBILITY_CLASSES.len(), 9);
        assert_eq!(CANONICAL_AUTHORITY_DOMAINS.len(), 9);
        assert_eq!(CANONICAL_TODAY_CARD_TYPES.len(), 12);
        assert_eq!(TODAY_CARD_RANKING_PRIORITY.len(), 12);
        assert_eq!(CANONICAL_ADD_ACTION_TYPES.len(), 18);
        assert_eq!(CANONICAL_ADD_FLOW_STATES.len(), 16);
        assert_eq!(CANONICAL_OBJECT_PAGE_FAMILIES.len(), 13);
        assert_eq!(CANONICAL_OUTBOX_STATES.len(), 10);
        assert_eq!(CANONICAL_SYNC_STATES.len(), 7);
        assert_eq!(CANONICAL_ROUTE_EXECUTION_FLOW_KINDS.len(), 3);
        assert_eq!(CANONICAL_ROUTE_EXECUTION_STEP_KINDS.len(), 6);
        assert_eq!(CANONICAL_PROOF_PROVENANCE_ARTIFACT_KINDS.len(), 4);
        assert_eq!(CANONICAL_PROOF_PROVENANCE_REVIEW_STATES.len(), 8);
        assert_eq!(CANONICAL_STEWARDSHIP_ACCESS_ITEM_KINDS.len(), 9);
    }

    #[test]
    fn today_card_ranking_priority_covers_every_card_type() {
        for card_type in CANONICAL_TODAY_CARD_TYPES {
            assert!(TODAY_CARD_RANKING_PRIORITY.contains(&card_type));
        }
        assert_eq!(TODAY_CARD_RANKING_PRIORITY[0], TodayCardType::Exception);
        assert_eq!(TODAY_CARD_RANKING_PRIORITY[1], TodayCardType::SyncOutbox);
        assert_eq!(TODAY_CARD_RANKING_PRIORITY[2], TodayCardType::Route);

        let cards = fixture_today_cards(None);
        assert_eq!(cards[0].card_type, TodayCardType::Exception);
        assert_eq!(cards[0].ranking_reason, "blocking exception");
        assert_eq!(cards[1].card_type, TodayCardType::SyncOutbox);
        assert_eq!(cards[1].outbox_state, OutboxState::Failed);
        assert!(cards[1].is_offline);
    }

    #[test]
    fn fixture_backed_projection_apis_cover_required_surface() {
        assert_eq!(
            fixture_active_contexts().len(),
            CANONICAL_CONTEXT_TYPES.len()
        );
        assert_eq!(
            fixture_today_cards(None).len(),
            CANONICAL_TODAY_CARD_TYPES.len()
        );
        assert_eq!(
            fixture_add_actions(None).len(),
            CANONICAL_ADD_ACTION_TYPES.len()
        );
        assert_eq!(
            fixture_object_page_summaries(None).len(),
            CANONICAL_OBJECT_PAGE_FAMILIES.len()
        );
        assert_eq!(fixture_outbox_items().len(), CANONICAL_OUTBOX_STATES.len());
        assert_eq!(
            fixture_route_execution_flows(None).len(),
            CANONICAL_ROUTE_EXECUTION_FLOW_KINDS.len()
        );
        assert_eq!(fixture_proof_provenance_artifacts(None).len(), 6);
        assert_eq!(fixture_stewardship_access_items(None).len(), 9);
    }

    #[test]
    fn object_page_fixtures_carry_routeable_refs_and_navigation_authority() {
        let pages = fixture_object_page_summaries(None);
        assert_eq!(pages.len(), CANONICAL_OBJECT_PAGE_FAMILIES.len());

        for (page, family) in pages.iter().zip(CANONICAL_OBJECT_PAGE_FAMILIES) {
            assert_eq!(page.family, family);
            assert_eq!(page.object_ref.object_type, object_kind_for_page(family));
            assert_eq!(
                page.required_authority.action,
                AuthorityAction::NavigateRelatedObject
            );
            assert_eq!(
                page.required_authority.domain,
                AuthorityDomain::RelayGroupAccess
            );
            assert_eq!(
                page.required_authority.context.context_ref.object_id,
                page.primary_context.context_ref.object_id
            );
            assert!(!page.object_ref.object_id.is_empty());
            assert!(!page.title.is_empty());
        }
    }

    #[test]
    fn authority_visibility_fixtures_cover_every_actor_and_visibility_class() {
        let context = active_context();
        for actor in CANONICAL_WORKFLOW_ACTORS {
            let gate = fixture_authority_gate(
                actor,
                context.clone(),
                AuthorityDomain::RelayGroupAccess,
                AuthorityAction::Search,
            );
            assert_eq!(gate.actor, actor);
            assert_eq!(gate.action, AuthorityAction::Search);
        }

        for visibility in CANONICAL_VISIBILITY_CLASSES {
            let result_allowed = visibility_allows_search_result(visibility);
            if matches!(
                visibility,
                VisibilityClass::LocalDraft
                    | VisibilityClass::FarmPrivate
                    | VisibilityClass::WorkspacePrivate
                    | VisibilityClass::RouteScoped
                    | VisibilityClass::BuyerScoped
                    | VisibilityClass::SecretNeverShared
            ) {
                assert!(!result_allowed);
            }
        }
    }

    #[test]
    fn search_results_filter_private_and_unauthorized_surfaces() {
        let network = context_for_type(ContextType::Network);
        let results = fixture_search_results(
            Some("fixture".to_string()),
            Some(network.context_ref.object_id),
        );
        assert!(!results.is_empty());
        assert!(
            results
                .iter()
                .all(|result| result.required_authority.is_allowed)
        );
        assert!(
            results
                .iter()
                .all(|result| visibility_allows_search_result(result.visibility))
        );
        assert!(
            results
                .iter()
                .all(|result| object_kind_allows_search_result(
                    result.object_ref.object_type,
                    result.visibility
                ))
        );
        assert!(
            !results
                .iter()
                .any(|result| result.object_ref.object_type == ObjectKind::BuyerPacket)
        );
        assert!(
            !results
                .iter()
                .any(|result| result.object_ref.object_type == ObjectKind::RouteStop)
        );

        let farm = context_for_type(ContextType::Farm);
        let denied = fixture_search_results(
            Some("fixture".to_string()),
            Some(farm.context_ref.object_id),
        );
        assert!(denied.is_empty());
    }

    #[test]
    fn prototype_paths_cover_required_phase_1_actor_routes() {
        let paths = fixture_prototype_paths();
        assert_eq!(paths.len(), 3);
        assert!(
            paths
                .iter()
                .any(|path| path.kind == PrototypePathKind::ProducerFoodToRoute
                    && path.actor == WorkflowActor::ProducerAdmin
                    && path.steps.iter().any(|step| step.label == "Add Food")
                    && path.steps.iter().any(|step| step.label == "Add to Route"))
        );
        assert!(paths.iter().any(|path| {
            path.kind == PrototypePathKind::BuyerCommitmentToRoute
                && path.actor == WorkflowActor::BuyerSourcingLead
                && path
                    .steps
                    .iter()
                    .any(|step| step.label == "Confirm Commitment")
        }));
        assert!(paths.iter().any(|path| {
            path.kind == PrototypePathKind::RouteCoordinatorAssignment
                && path.actor == WorkflowActor::RouteCoordinator
                && path
                    .steps
                    .iter()
                    .any(|step| step.label == "Assign RoutePartner")
        }));
    }

    #[test]
    fn prototype_paths_connect_actions_objects_outbox_and_authority() {
        let paths = fixture_prototype_paths();
        let steps: Vec<&PrototypePathStep> =
            paths.iter().flat_map(|path| path.steps.iter()).collect();

        assert!(
            steps
                .iter()
                .any(|step| step.action_type == Some(AddActionType::Food))
        );
        assert!(steps.iter().any(|step| {
            step.object_ref
                .as_ref()
                .is_some_and(|object_ref| object_ref.object_type == ObjectKind::Food)
        }));
        assert!(steps.iter().any(|step| {
            step.object_ref
                .as_ref()
                .is_some_and(|object_ref| object_ref.object_type == ObjectKind::Route)
        }));
        assert!(steps.iter().any(|step| {
            step.object_ref
                .as_ref()
                .is_some_and(|object_ref| object_ref.object_type == ObjectKind::RoutePartner)
        }));
        assert!(
            steps
                .iter()
                .any(|step| step.outbox_state == OutboxState::Queued)
        );
        assert!(
            steps
                .iter()
                .any(|step| step.outbox_state == OutboxState::Conflict)
        );
        assert!(steps.iter().all(|step| step.authority_gate.is_required));
        assert!(
            steps
                .iter()
                .all(|step| step.visibility != VisibilityClass::SecretNeverShared)
        );
    }

    #[test]
    fn route_execution_flows_cover_partner_receipt_and_exception_paths() {
        let flows = fixture_route_execution_flows(None);
        assert_eq!(flows.len(), 3);

        for kind in CANONICAL_ROUTE_EXECUTION_FLOW_KINDS {
            assert!(
                flows.iter().any(|flow| flow.kind == kind),
                "missing {kind:?}"
            );
        }

        let steps: Vec<&RouteExecutionStep> =
            flows.iter().flat_map(|flow| flow.steps.iter()).collect();
        for kind in CANONICAL_ROUTE_EXECUTION_STEP_KINDS {
            assert!(
                steps.iter().any(|step| step.kind == kind),
                "missing {kind:?}"
            );
        }

        assert!(steps.iter().any(|step| {
            step.kind == RouteExecutionStepKind::PickupConfirmation
                && step.supports_offline
                && step
                    .object_ref
                    .as_ref()
                    .is_some_and(|object_ref| object_ref.object_type == ObjectKind::Proof)
        }));
        assert!(steps.iter().any(|step| {
            step.kind == RouteExecutionStepKind::DropoffConfirmation
                && step.supports_offline
                && step
                    .object_ref
                    .as_ref()
                    .is_some_and(|object_ref| object_ref.object_type == ObjectKind::Proof)
        }));
        assert!(
            steps
                .iter()
                .any(|step| step.kind == RouteExecutionStepKind::ExceptionReport
                    && step.outbox_state == OutboxState::Conflict)
        );
        assert!(
            fixture_today_cards(None)
                .iter()
                .any(|card| card.card_type == TodayCardType::Exception)
        );
    }

    #[test]
    fn route_partner_execution_flow_is_assigned_route_scoped_only() {
        let flow = fixture_route_execution_flows(None)
            .into_iter()
            .find(|flow| flow.kind == RouteExecutionFlowKind::RoutePartnerAssignedStops)
            .expect("route partner flow");
        assert_eq!(flow.actor, WorkflowActor::RoutePartner);
        assert_eq!(flow.context.actor, WorkflowActor::RoutePartner);

        for step in &flow.steps {
            assert_eq!(step.actor, WorkflowActor::RoutePartner);
            assert_eq!(step.visibility, VisibilityClass::RouteScoped);
            assert_eq!(
                step.required_authority.domain,
                AuthorityDomain::RouteExecution
            );
            assert!(step.required_authority.is_allowed);
            assert_ne!(step.required_authority.domain, AuthorityDomain::Receipt);
            assert_ne!(
                step.required_authority.domain,
                AuthorityDomain::BuyerWorkspace
            );
            assert!(matches!(
                step.object_ref
                    .as_ref()
                    .map(|object_ref| object_ref.object_type),
                Some(ObjectKind::Route | ObjectKind::RouteStop | ObjectKind::Proof)
            ));
        }
    }

    #[test]
    fn buyer_receipt_flow_supports_partial_receipt_token_without_route_execution() {
        let flow = fixture_route_execution_flows(None)
            .into_iter()
            .find(|flow| flow.kind == RouteExecutionFlowKind::BuyerReceiptConfirmation)
            .expect("buyer receipt flow");
        assert_eq!(flow.actor, WorkflowActor::BuyerReceiver);
        assert_eq!(flow.context.actor, WorkflowActor::BuyerReceiver);
        assert_eq!(flow.steps.len(), 1);

        let receipt = &flow.steps[0];
        assert_eq!(receipt.kind, RouteExecutionStepKind::ReceiptConfirmation);
        assert_eq!(receipt.required_authority.domain, AuthorityDomain::Receipt);
        assert_eq!(receipt.required_authority.action, AuthorityAction::Submit);
        assert!(receipt.required_authority.is_allowed);
        assert!(receipt.supports_partial_receipt);
        assert!(receipt.uses_receipt_token);
        assert_ne!(
            receipt.required_authority.domain,
            AuthorityDomain::RouteExecution
        );
        assert_eq!(receipt.visibility, VisibilityClass::BuyerScoped);
    }

    #[test]
    fn route_exception_recovery_separates_reporting_from_coordination() {
        let flow = fixture_route_execution_flows(None)
            .into_iter()
            .find(|flow| flow.kind == RouteExecutionFlowKind::ExceptionRecovery)
            .expect("exception recovery flow");
        let report = flow
            .steps
            .iter()
            .find(|step| step.kind == RouteExecutionStepKind::ExceptionReport)
            .expect("report step");
        let recovery = flow
            .steps
            .iter()
            .find(|step| step.kind == RouteExecutionStepKind::RecoveryAction)
            .expect("recovery step");

        assert_eq!(report.actor, WorkflowActor::RoutePartner);
        assert_eq!(
            report.required_authority.domain,
            AuthorityDomain::RouteExecution
        );
        assert_eq!(report.outbox_state, OutboxState::Conflict);
        assert!(report.supports_offline);
        assert_eq!(recovery.actor, WorkflowActor::RouteCoordinator);
        assert_eq!(
            recovery.required_authority.domain,
            AuthorityDomain::RouteCoordination
        );
        assert_eq!(recovery.required_authority.action, AuthorityAction::Close);
        assert!(recovery.supports_partial_receipt);
        assert!(recovery.required_authority.is_allowed);
    }

    #[test]
    fn proof_provenance_artifacts_cover_required_kinds_and_states() {
        let artifacts = fixture_proof_provenance_artifacts(None);
        for kind in CANONICAL_PROOF_PROVENANCE_ARTIFACT_KINDS {
            assert!(
                artifacts.iter().any(|artifact| artifact.kind == kind),
                "missing {kind:?}"
            );
        }

        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == ProofProvenanceArtifactKind::ProofCompleteness
                && artifact.review_state == ProofProvenanceReviewState::MissingProof
        }));
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == ProofProvenanceArtifactKind::ProofCompleteness
                && artifact.review_state == ProofProvenanceReviewState::Complete
        }));
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == ProofProvenanceArtifactKind::BuyerPacketDraft
                && artifact.review_state == ProofProvenanceReviewState::Draft
        }));
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == ProofProvenanceArtifactKind::BuyerPacketShared
                && artifact.review_state == ProofProvenanceReviewState::Shared
        }));
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == ProofProvenanceArtifactKind::PublicProvenancePreview
                && artifact.review_state == ProofProvenanceReviewState::RedactionRequired
        }));
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == ProofProvenanceArtifactKind::PublicProvenancePreview
                && artifact.review_state == ProofProvenanceReviewState::ReadyToPublish
        }));
    }

    #[test]
    fn trace_lead_and_authorized_producer_can_review_proof_completeness() {
        let proof_artifacts: Vec<ProofProvenanceArtifact> =
            fixture_proof_provenance_artifacts(None)
                .into_iter()
                .filter(|artifact| artifact.kind == ProofProvenanceArtifactKind::ProofCompleteness)
                .collect();
        assert_eq!(proof_artifacts.len(), 2);
        assert!(proof_artifacts.iter().any(|artifact| {
            artifact.actor == WorkflowActor::TraceLead
                && artifact.required_authority.domain == AuthorityDomain::TraceProof
                && artifact.required_authority.action == AuthorityAction::Approve
                && artifact.required_authority.is_allowed
        }));
        assert!(proof_artifacts.iter().any(|artifact| {
            artifact.actor == WorkflowActor::ProducerAdmin
                && artifact.required_authority.domain == AuthorityDomain::TraceProof
                && artifact.required_authority.action == AuthorityAction::Approve
                && artifact.required_authority.is_allowed
        }));
    }

    #[test]
    fn buyer_packets_are_private_and_public_provenance_is_distinct() {
        let artifacts = fixture_proof_provenance_artifacts(None);
        let buyer_packets: Vec<&ProofProvenanceArtifact> = artifacts
            .iter()
            .filter(|artifact| {
                matches!(
                    artifact.kind,
                    ProofProvenanceArtifactKind::BuyerPacketDraft
                        | ProofProvenanceArtifactKind::BuyerPacketShared
                )
            })
            .collect();
        assert_eq!(buyer_packets.len(), 2);
        assert!(buyer_packets.iter().all(|artifact| {
            artifact.object_ref.object_type == ObjectKind::BuyerPacket
                && artifact.visibility == VisibilityClass::BuyerScoped
                && !artifact.is_public_preview
                && !artifact.can_publish
        }));

        let public_previews: Vec<&ProofProvenanceArtifact> = artifacts
            .iter()
            .filter(|artifact| {
                artifact.kind == ProofProvenanceArtifactKind::PublicProvenancePreview
            })
            .collect();
        assert_eq!(public_previews.len(), 2);
        assert!(public_previews.iter().all(|artifact| {
            artifact.object_ref.object_type == ObjectKind::Provenance
                && artifact.visibility == VisibilityClass::PublicProvenance
                && artifact.is_public_preview
                && artifact.required_authority.domain == AuthorityDomain::PublicPublishing
        }));
    }

    #[test]
    fn public_provenance_publication_requires_authority_and_redaction_review() {
        let artifacts = fixture_proof_provenance_artifacts(None);
        let review = artifacts
            .iter()
            .find(|artifact| {
                artifact.id == "public_provenance_redaction_review"
                    && artifact.kind == ProofProvenanceArtifactKind::PublicProvenancePreview
            })
            .expect("redaction review artifact");
        assert!(review.required_authority.is_allowed);
        assert_eq!(review.required_authority.action, AuthorityAction::Publish);
        assert!(review.requires_redaction_review);
        assert!(!review.can_publish);
        assert_eq!(review.outbox_state, OutboxState::AwaitingAuthority);

        let ready = artifacts
            .iter()
            .find(|artifact| artifact.id == "public_provenance_ready")
            .expect("ready artifact");
        assert!(ready.required_authority.is_allowed);
        assert!(!ready.requires_redaction_review);
        assert!(ready.can_publish);
        assert_eq!(
            ready.review_state,
            ProofProvenanceReviewState::ReadyToPublish
        );
    }

    #[test]
    fn public_provenance_never_leaks_private_trace_or_buyer_fields() {
        let public_previews: Vec<ProofProvenanceArtifact> =
            fixture_proof_provenance_artifacts(None)
                .into_iter()
                .filter(|artifact| {
                    artifact.kind == ProofProvenanceArtifactKind::PublicProvenancePreview
                })
                .collect();
        assert!(!public_previews.is_empty());

        let blocked = private_provenance_redaction_labels();
        for artifact in public_previews {
            for label in &blocked {
                assert!(
                    artifact
                        .redacted_field_labels
                        .iter()
                        .any(|redacted| redacted == label)
                );
            }

            let public_text = artifact.public_summary_lines.join(" ").to_lowercase();
            for label in &blocked {
                assert!(
                    !public_text.contains(&label.to_lowercase()),
                    "{label} leaked into public summary"
                );
            }
            assert!(
                !artifact
                    .source_object_refs
                    .iter()
                    .any(|object_ref| object_ref.object_type == ObjectKind::BuyerPacket)
            );
            assert!(
                !artifact
                    .source_object_refs
                    .iter()
                    .any(|object_ref| object_ref.object_type == ObjectKind::RouteStop)
            );
        }
    }

    #[test]
    fn stewardship_access_items_cover_admin_lite_and_member_access_states() {
        let items = fixture_stewardship_access_items(None);
        for kind in CANONICAL_STEWARDSHIP_ACCESS_ITEM_KINDS {
            assert!(
                items.iter().any(|item| item.kind == kind),
                "missing {kind:?}"
            );
        }

        assert!(items.iter().any(|item| {
            item.kind == StewardshipAccessItemKind::InviteAcceptance
                && item.actor == WorkflowActor::NetworkMember
        }));
        assert!(items.iter().any(|item| {
            item.kind == StewardshipAccessItemKind::RequestAccess
                && item.actor == WorkflowActor::NetworkMember
        }));
        assert!(items.iter().any(|item| {
            item.kind == StewardshipAccessItemKind::AccessDenied
                && !item.required_authority.is_allowed
                && item.visibility == VisibilityClass::FarmPrivate
        }));
        assert!(items.iter().any(|item| {
            item.kind == StewardshipAccessItemKind::GroupManagementDeferred
                && item.is_phase_2_deferred
        }));
    }

    #[test]
    fn network_steward_can_perform_admin_lite_actions() {
        let items: Vec<StewardshipAccessItem> = fixture_stewardship_access_items(None)
            .into_iter()
            .filter(|item| item.actor == WorkflowActor::NetworkSteward && item.is_admin_lite)
            .collect();
        assert!(items.len() >= 6);

        for kind in [
            StewardshipAccessItemKind::AccessRequestReview,
            StewardshipAccessItemKind::RoleApproval,
            StewardshipAccessItemKind::RoutePartnerInvite,
            StewardshipAccessItemKind::RoutePoolMetadata,
            StewardshipAccessItemKind::PublicModeration,
        ] {
            let item = items
                .iter()
                .find(|item| item.kind == kind)
                .unwrap_or_else(|| panic!("missing {kind:?}"));
            assert!(item.required_authority.is_allowed);
            assert!(!item.grants_private_access);
            assert!(!item.is_phase_2_deferred);
        }

        let route_pool = items
            .iter()
            .find(|item| item.kind == StewardshipAccessItemKind::RoutePoolMetadata)
            .expect("route pool item");
        assert_eq!(
            route_pool.required_authority.domain,
            AuthorityDomain::NetworkStewardship
        );
        assert_eq!(
            route_pool.required_authority.action,
            AuthorityAction::Assign
        );

        let moderation = items
            .iter()
            .find(|item| item.kind == StewardshipAccessItemKind::PublicModeration)
            .expect("public moderation item");
        assert_eq!(
            moderation.required_authority.domain,
            AuthorityDomain::PublicPublishing
        );
    }

    #[test]
    fn phase_2_group_management_remains_deferred() {
        let deferred = fixture_stewardship_access_items(None)
            .into_iter()
            .find(|item| item.kind == StewardshipAccessItemKind::GroupManagementDeferred)
            .expect("deferred group management item");
        assert!(deferred.is_phase_2_deferred);
        assert!(deferred.is_admin_lite);
        assert!(!deferred.grants_private_access);
        assert_eq!(deferred.outbox_state, OutboxState::NotQueued);
    }

    #[test]
    fn network_steward_does_not_automatically_gain_private_workspace_access() {
        let steward = context_for_type(ContextType::NetworkSteward);
        for (domain, context_type) in [
            (AuthorityDomain::FarmWorkspaceOperations, ContextType::Farm),
            (AuthorityDomain::BuyerWorkspace, ContextType::Buyer),
            (AuthorityDomain::RouteCoordination, ContextType::Route),
            (AuthorityDomain::RouteExecution, ContextType::RoutePartner),
            (AuthorityDomain::TraceProof, ContextType::TraceRecords),
            (AuthorityDomain::Receipt, ContextType::PickupPoint),
        ] {
            let context = context_for_type(context_type);
            let gate = fixture_authority_gate(
                WorkflowActor::NetworkSteward,
                context,
                domain,
                AuthorityAction::Search,
            );
            assert!(
                !gate.is_allowed,
                "NetworkSteward unexpectedly gained {domain:?}"
            );
        }

        assert!(
            fixture_authority_gate(
                WorkflowActor::NetworkSteward,
                steward.clone(),
                AuthorityDomain::RelayGroupAccess,
                AuthorityAction::Approve,
            )
            .is_allowed
        );
        assert!(
            fixture_authority_gate(
                WorkflowActor::NetworkSteward,
                steward,
                AuthorityDomain::NetworkStewardship,
                AuthorityAction::Assign,
            )
            .is_allowed
        );
    }

    #[test]
    fn serde_names_preserve_product_vocabulary() {
        assert_eq!(
            serde_json::to_value(WorkflowActor::NetworkMember).expect("serialize actor"),
            "NetworkMember"
        );
        assert_eq!(
            serde_json::to_value(VisibilityClass::PublicProvenance).expect("serialize visibility"),
            "PublicProvenance"
        );
        assert_eq!(
            serde_json::to_value(AuthorityDomain::RelayGroupAccess).expect("serialize authority"),
            "Relay/group access"
        );
        assert_eq!(
            serde_json::to_value(AddActionType::BuyerCommitment).expect("serialize action"),
            "BuyerCommitment"
        );
    }

    #[test]
    fn product_surface_records_round_trip_through_json() {
        let card = TodayCard {
            id: "card_route_gap_001".to_string(),
            card_type: TodayCardType::Proof,
            source_object_refs: vec![object_ref(ObjectKind::Route, "route_123", "Thursday loop")],
            source_event_refs: vec![EventRef {
                event_id: "event_abc".to_string(),
                relay_url: Some("wss://relay.example".to_string()),
                kind: Some(1),
            }],
            primary_context: active_context(),
            actor: WorkflowActor::ProducerAdmin,
            visibility: VisibilityClass::RouteScoped,
            visibility_label: "route crew".to_string(),
            title: "Proof needed for Thursday loop".to_string(),
            status_line: "missing pickup confirmation".to_string(),
            detail_lines: vec!["2 stops need proof".to_string()],
            pills: vec!["blocking".to_string(), "route".to_string()],
            primary_action: TodayCardAction {
                id: "add_proof".to_string(),
                label: "Add proof".to_string(),
                action_type: Some(AddActionType::Proof),
                target_object: Some(object_ref(ObjectKind::Route, "route_123", "Thursday loop")),
            },
            secondary_action: None,
            ranking_reason: "blocking exception".to_string(),
            ranking_features: vec!["proof_gap".to_string()],
            sync_state: SyncState::Online,
            outbox_state: OutboxState::NotQueued,
            is_stale: false,
            is_offline: false,
        };

        let json = serde_json::to_string(&card).expect("serialize card");
        let decoded: TodayCard = serde_json::from_str(&json).expect("decode card");
        assert_eq!(decoded, card);
        assert!(json.contains("\"cardType\":\"Proof\""));
        assert!(json.contains("\"visibility\":\"RouteScoped\""));
    }

    #[test]
    fn add_action_and_outbox_contract_carry_authority_and_visibility() {
        let add_action = AddAction {
            action_type: AddActionType::PublicUpdate,
            display_label: "Public update".to_string(),
            allowed_context_types: vec![ContextType::Network, ContextType::Farm],
            required_authority: authority_gate(),
            default_visibility: VisibilityClass::PublicCommunity,
            allowed_visibility_options: vec![
                VisibilityClass::NetworkVisible,
                VisibilityClass::PublicCommunity,
            ],
            created_or_updated_object_type: ObjectKind::Update,
            related_object_requirements: vec![RelatedObjectRequirement {
                object_type: ObjectKind::Farm,
                relationship_label: "posted by".to_string(),
                is_required: true,
            }],
            validation_requirements: vec![ValidationRequirement {
                id: "non_empty_body".to_string(),
                label: "Body is required".to_string(),
                is_blocking: true,
            }],
            supports_offline: true,
            supports_draft: true,
            outbox_behavior: OutboxBehavior::PublishWhenAuthorized,
            primary_submit_label: "Publish".to_string(),
            completion_state: AddFlowState::ReadyToSubmit,
        };
        let outbox_item = OutboxItem {
            id: "outbox_001".to_string(),
            action_type: add_action.action_type,
            context: active_context(),
            object_refs: vec![object_ref(
                ObjectKind::Update,
                "draft_001",
                "Public update draft",
            )],
            event_refs: Vec::new(),
            visibility: add_action.default_visibility,
            authority_gate: add_action.required_authority.clone(),
            flow_state: AddFlowState::Queued,
            outbox_state: OutboxState::AwaitingAuthority,
            sync_state: SyncState::Offline,
            queued_at_unix: Some(1_799_971_200),
            last_attempt_at_unix: None,
            retry_count: 0,
            last_error: None,
        };

        assert_eq!(
            outbox_item.authority_gate.domain,
            AuthorityDomain::FarmWorkspaceOperations
        );
        assert_eq!(outbox_item.visibility, VisibilityClass::PublicCommunity);
        assert_eq!(outbox_item.flow_state, AddFlowState::Queued);
        assert_eq!(outbox_item.sync_state, SyncState::Offline);
    }

    #[test]
    fn outbox_retry_decision_rechecks_state_visibility_and_authority() {
        let failed = fixture_outbox_items()
            .into_iter()
            .find(|item| item.outbox_state == OutboxState::Failed)
            .expect("failed outbox fixture");

        let retryable = fixture_outbox_retry_decision(failed.clone());
        assert!(retryable.is_retryable);
        assert_eq!(retryable.item_id, failed.id);
        assert_eq!(retryable.authority_gate.action, AuthorityAction::Retry);
        assert!(retryable.reason.is_none());

        let mut queued = failed.clone();
        queued.outbox_state = OutboxState::Queued;
        let queued_decision = fixture_outbox_retry_decision(queued);
        assert!(!queued_decision.is_retryable);
        assert!(
            queued_decision
                .reason
                .as_deref()
                .expect("queued reason")
                .contains("not a retryable outbox state")
        );

        let mut secret = failed.clone();
        secret.visibility = VisibilityClass::SecretNeverShared;
        let secret_decision = fixture_outbox_retry_decision(secret);
        assert!(!secret_decision.is_retryable);
        assert!(
            secret_decision
                .reason
                .as_deref()
                .expect("secret reason")
                .contains("visibility cannot be retried")
        );

        let mut denied = failed;
        denied.authority_gate.domain = AuthorityDomain::BuyerWorkspace;
        let denied_decision = fixture_outbox_retry_decision(denied);
        assert!(!denied_decision.is_retryable);
        assert!(!denied_decision.authority_gate.is_allowed);
        assert!(denied_decision.reason.is_some());
    }

    #[test]
    fn compatibility_note_quarantines_low_level_roles() {
        assert!(WORKFLOW_ACTOR_COMPATIBILITY_NOTE.contains("Farmer"));
        assert!(WORKFLOW_ACTOR_COMPATIBILITY_NOTE.contains("Buyer"));
        assert!(WORKFLOW_ACTOR_COMPATIBILITY_NOTE.contains("not sufficient authority"));
    }

    #[test]
    fn authority_fixture_allows_and_denies_by_actor_domain_pair() {
        let context = active_context();
        let allowed = fixture_authority_gate(
            WorkflowActor::ProducerAdmin,
            context.clone(),
            AuthorityDomain::FarmWorkspaceOperations,
            AuthorityAction::Submit,
        );
        assert!(allowed.is_allowed);
        assert_eq!(allowed.reason, None);

        let denied = fixture_authority_gate(
            WorkflowActor::NetworkMember,
            context,
            AuthorityDomain::FarmWorkspaceOperations,
            AuthorityAction::Submit,
        );
        assert!(!denied.is_allowed);
        assert!(denied.reason.is_some());
    }
}
