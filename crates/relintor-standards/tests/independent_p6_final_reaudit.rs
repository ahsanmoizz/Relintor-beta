use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_standards::*;
use std::collections::BTreeMap;

fn signed_registry() -> (StandardsRegistry, TrustedSignerSet) {
    let key = SigningKey::from_bytes(&[31_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p6-final-reaudit", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p6-final-reaudit".into(),
            algorithm: "Ed25519".into(),
            public_key: BASE64.encode(key.verifying_key().to_bytes()),
        }],
    };
    (registry, trusted)
}

fn complete_context() -> ApplicabilityContext {
    let mut facts = BTreeMap::new();
    for field in [
        "web",
        "backend",
        "database",
        "authentication",
        "ui_surface",
        "seo_relevance",
        "performance",
        "deployment",
        "observability",
        "privacy",
        "payments",
        "ai",
        "blockchain",
        "mobile",
        "desktop",
        "data_engineering",
        "integrations",
    ] {
        facts.insert(field.to_string(), FactValue::Bool(false));
    }
    facts.insert("backend".into(), FactValue::Bool(true));
    facts.insert("platform".into(), FactValue::Text("fixture".into()));
    ApplicabilityContext {
        facts,
        revision: "p6-final-complete-facts".into(),
    }
}

#[test]
fn mixed_known_and_unknown_authority_facts_block_preseal() {
    let (registry, trusted) = signed_registry();
    let context = ApplicabilityContext {
        facts: BTreeMap::from([
            ("desktop".into(), FactValue::Bool(true)),
            ("platform".into(), FactValue::Text("desktop".into())),
        ]),
        revision: "mixed-known-unknown".into(),
    };

    let engine = AuthorityEngine;
    let draft = engine
        .build_draft(
            "mixed-unknown-mission",
            "project",
            "source",
            "fingerprint",
            registry,
            &context,
            scope_fingerprint(BTreeMap::from([("root".into(), "fixture".into())])),
            Vec::new(),
        )
        .unwrap();

    assert!(
        matches!(
            engine.preseal(&draft, &trusted),
            Err(AuthorityError::UnknownApplicability(_))
        ),
        "a mission with some applicable rules and other unresolved authority facts must not seal"
    );
}

#[test]
fn explicit_false_seo_relevance_is_not_applicable() {
    let (registry, _) = signed_registry();
    let mut context = complete_context();
    context
        .facts
        .insert("seo_relevance".into(), FactValue::Bool(false));

    let (ledger, graph) = AuthorityEngine.evaluate(&registry, &context).unwrap();
    let seo_results = ledger
        .iter()
        .filter(|result| result.rule_id.contains("SEO_DISCOVERABILITY"))
        .collect::<Vec<_>>();

    assert!(!seo_results.is_empty());
    assert!(
        seo_results
            .iter()
            .all(|result| result.outcome == ApplicabilityOutcome::NotApplicable),
        "explicit SEO false must be N/A"
    );
    assert!(
        !graph
            .requirements
            .iter()
            .any(|requirement| requirement.requirement_type == "seo-discoverability"),
        "explicit SEO false must not create SEO implementation requirements"
    );
}

#[test]
fn immutable_p6_seal_state_tampering_invalidates_authority() {
    let (registry, trusted) = signed_registry();
    let engine = AuthorityEngine;
    let draft = engine
        .build_draft(
            "state-tamper-mission",
            "project",
            "source",
            "fingerprint",
            registry,
            &complete_context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "fixture".into())])),
            Vec::new(),
        )
        .unwrap();

    let (mut sealed, _) = engine.seal(&draft, &trusted, "fixture-time").unwrap();
    sealed.seal.state = "EXECUTING".into();

    assert!(
        !engine.validate_seal(&sealed).unwrap().valid,
        "P7-style state must not be forgeable by mutating the immutable P6 seal"
    );
}
