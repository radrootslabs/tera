use radroots_identity::PublicKey;
use radroots_sdk::{
    ClientBuilder,
    storage::{SqliteOpenMode, SqliteOptions, SqlitePaths},
};
use std::path::{Component, Path};

use super::*;

/// A host-fenced cold-start inspection. Native references must come from the
/// complete bounded persisted transfer inventory, before native task activation.
/// Never call this concurrently with a runtime or without the native fence.
pub async fn inspect_media_references(
    root: &Path,
    native_hashes: Vec<String>,
) -> Result<MediaReferenceInventory> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(MediaInventoryIncomplete);
    }
    directory(root)?;
    let mut hashes = BTreeSet::new();
    if native_hashes.len() > MEDIA_REFERENCE_BUDGET {
        return Err(MediaInventoryIncomplete);
    }
    for hash in native_hashes {
        retain(&mut hashes, hash)?;
    }
    let accounts = account_directories(root)?;
    let mut budget = InventoryBudget::default();
    for (author, path) in accounts {
        let paths = SqlitePaths::from_directory(path).map_err(|_| MediaInventoryIncomplete)?;
        let options = SqliteOptions::new(paths, SqliteOpenMode::ReadOnly)
            .with_busy_timeout(std::time::Duration::from_secs(1))
            .map_err(|_| MediaInventoryIncomplete)?;
        let client = ClientBuilder::sqlite(options)
            .await
            .map_err(|_| MediaInventoryIncomplete)?
            .build()
            .map_err(|_| MediaInventoryIncomplete)?;
        let result = match client.storage() {
            Ok(store) => collect_account(store, author, &mut hashes, &mut budget, None).await,
            Err(_) => Err(MediaInventoryIncomplete),
        };
        // Close even after a failed traversal. A failed close cannot authorize GC.
        let closed = client.close().await.map_err(|_| MediaInventoryIncomplete);
        closed?;
        result?;
    }
    Ok(MediaReferenceInventory { hashes })
}

fn directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| MediaInventoryIncomplete)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(MediaInventoryIncomplete)
    }
}

fn absent_or_directory(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        _ => directory(path).map(|()| true),
    }
}

fn account_directories(root: &Path) -> Result<Vec<([u8; 32], std::path::PathBuf)>> {
    let product = root.join("radroots");
    let users = product.join("users");
    if !absent_or_directory(&product)? || !absent_or_directory(&users)? {
        return Ok(Vec::new());
    }
    let mut accounts = Vec::new();
    for entry in std::fs::read_dir(users).map_err(|_| MediaInventoryIncomplete)? {
        if accounts.len() == MEDIA_ACCOUNT_LIMIT {
            return Err(MediaInventoryIncomplete);
        }
        let entry = entry.map_err(|_| MediaInventoryIncomplete)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| MediaInventoryIncomplete)?;
        if !canonical_hash(&name) {
            return Err(MediaInventoryIncomplete);
        }
        let author = PublicKey::from_hex(&name).map_err(|_| MediaInventoryIncomplete)?;
        directory(&entry.path())?;
        accounts.push((author.into_bytes(), entry.path()));
    }
    Ok(accounts)
}
