use radroots_event::admission::{SignatureVerifier, VisibilityPolicy};

// Shared typestates accept caller-supplied verifier/policy implementations.
// The app's public direct-ingest boundary must use its actual crypto/profile
// rules before writing even if another host supplied permissive evidence.
pub(super) struct PermissiveEvidence;
impl SignatureVerifier for PermissiveEvidence {
    fn verify_signature(
        &self,
        _: &radroots_event::envelope::EventEnvelope,
    ) -> Result<(), radroots_event::admission::Error> {
        Ok(())
    }
}
impl radroots_event::admission::AdmissionPolicy for PermissiveEvidence {
    type Error = std::convert::Infallible;
    fn policy_id(&self) -> &'static str {
        "tera.test.permissive-admission"
    }
    fn admit(
        &self,
        _: &radroots_event::admission::ContractValidatedEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
impl VisibilityPolicy for PermissiveEvidence {
    type Error = std::convert::Infallible;
    fn policy_id(&self) -> &'static str {
        "tera.test.permissive-visibility"
    }
    fn make_visible(
        &self,
        _: &radroots_event::admission::AdmittedEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
