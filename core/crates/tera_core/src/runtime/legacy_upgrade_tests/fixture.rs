//! Current readers consume frozen bytes written by exact historical owners.
use crate::TeraRuntime;
use crate::runtime::{builder::RuntimeBuilder, store::*};
use radroots_storage::authored_draft::{AuthoredDraftId, AuthoredDraftRevision};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub host: Value,
    pub rust: Value,
    pub expected: Value,
}

pub(super) fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(super) fn bytes<const N: usize>(value: &str) -> [u8; N] {
    hex::decode(value).unwrap().try_into().unwrap()
}
pub(super) fn field<'a>(object: &'a Value, key: &str) -> &'a str {
    object[key].as_str().unwrap()
}

impl Fixture {
    pub fn copy() -> Self {
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../test-fixtures/legacy_upgrade_v1");
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(source.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            manifest["state"],
            "HISTORICAL_WRITER_ADMITTED_CURRENT_READERS_PENDING"
        );
        let root = tempfile::tempdir().unwrap();
        for (relative, expected) in manifest["files"].as_object().unwrap() {
            let path = Path::new(relative);
            assert!(
                !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
            );
            let mut input = source.clone();
            for component in path.components() {
                input.push(component.as_os_str());
                assert!(!input.symlink_metadata().unwrap().file_type().is_symlink());
            }
            let content = std::fs::read(input).unwrap();
            assert_eq!(hash(&content), expected.as_str().unwrap());
            let output = root.path().join(path);
            std::fs::create_dir_all(output.parent().unwrap()).unwrap();
            std::fs::write(output, content).unwrap();
        }
        let object = |name: &str| {
            serde_json::from_slice(&std::fs::read(root.path().join(name)).unwrap()).unwrap()
        };
        let host = object("host-metadata.json");
        let rust = object("rust-metadata.json");
        let expected = object("owner-expectations.json");
        assert_eq!(
            manifest["sources"][0]["commit"],
            "e25819267b51f659e5dfdf7b318239a8969bf45c"
        );
        Self {
            root,
            host,
            rust,
            expected,
        }
    }

    pub fn config(&self, backups: bool) -> MobileUserStoreConfig {
        let config = MobileUserStoreConfig::from_encoded(
            self.root.path().join("data"),
            field(&self.rust, "public_key"),
            field(&self.host, "source_generation"),
            self.host["generation_created_at_unix_ms"].as_u64().unwrap(),
            ProtectedDataAvailability::Available,
        )
        .unwrap();
        if backups {
            std::fs::create_dir_all(config.backup_directory()).unwrap();
            config.with_local_backups()
        } else {
            config
        }
    }

    pub fn copy_for_old_reader_probe(&self) {
        let Some(destination) = std::env::var_os("TERA_UPGRADE_PROBE_OUTPUT") else {
            return;
        };
        let destination = PathBuf::from(destination);
        let managed = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
            .canonicalize()
            .unwrap();
        assert!(destination.is_absolute() && destination.starts_with(&managed));
        assert!(
            destination
                .components()
                .all(|part| !matches!(part, Component::ParentDir))
        );
        assert!(!destination.exists());
        std::fs::create_dir_all(&destination).unwrap();
        let metadata = |name: &str| {
            std::fs::copy(self.root.path().join(name), destination.join(name)).unwrap()
        };
        metadata("host-metadata.json");
        metadata("rust-metadata.json");
        let owner = self.config(false).owner_directory().to_path_buf();
        let output = destination
            .join("data/radroots/users")
            .join(field(&self.rust, "public_key"));
        std::fs::create_dir_all(&output).unwrap();
        for name in ["runtime.sqlite", "private.sqlite"] {
            std::fs::copy(owner.join(name), output.join(name)).unwrap();
        }
    }

    pub fn blob(&self) -> PathBuf {
        self.root
            .path()
            .join(field(&self.host, "staged_relative_path"))
    }

    pub async fn open(&self, backups: bool) -> TeraRuntime {
        RuntimeBuilder::new(self.config(backups))
            .build()
            .await
            .unwrap()
    }

    pub async fn assert_revisions(&self, runtime: &TeraRuntime) {
        let storage = runtime.client.storage().unwrap();
        for expected in self.expected["revisions"].as_array().unwrap() {
            let actual = storage
                .authored_draft_revision(
                    AuthoredDraftId::new(bytes(field(expected, "draft_id"))).unwrap(),
                    AuthoredDraftRevision::new(expected["revision"].as_u64().unwrap()).unwrap(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                actual.author(),
                &bytes::<32>(field(expected, "author_public_key"))
            );
            assert_eq!(
                actual.operation_id().map(|id| hex::encode(id.as_bytes())),
                expected["operation_id"].as_str().map(str::to_owned)
            );
            assert_eq!(actual.payload_schema(), field(expected, "payload_schema"));
            assert_eq!(
                hex::encode(actual.payload_sha256()),
                field(expected, "payload_sha256")
            );
            assert_eq!(
                hash(&serde_json::to_vec(&actual).unwrap()),
                field(expected, "snapshot_sha256")
            );
        }
    }

    pub async fn assert_statuses(&self, runtime: &TeraRuntime) {
        assert_eq!(
            runtime.authenticated_store_public_key_hex().as_deref(),
            Some(field(&self.rust, "public_key"))
        );
        for expected in self.rust["drafts"].as_array().unwrap() {
            let actual = runtime
                .phase1_draft_status(bytes(field(expected, "draft_id")))
                .await
                .unwrap();
            assert_eq!(
                actual.draft().revision().get(),
                expected["revision"].as_u64().unwrap()
            );
            assert_eq!(actual.card_id().to_hex(), field(expected, "card_id"));
            assert_eq!(format!("{:?}", actual.state()), field(expected, "state"));
            assert_eq!(
                actual
                    .draft()
                    .operation_id()
                    .map(|id| hex::encode(id.as_bytes())),
                expected["operation_id"].as_str().map(str::to_owned)
            );
        }
        let signed = runtime
            .phase1_draft_status(bytes::<16>("00000000000000000000000000000003"))
            .await
            .unwrap();
        let signed = signed.push().unwrap().artifact().signed().unwrap().event();
        assert_eq!(
            signed.raw_json(),
            field(&self.expected["signed_event"], "raw_json")
        );
        assert_eq!(
            hash(signed.raw_json().as_bytes()),
            field(&self.expected["signed_event"], "raw_sha256")
        );
    }
}
