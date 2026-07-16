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
        ) | (
            WorkflowActor::ProducerAdmin,
            AuthorityDomain::PublicPublishing
        ) | (
            WorkflowActor::FarmTeamMember,
            AuthorityDomain::FarmWorkspaceOperations
        ) | (
            WorkflowActor::HubOperator,
            AuthorityDomain::FarmWorkspaceOperations
        ) | (WorkflowActor::HubOperator, AuthorityDomain::RouteExecution)
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
