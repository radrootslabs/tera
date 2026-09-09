use serde::{Deserialize, Serialize};

use super::LocalNetworkError;

/// Stable local context identity, independent of any presentation session.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct LocalNetworkId(String);

impl LocalNetworkId {
    pub fn new(value: String) -> Result<Self, LocalNetworkError> {
        if value.is_empty()
            || value.len() > 256
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(LocalNetworkError::InvalidText { field: "id" });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl TryFrom<String> for LocalNetworkId {
    type Error = LocalNetworkError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<LocalNetworkId> for String {
    fn from(value: LocalNetworkId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_context_wire_value_round_trips_without_a_session_generation() {
        for value in ["nearby".to_owned(), "é".repeat(128)] {
            let id = LocalNetworkId::new(value.clone()).unwrap();
            let wire = serde_json::to_string(&id).unwrap();
            assert_eq!(wire, serde_json::to_string(&value).unwrap());
            assert_eq!(serde_json::from_str::<LocalNetworkId>(&wire).unwrap(), id);
            assert_eq!(String::from(id), value);
        }
    }

    #[test]
    fn constructors_and_deserialization_enforce_the_same_byte_bounds() {
        for value in [
            "".into(),
            " nearby".into(),
            "x\u{7f}".into(),
            "é".repeat(129),
        ] {
            assert!(LocalNetworkId::new(value.clone()).is_err());
            let wire = serde_json::to_string(&value).unwrap();
            assert!(serde_json::from_str::<LocalNetworkId>(&wire).is_err());
        }
    }
}
