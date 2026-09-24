use super::Phase1DraftError;
impl Phase1DraftError {
    pub(in crate::runtime::product_surface) fn storage_error(
        error: radroots_storage::Error,
    ) -> Self {
        match error {
            radroots_storage::Error::SpaceInsufficient => Self::SpaceInsufficient,
            _ => Self::Storage,
        }
    }
    pub(in crate::runtime::product_surface) fn sync_error(error: radroots_sync::Error) -> Self {
        match error {
            radroots_sync::Error::StorageSpaceInsufficient => Self::SpaceInsufficient,
            _ => Self::Operation,
        }
    }
}
