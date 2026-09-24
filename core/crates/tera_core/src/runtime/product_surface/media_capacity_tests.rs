use super::*;

#[test]
fn capacity_limits_reject_unbounded_or_deserialized_policy_before_admission() {
    for (bytes, count) in [(0, 1), (1, 0), (u64::MAX, 1), (1, u32::MAX)] {
        assert!(Phase1MediaCachePolicy::new(bytes, count).is_err());
        let value = serde_json::json!({"maxBytes":bytes,"maxArtifacts":count});
        let policy: Phase1MediaCachePolicy = serde_json::from_value(value).unwrap();
        assert_eq!(
            Phase1MediaCacheIndex::validate_policy(policy),
            Err(Phase1InboundMediaError::InvalidCachePolicy)
        );
    }
    assert!(
        Phase1MediaCachePolicy::new(
            super::super::media_capacity::MEDIA_CACHE_MAX_BYTES,
            super::super::media_capacity::MEDIA_CACHE_MAX_ARTIFACTS
        )
        .is_ok()
    );
}

#[cfg(feature = "mobile-social")]
#[test]
fn capacity_io_classification_preserves_typed_exhaustion_without_diagnostics() {
    for kind in [
        std::io::ErrorKind::StorageFull,
        std::io::ErrorKind::QuotaExceeded,
    ] {
        let error = cache_io_error(std::io::Error::new(kind, "PRIVATE PATH"));
        assert_eq!(error, Phase1InboundMediaError::SpaceInsufficient);
        assert!(!error.to_string().contains("PRIVATE"));
    }
    assert_eq!(
        cache_io_error(std::io::ErrorKind::PermissionDenied.into()),
        Phase1InboundMediaError::CacheIo
    );
}
