use super::RecoveryDisposition;
use crate::runtime::product_surface::Phase1InboundMediaError;

pub(super) fn classify(code: &str) -> RecoveryDisposition {
    use RecoveryDisposition as Recovery;
    match code {
        "composer_owner_unavailable" | "composer_scope_mismatch" => Recovery::IdentityUnavailable,
        "composer_storage_failed"
        | "composer_record_corrupt"
        | "submission_storage_failed"
        | "submission_record_corrupt" => Recovery::StorageFailure,
        "composer_schema_unsupported"
        | "composer_revision_exhausted"
        | "submission_schema_unsupported" => Recovery::UnsupportedVersion,
        "composer_revision_conflict" | "composer_edit_sequence_conflict" => Recovery::StaleRevision,
        "composer_cursor_invalid" => Recovery::StaleCursor,
        "composer_receipt_mismatch" | "submission_receipt_mismatch" => Recovery::OutcomeUnknown,
        "submission_command_id_invalid"
        | "composer_id_invalid"
        | "composer_revision_invalid"
        | "composer_edit_sequence_invalid"
        | "composer_form_invalid"
        | "composer_scope_invalid"
        | "composer_not_found"
        | "composer_list_invalid" => Recovery::InvalidInput,
        "protected_data_unavailable" => Recovery::ProtectedDataUnavailable,
        "identity_unavailable" | "no_active_identity" | "unknown_identity" => {
            Recovery::IdentityUnavailable
        }
        "store_path_unavailable"
        | "invalid_store_configuration"
        | "settings_storage_unavailable"
        | "corrupt_settings_document"
        | "today_storage_failed"
        | "authoring_storage_failed"
        | "today_media_storage_failed" => Recovery::StorageFailure,
        "today_media_quota_exceeded" => Recovery::QuotaExhausted,
        "unsupported"
        | "unsupported_schema_version"
        | "unsupported_settings_schema"
        | "today_media_schema_unsupported"
        | "settings_revision_exhausted"
        | "operation_deadline_overflow"
        | "ios.support.unsupported"
        | "ios.add.unsupported" => Recovery::UnsupportedVersion,
        "draft_revision_conflict"
        | "settings_revision_conflict"
        | "today_refresh_required"
        | "today_state_failed"
        | "authoring_overlay_failed" => Recovery::StaleRevision,
        "today_cursor_invalid" | "draft_inventory_cursor_invalid" => Recovery::StaleCursor,
        "media_operation_already_used" | "identity_import_operation_mismatch" => {
            Recovery::IdempotencyConflict
        }
        "today_media_corrupt"
        | "draft_corrupt"
        | "blossom_media_type_mismatch"
        | "blossom_invalid_image_bytes"
        | "blossom_dimension_mismatch"
        | "blossom_response_size_mismatch"
        | "blossom_response_hash_mismatch"
        | "blossom_invalid_descriptor"
        | "blossom_descriptor_mismatch"
        | "blossom_retrieved_bytes_mismatch" => Recovery::MediaCorrupt,
        "blossom_resolution_failed" | "blossom_transport_failed" | "today_relay_offline" => {
            Recovery::NetworkUnavailable
        }
        "today_relay_partial" => Recovery::PartialResult,
        "writable_relay_unavailable"
        | "blossom_unconfigured"
        | "blossom_endpoint_not_configured"
        | "blossom_configuration_changed"
        | "blossom_endpoint_scheme_denied"
        | "blossom_resolved_address_denied"
        | "blossom_authorization_failed"
        | "blossom_http_status"
        | "blossom_unsafe_redirect"
        | "blossom_redirect_limit"
        | "blossom_content_encoding_denied"
        | "blossom_response_too_large" => Recovery::NetworkPolicy,
        "authoring_failed"
        | "ios.runtime.deadline_exceeded"
        | "ios.runtime.cancelled"
        | "blossom_timeout"
        | "blossom_cancelled"
        | "identity_import_already_pending" => Recovery::OutcomeUnknown,
        "initialization_failed"
        | "runtime_failed"
        | "today_runtime_unavailable"
        | "authoring_unavailable"
        | "operation_clock_unavailable" => Recovery::RuntimeUnavailable,
        "today_invalid_request"
        | "draft_invalid"
        | "draft_inventory_invalid"
        | "draft_media_invalid"
        | "draft_queue_policy_invalid"
        | "revision_invalid"
        | "today_media_invalid"
        | "invalid_today_page_request"
        | "today_as_of_required"
        | "invalid_existing_draft"
        | "invalid_card_id"
        | "invalid_media_cache_policy"
        | "invalid_identity_command"
        | "invalid_identity_id"
        | "invalid_public_key"
        | "invalid_identity_operation_id"
        | "duplicate_identity_id"
        | "duplicate_identity_public_key"
        | "invalid_profile_name"
        | "invalid_profile_display_name"
        | "invalid_profile_about"
        | "invalid_profile_nip05"
        | "unknown_relay_access"
        | "invalid_relay_endpoint"
        | "invalid_relay_endpoint_count"
        | "duplicate_relay_endpoint"
        | "invalid_blossom_endpoint"
        | "invalid_blossom_endpoint_count"
        | "network_environment_mismatch"
        | "invalid_media_cache_bytes"
        | "invalid_media_cache_artifacts"
        | "invalid_media_artifact_id"
        | "invalid_media_configuration"
        | "invalid_media_reference_fingerprint"
        | "invalid_blossom_authorization"
        | "blossom_invalid_endpoint"
        | "blossom_invalid_endpoint_count"
        | "blossom_duplicate_endpoint"
        | "blossom_invalid_limits"
        | "blossom_invalid_request"
        | "blossom_invalid_dimensions"
        | "blossom_unsupported_media_type" => Recovery::InvalidInput,
        _ => Recovery::Unknown,
    }
}

/// Preserve distinctions previously erased by the single Today media code.
pub fn inbound_media_code(error: &Phase1InboundMediaError) -> &'static str {
    use Phase1InboundMediaError as Error;
    match error {
        Error::CacheQuotaExceeded => "today_media_quota_exceeded",
        Error::UnsupportedSchema => "today_media_schema_unsupported",
        Error::CacheUnavailable | Error::CacheIo => "today_media_storage_failed",
        Error::MetadataMismatch
        | Error::ArtifactCollision
        | Error::CorruptReceipt
        | Error::CorruptState
        | Error::CorruptArtifact => "today_media_corrupt",
        Error::InvalidReference
        | Error::InvalidDigest
        | Error::MissingDigest
        | Error::InvalidMediaType
        | Error::InvalidDimensions
        | Error::InvalidByteSize
        | Error::InvalidAlt
        | Error::InvalidOperation
        | Error::OperationMismatch
        | Error::InvalidFailure
        | Error::InvalidConfiguration
        | Error::ConfigurationMismatch
        | Error::InvalidVerificationTime
        | Error::InvalidCachePolicy
        | Error::InvalidCacheObservation => "today_media_invalid",
    }
}
