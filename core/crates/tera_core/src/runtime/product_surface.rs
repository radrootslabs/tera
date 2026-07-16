//! Phase 1 product-surface contracts for native Radroots clients.
//!
//! These DTOs are the shared Rust vocabulary for the iOS `Today | Add`
//! surface. They deliberately model workflow actors, visibility, authority,
//! Today cards, Add actions, object pages, and outbox state separately from
//! low-level Nostr protocol roles or legacy trade/listing APIs.

use serde::{Deserialize, Serialize};

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
        assert_eq!(CANONICAL_ADD_ACTION_TYPES.len(), 18);
        assert_eq!(CANONICAL_ADD_FLOW_STATES.len(), 16);
        assert_eq!(CANONICAL_OBJECT_PAGE_FAMILIES.len(), 13);
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
    fn compatibility_note_quarantines_low_level_roles() {
        assert!(WORKFLOW_ACTOR_COMPATIBILITY_NOTE.contains("Farmer"));
        assert!(WORKFLOW_ACTOR_COMPATIBILITY_NOTE.contains("Buyer"));
        assert!(WORKFLOW_ACTOR_COMPATIBILITY_NOTE.contains("not sufficient authority"));
    }
}
