use super::super::tests::context;
use super::*;

#[test]
fn sidecar_binds_context_and_does_not_serialize_a_feed() {
    let selected = context(None, 1);
    let source = EventSourceToken::new(
        radroots_storage::event::SourceGeneration::new([1; 32]).unwrap(),
        1,
    );
    let local = LocalState::new(
        &selected,
        source,
        ProjectionGeneration::new([2; 32]).unwrap(),
        [3; 32],
        None,
    );
    local.validate(&projection_document_key(&selected)).unwrap();
    assert!(
        local
            .validate(&projection_document_key(&context(None, 2)))
            .is_err()
    );
    let raw = String::from_utf8(encode(&local).unwrap()).unwrap();
    assert!(!raw.contains("cards") && !raw.contains("profiles") && !raw.contains("thread"));
    let decoded: LocalState = decode(raw.as_bytes()).unwrap();
    assert_eq!(decoded.revision().unwrap(), local.revision().unwrap());
}
