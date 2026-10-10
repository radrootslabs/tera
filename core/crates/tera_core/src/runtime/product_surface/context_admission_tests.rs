use super::*;

fn selected(authors: Vec<String>) -> Result<LocalNetwork, LocalNetworkError> {
    LocalNetwork::new(
        "nearby".into(),
        "Nearby".into(),
        vec!["wss://relay.example".into()],
        None,
        authors,
        7,
    )
}

#[test]
fn followed_author_aggregate_is_admitted_at_exact_boundary() {
    let authors = (0..4096)
        .map(|index| format!("{index:064x}"))
        .collect::<Vec<_>>();
    let context = selected(authors.clone()).unwrap();
    assert_eq!(context.followed_authors, authors);
    assert_eq!(
        context
            .followed_authors
            .iter()
            .map(String::len)
            .sum::<usize>(),
        262_144
    );
    let mut over = authors;
    over.push(format!("{:064x}", 4096));
    assert_eq!(selected(over), Err(LocalNetworkError::InvalidAuthor));
    assert!(selected(Vec::new()).unwrap().followed_authors.is_empty());
}

#[test]
fn context_author_shape_and_duplicates_retain_typed_failures() {
    for authors in [
        vec!["a".repeat(63)],
        vec!["A".repeat(64)],
        vec!["x".repeat(262_145)],
    ] {
        assert_eq!(selected(authors), Err(LocalNetworkError::InvalidAuthor));
    }
    assert_eq!(
        selected(vec!["a".repeat(64), "a".repeat(64)]),
        Err(LocalNetworkError::DuplicateAuthor)
    );
}

#[test]
fn context_revalidation_preserves_supported_relay_forms_and_wire_values() {
    for (relay, policy) in [
        ("wss://relay.example", LocalNetworkRelayPolicy::Public),
        ("ws://127.0.0.1:7447", LocalNetworkRelayPolicy::Simulator),
        ("ws://10.0.0.1:7447", LocalNetworkRelayPolicy::Device),
    ] {
        let context = LocalNetwork::new_for_relay_policy(
            "nearby".into(),
            "Nearby".into(),
            vec![relay.into()],
            None,
            vec![],
            7,
            policy,
        )
        .unwrap();
        context.validate_query_context().unwrap();
        let wire = serde_json::to_value(&context).unwrap();
        assert_eq!(wire["id"], "nearby");
        assert_eq!(wire["relayUrls"][0], relay);
        assert_eq!(wire["followedAuthors"], serde_json::json!([]));
        let decoded: LocalNetwork = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, context);
        decoded.validate_query_context().unwrap();
    }
    let public = LocalNetwork::new(
        "nearby".into(),
        "Nearby".into(),
        vec!["ws://10.0.0.1:7447".into()],
        None,
        vec![],
        7,
    );
    assert_eq!(public, Err(LocalNetworkError::InvalidRelay));
}

#[test]
fn context_debug_redacts_raw_fields_and_keeps_safe_diagnostics() {
    let mut context = selected(vec!["a".repeat(64)]).unwrap();
    context.label = "private-label-sentinel".into();
    context.locality = Some("private-location-sentinel".into());
    let debug = format!("{context:?}");
    for sentinel in [
        "nearby",
        "private-label-sentinel",
        "private-location-sentinel",
        "wss://relay.example",
        &"a".repeat(64),
    ] {
        assert!(
            !debug.contains(sentinel),
            "Sensitive context field must be redacted"
        );
    }
    assert!(debug.contains("relay_count: 1"));
    assert!(debug.contains("followed_author_count: 1"));
    assert!(debug.contains("generation: 7"));
    assert_eq!(
        serde_json::to_value(&context).unwrap()["label"],
        "private-label-sentinel"
    );
}
