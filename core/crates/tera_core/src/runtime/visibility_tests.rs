use super::*;

fn author(n: usize) -> String {
    nostr::Keys::parse(&format!("{n:064x}"))
        .unwrap()
        .public_key()
        .to_string()
}

#[test]
fn bounds_idempotence_and_explicit_restore_are_exact() {
    let mut policy = AuthorVisibilityPolicy::default();
    for n in 1..=AUTHOR_VISIBILITY_MAX_ENTRIES {
        policy = policy.changed(&author(n), AuthorVisibility::Muted).unwrap();
    }
    assert_eq!(policy.entries.len(), AUTHOR_VISIBILITY_MAX_ENTRIES);
    let mut invalid_revision = policy.clone();
    invalid_revision.revision = 0;
    assert!(invalid_revision.validate().is_err());
    let mut oversized = policy.clone();
    oversized.entries.insert(
        author(AUTHOR_VISIBILITY_MAX_ENTRIES + 1),
        AuthorVisibility::Blocked,
    );
    assert!(oversized.validate().is_err());
    assert!(serde_json::to_vec(&policy).unwrap().len() <= AUTHOR_VISIBILITY_MAX_BYTES);
    assert!(
        policy
            .changed(
                &author(AUTHOR_VISIBILITY_MAX_ENTRIES + 1),
                AuthorVisibility::Blocked
            )
            .is_err()
    );
    assert_eq!(
        policy.changed(&author(1), AuthorVisibility::Muted).unwrap(),
        policy
    );
    let blocked = policy
        .changed(&author(1), AuthorVisibility::Blocked)
        .unwrap();
    assert_eq!(blocked.revision, policy.revision + 1);
    assert!(!blocked.allows(&author(1)));
    let restored = blocked
        .changed(&author(1), AuthorVisibility::Visible)
        .unwrap();
    assert!(restored.allows(&author(1)));
    assert_eq!(restored.entries.len(), AUTHOR_VISIBILITY_MAX_ENTRIES - 1);
    assert!(
        restored
            .changed(
                &author(AUTHOR_VISIBILITY_MAX_ENTRIES + 1),
                AuthorVisibility::Blocked
            )
            .is_ok()
    );
    assert!(policy.changed("BAD", AuthorVisibility::Blocked).is_err());
    let exhausted = AuthorVisibilityPolicy {
        revision: u64::MAX,
        ..Default::default()
    };
    assert!(
        exhausted
            .changed(&author(1), AuthorVisibility::Blocked)
            .is_err()
    );
    assert!(!format!("{policy:?}").contains(&author(1)));
}

#[tokio::test]
async fn corrupt_oversized_unsupported_and_uncertain_policy_never_becomes_permissive() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let (id, generation) = identity().unwrap();
    let storage = runtime.client.storage().unwrap();
    for bytes in [
        b"broken".to_vec(),
        vec![b' '; AUTHOR_VISIBILITY_MAX_BYTES + 1],
        br#"{"schema_version":2,"revision":0,"entries":{}}"#.to_vec(),
        format!(
            r#"{{"schema_version":1,"revision":1,"entries":{{"{}":"visible"}}}}"#,
            author(1)
        )
        .into_bytes(),
    ] {
        ProjectionStore::put_projection_document(
            storage,
            id.clone(),
            generation,
            ProjectionDocument::new(KEY.to_owned(), bytes).unwrap(),
        )
        .await
        .unwrap();
        assert!(runtime.author_visibility().await.is_err());
        assert!(
            runtime
                .set_author_visibility(&author(2), AuthorVisibility::Visible)
                .await
                .is_err()
        );
    }
    drop(runtime.author_visibility_fence.begin_write());
    assert!(matches!(
        runtime.author_visibility().await,
        Err(TodayError::RuntimeUnavailable)
    ));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn serialized_edits_do_not_lose_independent_authors() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let a = author(1);
    let b = author(2);
    let (left, right) = tokio::join!(
        runtime.set_author_visibility(&a, AuthorVisibility::Blocked),
        runtime.set_author_visibility(&b, AuthorVisibility::Muted)
    );
    left.unwrap();
    right.unwrap();
    let result = runtime.author_visibility().await.unwrap();
    assert_eq!(result.entries.len(), 2);
    assert_eq!(result.revision, 2);
    runtime.shutdown().await.unwrap();
}
