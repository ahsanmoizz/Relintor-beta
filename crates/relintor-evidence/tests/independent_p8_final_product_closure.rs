use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::{
    sha256, AcceptedRiskRecord, AiVerifierJudgement, CollectorBinding, CompletionAuthority,
    CompletionDecision, CompletionState, DeterministicGateResult, FreshnessContext,
    VerificationAuthority, VerificationReport,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};

fn context() -> ApplicabilityContext {
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
        facts.insert(field.into(), FactValue::Bool(false));
    }
    facts.insert("platform".into(), FactValue::Text("windows".into()));
    ApplicabilityContext {
        facts,
        revision: "p8-final-product-closure".into(),
    }
}

fn seed(id: &str) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: id.into(),
        intent: format!("Verify {id}"),
        source: RequirementSource::User {
            reference: format!("audit://{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "Required real verification".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "P8 final product closure".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    }
}

fn authority() -> VerificationAuthority {
    let key = SigningKey::from_bytes(&[97_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p8-final-product-closure", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-final-product-closure".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        }],
    };
    let input = ProjectAuthorityInput {
        requirements: vec![seed("P8-CLOSURE")],
        source_revision: "p8-closure-source".into(),
        source_fingerprint: "p8-closure-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p8-final-product-closure",
            "project-p8-final-product-closure",
            "p8-closure-source",
            "p8-closure-workspace",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "closure".into())])),
            input,
            Vec::new(),
        )
        .unwrap();
    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .unwrap();
    VerificationAuthority {
        revision,
        handoff,
        registry,
        trusted_signers: trusted,
        p7_run_id: "p7-real-required".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fingerprint".into(),
        source_revision: Some("p8-closure-source".into()),
        environment_fingerprint: "environment-fingerprint".into(),
    }
}

#[test]
fn collector_binding_rejects_unknown_requirement_and_criterion() {
    let authority = authority();
    let current = FreshnessContext {
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        source_revision: authority.source_revision.clone(),
        environment_fingerprint: authority.environment_fingerprint.clone(),
        dependency_lock_hashes: BTreeMap::new(),
        workspace_root: None,
    };

    let result = CollectorBinding::from_authority(
        &authority,
        &current,
        "forged-binding",
        vec!["NOT-A-SEALED-REQUIREMENT".into()],
        true,
        BTreeSet::from(["NOT-A-SEALED-CRITERION".into()]),
        BTreeSet::new(),
    );

    assert!(
        result.is_err(),
        "collector binding accepted caller-selected unknown requirement/criterion authority"
    );
}

#[test]
fn recomputed_public_report_checksum_cannot_issue_completion_certificate() {
    let authority = authority();
    let mut report = VerificationReport {
        verification_run_id: "caller-forged-run".into(),
        authority_digest: authority.identity_digest().unwrap(),
        requirement_statuses: Vec::new(),
        decision: CompletionDecision {
            state: CompletionState::VerifiedComplete,
            reason: "caller changed public fields".into(),
            deterministic_gates: Vec::<DeterministicGateResult>::new(),
            accepted_risks: Vec::<AcceptedRiskRecord>::new(),
            blocked_external: Vec::new(),
        },
        builder_claim: Some("DONE".into()),
        coverage_total: 0,
        coverage_accounted: 0,
        evidence_manifest_hash: "0".repeat(64),
        p7_ledger_digest: None,
        ai_judgements: Vec::<AiVerifierJudgement>::new(),
        integrity_tag: String::new(),
    };

    // Reproduce the current public/checksum algorithm. A checksum is not an
    // authority proof because the caller can recompute it after changing fields.
    let body = serde_json::to_vec(&(
        report.verification_run_id.clone(),
        report.authority_digest.clone(),
        report.requirement_statuses.clone(),
        report.decision.clone(),
        report.builder_claim.clone(),
        report.coverage_total,
        report.coverage_accounted,
        report.evidence_manifest_hash.clone(),
        report.p7_ledger_digest.clone(),
        report.ai_judgements.clone(),
    ))
    .unwrap();
    report.integrity_tag = sha256(&body);

    let completion = CompletionAuthority::new(b"p8-final-closure-key").unwrap();
    assert!(
        completion.issue_for_test(&report, &authority).is_err(),
        "caller-recomputed unkeyed report checksum was accepted for certificate issuance"
    );
}
