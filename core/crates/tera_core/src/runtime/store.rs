//! Validated host contract for one authenticated mobile user's durable store.

use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};

use radroots_identity::PublicKey;
use radroots_storage::event::SourceGeneration;

use crate::RadrootsAppError;

const PRODUCT_DIRECTORY: &str = "radroots";
const USER_DIRECTORY: &str = "users";
const GENERATION_HEX_LENGTH: usize = 64;

/// Host-observed Apple protected-data state at runtime construction time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectedDataAvailability {
    Available,
    Unavailable,
}

/// Validated composition for one authenticated user's SQLite owner directory.
///
/// The Apple host owns directory creation and data-protection attributes. Rust
/// derives the exact identity-scoped suffix and refuses alternate, relative,
/// or symlinked directory layouts before SQLite is opened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MobileUserStoreConfig {
    application_support_directory: PathBuf,
    owner_directory: PathBuf,
    public_key: PublicKey,
    source_generation: SourceGeneration,
    source_generation_created_at_unix_ms: u64,
    protected_data: ProtectedDataAvailability,
}

impl MobileUserStoreConfig {
    /// Validates encoded host values without touching SQLite.
    pub fn from_encoded(
        application_support_directory: impl Into<PathBuf>,
        public_key_hex: &str,
        source_generation_hex: &str,
        source_generation_created_at_unix_ms: u64,
        protected_data: ProtectedDataAvailability,
    ) -> Result<Self, RadrootsAppError> {
        let public_key = PublicKey::from_hex(public_key_hex)
            .map_err(|_| RadrootsAppError::store_invalid_configuration())?;
        let source_generation = parse_source_generation(source_generation_hex)?;
        Self::new(
            application_support_directory,
            public_key,
            source_generation,
            source_generation_created_at_unix_ms,
            protected_data,
        )
    }

    /// Creates a validated store configuration from canonical typed values.
    pub fn new(
        application_support_directory: impl Into<PathBuf>,
        public_key: PublicKey,
        source_generation: SourceGeneration,
        source_generation_created_at_unix_ms: u64,
        protected_data: ProtectedDataAvailability,
    ) -> Result<Self, RadrootsAppError> {
        let application_support_directory = application_support_directory.into();
        validate_absolute_normal_directory(&application_support_directory)?;
        if source_generation_created_at_unix_ms == 0
            || i64::try_from(source_generation_created_at_unix_ms).is_err()
        {
            return Err(RadrootsAppError::store_invalid_configuration());
        }
        let owner_directory = application_support_directory
            .join(PRODUCT_DIRECTORY)
            .join(USER_DIRECTORY)
            .join(public_key.to_hex());
        Ok(Self {
            application_support_directory,
            owner_directory,
            public_key,
            source_generation,
            source_generation_created_at_unix_ms,
            protected_data,
        })
    }

    /// Returns the host-owned Application Support root.
    pub fn application_support_directory(&self) -> &Path {
        self.application_support_directory.as_path()
    }

    /// Returns the exact existing directory that must own both SQLite files.
    pub fn owner_directory(&self) -> &Path {
        self.owner_directory.as_path()
    }

    /// Returns the authenticated identity that scopes this store.
    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub(crate) const fn protected_data(&self) -> ProtectedDataAvailability {
        self.protected_data
    }

    pub(crate) fn validate_host_filesystem(&self) -> Result<(), RadrootsAppError> {
        let directories = [
            self.application_support_directory.clone(),
            self.application_support_directory
                .join(PRODUCT_DIRECTORY)
                .to_path_buf(),
            self.application_support_directory
                .join(PRODUCT_DIRECTORY)
                .join(USER_DIRECTORY)
                .to_path_buf(),
            self.owner_directory.clone(),
        ];
        for directory in directories {
            let metadata = std::fs::symlink_metadata(&directory)
                .map_err(|_| RadrootsAppError::store_path_unavailable())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(RadrootsAppError::store_invalid_configuration());
            }
        }
        Ok(())
    }

    pub(crate) fn sqlite_options(
        &self,
    ) -> Result<radroots_sdk::storage::SqliteOptions, RadrootsAppError> {
        let paths = radroots_sdk::storage::SqlitePaths::from_directory(&self.owner_directory)
            .map_err(|_| RadrootsAppError::store_invalid_configuration())?;
        radroots_sdk::storage::SqliteOptions::new(
            paths,
            radroots_sdk::storage::SqliteOpenMode::Create,
        )
        .with_busy_timeout(Duration::from_secs(5))
        .and_then(|options| {
            options.with_source_generation(
                self.source_generation,
                self.source_generation_created_at_unix_ms,
            )
        })
        .map_err(|_| RadrootsAppError::store_invalid_configuration())
    }
}

fn parse_source_generation(value: &str) -> Result<SourceGeneration, RadrootsAppError> {
    if value.len() != GENERATION_HEX_LENGTH {
        return Err(RadrootsAppError::store_invalid_configuration());
    }
    let bytes = hex::decode(value).map_err(|_| RadrootsAppError::store_invalid_configuration())?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| RadrootsAppError::store_invalid_configuration())?;
    SourceGeneration::new(bytes).map_err(|_| RadrootsAppError::store_invalid_configuration())
}

fn validate_absolute_normal_directory(path: &Path) -> Result<(), RadrootsAppError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(RadrootsAppError::store_invalid_configuration());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    const GENERATION: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    #[test]
    fn encoded_scope_derives_the_exact_user_directory() {
        let root = tempfile::tempdir().expect("tempdir");
        let config = MobileUserStoreConfig::from_encoded(
            root.path(),
            PUBLIC_KEY,
            GENERATION,
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .expect("config");
        assert_eq!(
            config.owner_directory(),
            root.path().join("radroots").join("users").join(PUBLIC_KEY)
        );
        assert_eq!(config.public_key().to_hex(), PUBLIC_KEY);
    }

    #[test]
    fn encoded_scope_rejects_invalid_identity_generation_time_and_path() {
        let root = tempfile::tempdir().expect("tempdir");
        for result in [
            MobileUserStoreConfig::from_encoded(
                "relative",
                PUBLIC_KEY,
                GENERATION,
                1,
                ProtectedDataAvailability::Available,
            ),
            MobileUserStoreConfig::from_encoded(
                root.path(),
                "bad",
                GENERATION,
                1,
                ProtectedDataAvailability::Available,
            ),
            MobileUserStoreConfig::from_encoded(
                root.path(),
                PUBLIC_KEY,
                "00",
                1,
                ProtectedDataAvailability::Available,
            ),
            MobileUserStoreConfig::from_encoded(
                root.path(),
                PUBLIC_KEY,
                GENERATION,
                0,
                ProtectedDataAvailability::Available,
            ),
        ] {
            assert!(matches!(result, Err(RadrootsAppError::Store { .. })));
        }
    }

    #[cfg(unix)]
    #[test]
    fn host_filesystem_rejects_a_symlinked_user_scope() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("tempdir");
        let config = MobileUserStoreConfig::from_encoded(
            root.path(),
            PUBLIC_KEY,
            GENERATION,
            1,
            ProtectedDataAvailability::Available,
        )
        .expect("config");
        std::fs::create_dir_all(root.path().join(PRODUCT_DIRECTORY).join(USER_DIRECTORY))
            .expect("parents");
        let target = tempfile::tempdir().expect("target");
        symlink(target.path(), config.owner_directory()).expect("symlink");
        assert!(matches!(
            config.validate_host_filesystem(),
            Err(RadrootsAppError::Store { .. })
        ));
    }
}
