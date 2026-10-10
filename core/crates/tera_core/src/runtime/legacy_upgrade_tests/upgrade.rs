use super::fixture::*;

#[tokio::test]
async fn pre_refactor_owner_bytes_upgrade_without_changing_any_acknowledged_identity() {
    let fixture = Fixture::copy();
    assert_eq!(fixture.expected["databases"]["runtime"]["user_version"], 13);
    assert_eq!(fixture.expected["draft_count"], 106);
    for _ in 0..2 {
        let runtime = fixture.open(false).await;
        fixture.assert_revisions(&runtime).await;
        fixture.assert_statuses(&runtime).await;
        runtime.shutdown().await.unwrap();
    }
    assert_eq!(
        hash(&std::fs::read(fixture.blob()).unwrap()),
        field(&fixture.host, "media_sha256")
    );
    fixture.copy_for_old_reader_probe();
}

#[tokio::test]
async fn legacy_missing_companion_refuses_upgrade_without_inventing_empty_state() {
    let fixture = Fixture::copy();
    let config = fixture.config(false);
    let private = config.owner_directory().join("private.sqlite");
    let retained = private.with_extension("retained");
    std::fs::rename(&private, &retained).unwrap();
    let runtime = config.owner_directory().join("runtime.sqlite");
    let before = hash(&std::fs::read(&runtime).unwrap());
    let result = crate::runtime::builder::RuntimeBuilder::new(config)
        .build()
        .await;
    match result {
        Err(crate::TeraAppError::Store { report }) => assert_eq!(report.code, "store_incomplete"),
        _ => panic!("Missing historical companion must produce the typed incomplete-store refusal"),
    }
    assert!(!private.exists());
    assert_eq!(hash(&std::fs::read(runtime).unwrap()), before);
    std::fs::rename(retained, private).unwrap();
    let runtime = fixture.open(false).await;
    fixture.assert_revisions(&runtime).await;
    fixture.assert_statuses(&runtime).await;
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_future_version_and_damaged_catalog_refuse_without_rewriting_acknowledged_bytes() {
    for name in ["unsupported_version", "damaged_catalog"] {
        let fixture = Fixture::copy();
        let config = fixture.config(false);
        let runtime_path = config.owner_directory().join("runtime.sqlite");
        let pristine = std::fs::read(&runtime_path).unwrap();
        let negative = std::fs::read(
            fixture
                .root
                .path()
                .join("refusal_inputs")
                .join(name)
                .join("runtime.sqlite"),
        )
        .unwrap();
        let derivative = &fixture.expected["negative_derivatives"][name];
        assert_eq!(hash(&pristine), field(derivative, "source_sha256"));
        assert_eq!(hash(&negative), field(derivative, "derived_sha256"));
        std::fs::write(&runtime_path, &negative).unwrap();
        let private_path = config.owner_directory().join("private.sqlite");
        let private_before = std::fs::read(&private_path).unwrap();
        let result = crate::runtime::builder::RuntimeBuilder::new(config)
            .build()
            .await;
        // This immutable version18 derivative was future input to its original
        // reader. Under the current reader it has an incomplete catalog. Keep
        // the historical bytes and qualify genuinely future input separately.
        let expected_code = "storage_integrity_failed";
        match result {
            Err(crate::TeraAppError::Sdk { report }) => assert_eq!(report.code, expected_code),
            _ => {
                panic!("Negative historical input must produce its specific typed storage refusal")
            }
        }
        assert_eq!(std::fs::read(&runtime_path).unwrap(), negative);
        assert_eq!(std::fs::read(private_path).unwrap(), private_before);
        // Recovery replaces only the disposable negative test copy with its
        // original already-hashed bytes. Production never rewinds a schema.
        std::fs::write(runtime_path, pristine).unwrap();
        let runtime = fixture.open(false).await;
        fixture.assert_revisions(&runtime).await;
        fixture.assert_statuses(&runtime).await;
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn current_future_schema_refuses_without_rewriting_either_acknowledged_owner() {
    let fixture = Fixture::copy();
    let config = fixture.config(false);
    let runtime = fixture.open(false).await;
    fixture.assert_revisions(&runtime).await;
    fixture.assert_statuses(&runtime).await;
    runtime.shutdown().await.unwrap();
    drop(runtime);

    let runtime_path = config.owner_directory().join("runtime.sqlite");
    let private_path = config.owner_directory().join("private.sqlite");
    let admitted = std::fs::read(&runtime_path).unwrap();
    let private_before = std::fs::read(&private_path).unwrap();
    assert_eq!(&admitted[..16], b"SQLite format 3\0");
    // SQLite's documented user_version is four big-endian bytes at offset60:
    // https://sqlite.org/fileformat2.html#user_version_number
    let current = u32::from_be_bytes(admitted[60..64].try_into().unwrap());
    assert!(current > 18);
    let future = current.checked_add(1).unwrap();
    assert!(future <= i32::MAX as u32);
    let mut negative = admitted.clone();
    negative[60..64].copy_from_slice(&future.to_be_bytes());
    std::fs::write(&runtime_path, &negative).unwrap();
    match crate::runtime::builder::RuntimeBuilder::new(config)
        .build()
        .await
    {
        Err(crate::TeraAppError::Sdk { report }) => assert_eq!(report.code, "schema_too_new"),
        _ => panic!("A genuinely future schema must produce its typed refusal"),
    }
    assert_eq!(std::fs::read(&runtime_path).unwrap(), negative);
    assert_eq!(std::fs::read(private_path).unwrap(), private_before);
    // Restore only this disposable test copy to its admitted current bytes.
    std::fs::write(runtime_path, admitted).unwrap();
    let runtime = fixture.open(false).await;
    fixture.assert_revisions(&runtime).await;
    fixture.assert_statuses(&runtime).await;
    runtime.shutdown().await.unwrap();
}
