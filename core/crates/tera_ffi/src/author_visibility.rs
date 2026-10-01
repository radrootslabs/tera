use crate::{FfiRuntimeChangeKind, TeraAppError, TeraRuntime};
use tera_core::runtime::visibility::{AuthorVisibility, AuthorVisibilityPolicy};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiAuthorVisibility {
    Visible,
    Muted,
    Blocked,
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiAuthorVisibilityEntry {
    pub author: String,
    pub visibility: FfiAuthorVisibility,
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiAuthorVisibilityPolicy {
    pub revision: u64,
    pub entries: Vec<FfiAuthorVisibilityEntry>,
}

impl From<AuthorVisibilityPolicy> for FfiAuthorVisibilityPolicy {
    fn from(value: AuthorVisibilityPolicy) -> Self {
        Self {
            revision: value.revision(),
            entries: value
                .entries()
                .iter()
                .map(|(author, mode)| FfiAuthorVisibilityEntry {
                    author: author.clone(),
                    visibility: match mode {
                        AuthorVisibility::Visible => FfiAuthorVisibility::Visible,
                        AuthorVisibility::Muted => FfiAuthorVisibility::Muted,
                        AuthorVisibility::Blocked => FfiAuthorVisibility::Blocked,
                    },
                })
                .collect(),
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl TeraRuntime {
    pub async fn author_visibility(&self) -> Result<FfiAuthorVisibilityPolicy, TeraAppError> {
        Ok(self.inner.author_visibility().await?.into())
    }

    pub async fn set_author_visibility(
        &self,
        author: String,
        visibility: FfiAuthorVisibility,
    ) -> Result<FfiAuthorVisibilityPolicy, TeraAppError> {
        let mode = match visibility {
            FfiAuthorVisibility::Visible => AuthorVisibility::Visible,
            FfiAuthorVisibility::Muted => AuthorVisibility::Muted,
            FfiAuthorVisibility::Blocked => AuthorVisibility::Blocked,
        };
        let result = self.inner.set_author_visibility(&author, mode).await;
        // Resnapshot even on uncertainty. No author is sent in notifications.
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Settings, None);
        self.subscriptions.notify(FfiRuntimeChangeKind::Today, None);
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        Ok(result?.into())
    }
}
