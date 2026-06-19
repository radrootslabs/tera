use super::RadrootsRuntime;
use crate::RadrootsAppError;
#[cfg(feature = "nostr-client")]
use tokio::sync::broadcast::error::TryRecvError;

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NostrLight {
    Red,
    Yellow,
    Green,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct NostrConnectionStatus {
    pub light: NostrLight,
    pub connected: u32,
    pub connecting: u32,
    pub last_error: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, Default)]
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

#[derive(uniffi::Record, Debug, Clone)]
pub struct NostrProfileEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub profile: NostrProfile,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct NostrEvent {
    pub id: String,
    pub author: String,
    pub created_at: u64,
    pub kind: u32,
    pub content: String,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct NostrPost {
    pub content: String,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct NostrPostEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub post: NostrPost,
}

#[cfg(feature = "nostr-client")]
fn map_post_event_metadata(
    event: radroots_events_codec::parsed::RadrootsParsedData<radroots_events::post::RadrootsPost>,
) -> NostrPostEventMetadata {
    NostrPostEventMetadata {
        id: event.id,
        author: event.author,
        published_at: event.published_at as u64,
        post: NostrPost {
            content: event.data.content,
        },
    }
}

#[cfg(feature = "nostr-client")]
fn map_profile_event_metadata(
    event: radroots_events_codec::parsed::RadrootsParsedData<
        radroots_events_codec::profile::RadrootsProfileData,
    >,
) -> NostrProfileEventMetadata {
    NostrProfileEventMetadata {
        id: event.id,
        author: event.author,
        published_at: event.published_at as u64,
        profile: NostrProfile {
            name: event.data.profile.name.into(),
            display_name: event.data.profile.display_name.into(),
            nip05: event.data.profile.nip05.into(),
            about: event.data.profile.about.into(),
            website: event.data.profile.website,
            picture: event.data.profile.picture,
            banner: event.data.profile.banner,
            lud06: event.data.profile.lud06,
            lud16: event.data.profile.lud16,
            bot: event.data.profile.bot,
        },
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    pub fn nostr_set_default_relays(&self, relays: Vec<String>) -> Result<(), RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let mut guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            guard
                .nostr_set_default_relays(&relays)
                .map_err(|e| RadrootsAppError::relay(format!("{e}")))
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            let _ = relays;
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_connect_if_key_present(&self) -> Result<(), RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let mut guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            guard
                .nostr_connect_if_key_present()
                .map_err(|e| RadrootsAppError::relay(format!("{e}")))
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_connection_status(&self) -> NostrConnectionStatus {
        #[cfg(feature = "nostr-client")]
        {
            let guard = self.net.lock();
            if let Ok(g) = guard {
                if let Some(s) = g.nostr_connection_snapshot() {
                    let light = match s.light {
                        radroots_net_core::nostr_client::Light::Green => NostrLight::Green,
                        radroots_net_core::nostr_client::Light::Yellow => NostrLight::Yellow,
                        radroots_net_core::nostr_client::Light::Red => NostrLight::Red,
                    };
                    return NostrConnectionStatus {
                        light,
                        connected: s.connected as u32,
                        connecting: s.connecting as u32,
                        last_error: s.last_error,
                    };
                }
            }
            NostrConnectionStatus {
                light: NostrLight::Red,
                connected: 0,
                connecting: 0,
                last_error: None,
            }
        }

        #[cfg(not(feature = "nostr-client"))]
        {
            NostrConnectionStatus {
                light: NostrLight::Red,
                connected: 0,
                connecting: 0,
                last_error: None,
            }
        }
    }

    pub fn nostr_profile_for_self(
        &self,
    ) -> Result<Option<NostrProfileEventMetadata>, RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let keys = guard.selected_nostr_keys().ok_or_else(|| {
                RadrootsAppError::identity("selected signing identity is not configured")
            })?;
            let pk = keys.public_key();
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            let out = mgr
                .fetch_profile_event_blocking(pk)
                .map_err(|error| RadrootsAppError::relay(error.to_string()))?;
            Ok(out.map(map_profile_event_metadata))
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_post_profile(
        &self,
        name: Option<String>,
        display_name: Option<String>,
        nip05: Option<String>,
        about: Option<String>,
    ) -> Result<String, RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            mgr.publish_profile_event_blocking(name, display_name, nip05, about)
                .map_err(|e| RadrootsAppError::relay(e.to_string()))
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            let _ = (name, display_name, nip05, about);
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_post_text_note(&self, content: String) -> Result<String, RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            mgr.publish_post_event_blocking(content)
                .map_err(|e| RadrootsAppError::relay(e.to_string()))
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            let _ = content;
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_fetch_text_notes(
        &self,
        limit: u16,
        since_unix: Option<u64>,
    ) -> Result<Vec<NostrPostEventMetadata>, RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            let items = mgr
                .fetch_post_events_blocking(limit, since_unix)
                .map_err(|e| RadrootsAppError::relay(e.to_string()))?;
            Ok(items.into_iter().map(map_post_event_metadata).collect())
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            let _ = (limit, since_unix);
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_post_reply(
        &self,
        parent_event_id_hex: String,
        parent_author_hex: String,
        content: String,
        root_event_id_hex: Option<String>,
    ) -> Result<String, RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            mgr.publish_post_reply_event_blocking(
                parent_event_id_hex,
                parent_author_hex,
                content,
                root_event_id_hex,
            )
            .map_err(|e| RadrootsAppError::relay(e.to_string()))
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            let _ = (
                parent_event_id_hex,
                parent_author_hex,
                content,
                root_event_id_hex,
            );
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_start_post_event_stream(
        &self,
        since_unix: Option<u64>,
    ) -> Result<(), RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            mgr.start_post_event_stream(since_unix);
            if let Ok(mut rx_guard) = self.post_events_rx.lock() {
                if rx_guard.is_none() {
                    *rx_guard = Some(mgr.subscribe_post_events());
                }
            }
            Ok(())
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            let _ = since_unix;
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_next_post_event(
        &self,
    ) -> Result<Option<NostrPostEventMetadata>, RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let mut rx_guard = match self.post_events_rx.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let Some(rx) = rx_guard.as_mut() else {
                return Ok(None);
            };
            match rx.try_recv() {
                Ok(event) => Ok(Some(map_post_event_metadata(event))),
                Err(TryRecvError::Empty) => Ok(None),
                Err(TryRecvError::Lagged(count)) => Err(RadrootsAppError::relay(format!(
                    "post event stream lagged by {count} events"
                ))),
                Err(TryRecvError::Closed) => {
                    *rx_guard = None;
                    Err(RadrootsAppError::relay("post event stream closed"))
                }
            }
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }

    pub fn nostr_stop_post_event_stream(&self) -> Result<(), RadrootsAppError> {
        #[cfg(feature = "nostr-client")]
        {
            let guard = match self.net.lock() {
                Ok(guard) => guard,
                Err(err) => return Err(RadrootsAppError::runtime(format!("{err}"))),
            };
            let mgr = guard
                .nostr
                .as_ref()
                .ok_or_else(|| RadrootsAppError::relay("nostr not initialized"))?;
            mgr.stop_post_event_stream();
            if let Ok(mut rx_guard) = self.post_events_rx.lock() {
                *rx_guard = None;
            }
            Ok(())
        }
        #[cfg(not(feature = "nostr-client"))]
        {
            Err(RadrootsAppError::unsupported("nostr disabled"))
        }
    }
}
