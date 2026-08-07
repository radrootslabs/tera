//! Host-custodied mobile identity presentation over the SDK signer slot.

use super::RadrootsRuntime;
use crate::RadrootsAppError;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NostrIdentityRecord {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
    pub label: Option<String>,
    pub is_selected: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NostrIdentitySnapshot {
    pub has_selected_signing_identity: bool,
    pub selected_identity_id: Option<String>,
    pub selected_npub: Option<String>,
    pub identities: Vec<NostrIdentityRecord>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NostrHostCustodyIdentity {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
}

fn host_identity(identity: &radroots_sdk::signing::LocalIdentity) -> NostrHostCustodyIdentity {
    let public_key_hex = identity.public_key_hex();
    NostrHostCustodyIdentity {
        id: public_key_hex.clone(),
        public_key_hex,
        public_key_npub: identity.npub().to_owned(),
    }
}

fn identity_record(
    identity: &radroots_sdk::signing::LocalIdentity,
    label: Option<String>,
) -> NostrIdentityRecord {
    let identity = host_identity(identity);
    NostrIdentityRecord {
        id: identity.id,
        public_key_hex: identity.public_key_hex,
        public_key_npub: identity.public_key_npub,
        label,
        is_selected: true,
    }
}

impl RadrootsRuntime {
    pub fn nostr_identity_has_selected_signing_identity(&self) -> bool {
        self.signing_slot.identity().is_some()
    }

    pub fn nostr_identity_selected_npub(&self) -> Option<String> {
        self.signing_slot
            .identity()
            .map(|identity| identity.npub().to_owned())
    }

    pub fn nostr_identity_list(&self) -> Result<Vec<NostrIdentityRecord>, RadrootsAppError> {
        let Some(identity) = self.signing_slot.identity() else {
            return Ok(Vec::new());
        };
        Ok(vec![identity_record(&identity, self.identity_label())])
    }

    pub fn nostr_identity_list_ids(&self) -> Result<Vec<String>, RadrootsAppError> {
        Ok(self
            .nostr_identity_list()?
            .into_iter()
            .map(|identity| identity.id)
            .collect())
    }

    pub fn nostr_identity_snapshot(&self) -> Result<NostrIdentitySnapshot, RadrootsAppError> {
        let identities = self.nostr_identity_list()?;
        let selected = identities.first();
        Ok(NostrIdentitySnapshot {
            has_selected_signing_identity: selected.is_some(),
            selected_identity_id: selected.map(|identity| identity.id.clone()),
            selected_npub: selected.map(|identity| identity.public_key_npub.clone()),
            identities,
        })
    }

    pub fn nostr_identity_validate_host_custody_secret(
        &self,
        secret_key: String,
    ) -> Result<NostrHostCustodyIdentity, RadrootsAppError> {
        let slot = radroots_sdk::signing::Slot::new();
        let identity = slot
            .install(secret_key.as_str())
            .map_err(|_| RadrootsAppError::runtime("identity secret is invalid"))?;
        slot.clear();
        Ok(host_identity(&identity))
    }

    pub fn nostr_identity_restore_host_custody_secret(
        &self,
        secret_key: String,
        label: Option<String>,
        make_selected: bool,
    ) -> Result<NostrIdentityRecord, RadrootsAppError> {
        if !make_selected {
            let identity = self.nostr_identity_validate_host_custody_secret(secret_key)?;
            return Ok(NostrIdentityRecord {
                id: identity.id,
                public_key_hex: identity.public_key_hex,
                public_key_npub: identity.public_key_npub,
                label,
                is_selected: false,
            });
        }
        let identity = self
            .signing_slot
            .install(secret_key.as_str())
            .map_err(|_| RadrootsAppError::runtime("identity secret is invalid"))?;
        if self
            .store_public_key
            .is_some_and(|expected| expected.to_hex() != identity.public_key_hex())
        {
            self.signing_slot.clear();
            return Err(RadrootsAppError::runtime(
                "identity does not match the authenticated user store",
            ));
        }
        self.set_identity_label(label.clone())?;
        Ok(identity_record(&identity, label))
    }

    pub fn nostr_identity_select(&self, identity_id: String) -> Result<(), RadrootsAppError> {
        let current = self
            .signing_slot
            .identity()
            .ok_or_else(|| RadrootsAppError::runtime("identity is not installed"))?;
        if current.public_key_hex() != identity_id {
            return Err(RadrootsAppError::runtime("identity is not installed"));
        }
        Ok(())
    }

    pub fn nostr_identity_remove(&self, identity_id: String) -> Result<(), RadrootsAppError> {
        if self
            .signing_slot
            .identity()
            .is_some_and(|identity| identity.public_key_hex() == identity_id)
        {
            self.signing_slot.clear();
            self.set_identity_label(None)?;
        }
        Ok(())
    }

    pub fn nostr_identity_lock_host_custody_runtime(&self) -> Result<(), RadrootsAppError> {
        self.signing_slot.clear();
        self.set_identity_label(None)
    }

    pub fn nostr_identity_reset_host_custody_runtime(&self) -> Result<(), RadrootsAppError> {
        self.nostr_identity_lock_host_custody_runtime()
    }

    fn identity_label(&self) -> Option<String> {
        self.identity_label
            .read()
            .ok()
            .and_then(|label| label.clone())
    }

    fn set_identity_label(&self, label: Option<String>) -> Result<(), RadrootsAppError> {
        let mut current = self
            .identity_label
            .write()
            .map_err(|_| RadrootsAppError::runtime("identity label state is unavailable"))?;
        *current = label;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn validation_does_not_select_and_restore_is_single_slot() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
        let validated = runtime
            .nostr_identity_validate_host_custody_secret(SECRET.to_owned())
            .expect("valid secret");
        assert!(!runtime.nostr_identity_has_selected_signing_identity());

        let staged = runtime
            .nostr_identity_restore_host_custody_secret(
                SECRET.to_owned(),
                Some("staged".to_owned()),
                false,
            )
            .expect("staged");
        assert_eq!(staged.id, validated.id);
        assert!(!staged.is_selected);

        let selected = runtime
            .nostr_identity_restore_host_custody_secret(
                SECRET.to_owned(),
                Some("selected".to_owned()),
                true,
            )
            .expect("selected");
        assert!(selected.is_selected);
        assert_eq!(runtime.nostr_identity_list().expect("list"), vec![selected]);
        runtime
            .nostr_identity_lock_host_custody_runtime()
            .expect("lock");
        assert!(runtime.nostr_identity_list().expect("list").is_empty());
    }
}
