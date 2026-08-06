//! Bounded mobile Nostr presentation over shared SDK operations.

use super::RadrootsRuntime;
use crate::RadrootsAppError;

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NostrLight {
    Red,
    Yellow,
    Green,
}

#[derive(uniffi::Record, Debug, Clone, Eq, PartialEq)]
pub struct NostrConnectionStatus {
    pub light: NostrLight,
    pub configured: bool,
    pub source_available: bool,
    pub sink_available: bool,
    pub last_error: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, Default, Eq, PartialEq)]
pub struct NostrProfile {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
    pub website: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub lud06: Option<String>,
    pub lud16: Option<String>,
    pub bot: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, Eq, PartialEq)]
pub struct NostrProfileEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub profile: NostrProfile,
}

#[derive(uniffi::Record, Debug, Clone, Eq, PartialEq)]
pub struct NostrPost {
    pub content: String,
}

#[derive(uniffi::Record, Debug, Clone, Eq, PartialEq)]
pub struct NostrPostEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub post: NostrPost,
}

fn map_profile(event: radroots_sdk::client::ProfileEvent) -> NostrProfileEventMetadata {
    NostrProfileEventMetadata {
        id: event.event_id().to_owned(),
        author: event.author().to_owned(),
        published_at: event.created_at(),
        profile: NostrProfile {
            name: event.name().map(str::to_owned),
            display_name: event.display_name().map(str::to_owned),
            nip05: event.nip05().map(str::to_owned),
            about: event.about().map(str::to_owned),
            website: None,
            picture: event.picture().map(str::to_owned),
            banner: event.banner().map(str::to_owned),
            lud06: None,
            lud16: None,
            bot: event.bot().map(|value| value.to_string()),
        },
    }
}

fn map_post(event: radroots_sdk::client::PostEvent) -> NostrPostEventMetadata {
    NostrPostEventMetadata {
        id: event.event_id().to_owned(),
        author: event.author().to_owned(),
        published_at: event.created_at(),
        post: NostrPost {
            content: event.content().to_owned(),
        },
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    pub fn nostr_set_default_relays(&self, relays: Vec<String>) -> Result<(), RadrootsAppError> {
        self.nostr_slot
            .configure(relays)
            .map_err(RadrootsAppError::from_sdk)
    }

    /// Validates readiness; relay connections remain operation-scoped.
    pub fn nostr_connect_if_key_present(&self) -> Result<(), RadrootsAppError> {
        if self.signing_slot.identity().is_none() {
            return Err(RadrootsAppError::runtime("identity is not installed"));
        }
        if self.nostr_slot.targets().is_none() {
            return Err(RadrootsAppError::runtime(
                "relay selection is not configured",
            ));
        }
        Ok(())
    }

    pub async fn nostr_connection_status(&self) -> Result<NostrConnectionStatus, RadrootsAppError> {
        let social = self.client.social().map_err(RadrootsAppError::from_sdk)?;
        let health = social
            .transport_health()
            .await
            .map_err(RadrootsAppError::from_sdk)?;
        let light = if health.is_source_available() && health.is_sink_available() {
            NostrLight::Green
        } else if health.is_configured() {
            NostrLight::Yellow
        } else {
            NostrLight::Red
        };
        Ok(NostrConnectionStatus {
            light,
            configured: health.is_configured(),
            source_available: health.is_source_available(),
            sink_available: health.is_sink_available(),
            last_error: None,
        })
    }

    pub async fn nostr_profile_for_self(
        &self,
    ) -> Result<Option<NostrProfileEventMetadata>, RadrootsAppError> {
        self.client
            .social()
            .map_err(RadrootsAppError::from_sdk)?
            .fetch_profile_for_signer()
            .await
            .map(|profile| profile.map(map_profile))
            .map_err(RadrootsAppError::from_sdk)
    }

    pub async fn nostr_post_profile(
        &self,
        name: Option<String>,
        display_name: Option<String>,
        nip05: Option<String>,
        about: Option<String>,
    ) -> Result<String, RadrootsAppError> {
        let name = name
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| RadrootsAppError::runtime("profile name is required"))?;
        let mut draft = radroots_sdk::client::ProfileDraft::new(name);
        if let Some(value) = display_name.filter(|value| !value.is_empty()) {
            draft = draft.with_display_name(value);
        }
        if let Some(value) = nip05.filter(|value| !value.is_empty()) {
            draft = draft.with_nip05(value);
        }
        if let Some(value) = about.filter(|value| !value.is_empty()) {
            draft = draft.with_about(value);
        }
        self.client
            .social()
            .map_err(RadrootsAppError::from_sdk)?
            .publish_profile(draft)
            .await
            .map(|receipt| receipt.event_id().to_owned())
            .map_err(RadrootsAppError::from_sdk)
    }

    pub async fn nostr_post_text_note(&self, content: String) -> Result<String, RadrootsAppError> {
        self.client
            .social()
            .map_err(RadrootsAppError::from_sdk)?
            .publish_text(content)
            .await
            .map(|receipt| receipt.event_id().to_owned())
            .map_err(RadrootsAppError::from_sdk)
    }

    pub async fn nostr_fetch_text_notes(
        &self,
        limit: u16,
        since_unix: Option<u64>,
    ) -> Result<Vec<NostrPostEventMetadata>, RadrootsAppError> {
        self.client
            .social()
            .map_err(RadrootsAppError::from_sdk)?
            .fetch_posts(limit, since_unix)
            .await
            .map(|events| events.into_iter().map(map_post).collect())
            .map_err(RadrootsAppError::from_sdk)
    }

    pub async fn nostr_post_reply(
        &self,
        parent_event_id_hex: String,
        parent_author_hex: String,
        content: String,
        root_event_id_hex: Option<String>,
    ) -> Result<String, RadrootsAppError> {
        if root_event_id_hex
            .as_deref()
            .is_some_and(|root| root != parent_event_id_hex.as_str())
        {
            return Err(RadrootsAppError::unsupported(
                "nested reply author context is required",
            ));
        }
        self.client
            .social()
            .map_err(RadrootsAppError::from_sdk)?
            .publish_reply(
                content,
                parent_event_id_hex.as_str(),
                parent_author_hex.as_str(),
                None,
            )
            .await
            .map(|receipt| receipt.event_id().to_owned())
            .map_err(RadrootsAppError::from_sdk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn relay_configuration_is_explicit_and_status_is_categorical() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        let initial = runtime
            .nostr_connection_status()
            .await
            .expect("initial status");
        assert_eq!(initial.light, NostrLight::Red);
        assert!(!initial.configured);
        assert!(runtime.nostr_set_default_relays(Vec::new()).is_err());
    }
}
