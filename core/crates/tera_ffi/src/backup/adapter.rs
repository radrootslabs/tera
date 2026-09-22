use super::*;
use radroots_storage::outbox::BoxFuture;
use tera_core::runtime::backup::{
    ApplicationBackupManifest, BackupHost, BackupMediaLease, BackupMediaRequirement,
};

pub(crate) struct BackupHostAdapter(pub(crate) Box<dyn TeraBackupHost>);

impl BackupHost for BackupHostAdapter {
    fn load_candidate(
        &self,
        request: BackupRequest,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, BackupError>> {
        Box::pin(async move {
            self.0
                .load_candidate((&request).into())
                .await
                .map_err(host_error)
        })
    }
    fn retain_media(
        &self,
        request: BackupRequest,
        media: Vec<BackupMediaRequirement>,
    ) -> BoxFuture<'_, Result<Vec<BackupMediaLease>, BackupError>> {
        Box::pin(async move {
            let media = media
                .into_iter()
                .map(|media| FfiBackupMedia {
                    lease_identifier: request.lease_identifier(&media.sha256),
                    sha256: media.sha256,
                    byte_length: media.byte_length,
                })
                .collect();
            Ok(self
                .0
                .retain_media((&request).into(), media)
                .await
                .map_err(host_error)?
                .into_iter()
                .map(Into::into)
                .collect())
        })
    }
    fn persist_candidate(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async move {
            self.0
                .persist_candidate(manifest.try_into()?)
                .await
                .map_err(host_error)
        })
    }
    fn publish_complete(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async move {
            self.0
                .publish_complete(manifest.try_into()?)
                .await
                .map_err(host_error)
        })
    }
}

impl TryFrom<ApplicationBackupManifest> for FfiBackupManifest {
    type Error = BackupError;
    fn try_from(value: ApplicationBackupManifest) -> Result<Self, Self::Error> {
        Ok(Self {
            request: value.request().into(),
            media: value
                .media()
                .iter()
                .map(|media| FfiBackupMedia {
                    sha256: media.sha256.clone(),
                    byte_length: media.byte_length,
                    lease_identifier: media.identifier.clone(),
                })
                .collect(),
            manifest: value.encode()?,
        })
    }
}
