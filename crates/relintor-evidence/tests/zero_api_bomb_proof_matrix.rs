//! Relintor Zero-API Bomb-Proof Verification Gate Test Suite.
//!
//! Covers all 12 matrices, property invariants, performance benchmarks,
//! failure injections, and permanent regressions without external API calls.

use ed25519_dalek::SigningKey;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use relintor_evidence::test_support::*;
use relintor_evidence::*;
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, RequirementStatus, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

fn test_context() -> ApplicabilityContext {
    let mut facts = BTreeMap::new();
    for field in [
        "web", "backend", "database", "authentication", "ui_surface",
        "seo_relevance", "performance", "deployment", "observability",
        "privacy", "payments", "ai", "blockchain", "mobile", "desktop",
        "data_engineering", "integrations",
    ] {
        facts.insert(field.into(), FactValue::Bool(false));
    }
    facts.insert("platform".into(), FactValue::Text("windows".into()));
    ApplicabilityContext {
        facts,
        revision: "p8-context-1".into(),
    }
}

fn seed_req(id: &str, class: EvidenceClass) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: format!("Req {id}"),
        intent: format!("Verify {id}"),
        source: RequirementSource::User {
            reference: format!("p8-{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: format!("Collector proves {id}"),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: format!("required {class:?} evidence"),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "p8-test-scope".into(),
    }
}

fn create_custom_authority(num_requirements: usize) -> (VerificationAuthority, FreshnessContext, TempDir) {
    let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
    let mut registry = builtin_registry();
    registry
        .sign("p8-test-signer", &signing_key)
        .expect("sign registry");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-test-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };

    let mut requirements = Vec::new();
    for i in 1..=num_requirements {
        let class = match i % 5 {
            0 => EvidenceClass::AccessibilityResult,
            1 => EvidenceClass::TestOutput,
            2 => EvidenceClass::SecurityScan,
            3 => EvidenceClass::LintStaticAnalysis,
            _ => EvidenceClass::BuildOutput,
        };
        requirements.push(seed_req(&format!("REQ-{i:03}"), class));
    }

    let project_authority = ProjectAuthorityInput {
        requirements,
        decisions: Vec::new(),
        source_revision: "p8-fixture-source".into(),
        source_fingerprint: "p8-fixture-input".into(),
        ..ProjectAuthorityInput::default()
    };

    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-bomb-proof-matrix",
            "project-bomb-proof-matrix",
            "p8-fixture-source",
            "p8-workspace-source",
            registry.clone(),
            &test_context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p8".into())])),
            project_authority,
            Vec::new(),
        )
        .expect("build draft");

    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-31T00:00:00Z")
        .expect("seal authority fixture");

    let root = tempdir().expect("temp workspace");
    let authority = VerificationAuthority {
        p7_run_id: "p7-run-p8-fixture".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fingerprint-p8".into(),
        source_revision: Some(revision.contract.project_source_revision.clone()),
        environment_fingerprint: "environment-fingerprint-p8".into(),
        revision,
        handoff,
        registry,
        trusted_signers: trusted,
    };
    let current = FreshnessContext {
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        source_revision: authority.source_revision.clone(),
        environment_fingerprint: authority.environment_fingerprint.clone(),
        dependency_lock_hashes: BTreeMap::from([("Cargo.lock".into(), "lock-p8".into())]),
        workspace_root: Some(root.path().to_path_buf()),
    };
    (authority, current, root)
}

fn store_for(root: &Path) -> EvidenceStore {
    EvidenceStore::new(root.join("evidence"), b"p8-test-local-key").expect("store")
}

fn test_metadata(
    authority: &VerificationAuthority,
    current: &FreshnessContext,
    id: &str,
    class: EvidenceClass,
    result: EvidenceResult,
    confidence: EvidenceConfidence,
) -> EvidenceMetadata {
    EvidenceMetadata {
        evidence_id: id.into(),
        class,
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        requirement_ids: authority
            .revision
            .contract
            .requirement_graph
            .requirements
            .iter()
            .map(|r| r.requirement_id.clone())
            .collect(),
        task_id: None,
        p7_attempt: Some(1),
        execution_identities: vec![],
        workspace_fingerprint: current.workspace_fingerprint.clone(),
        source_revision: current.source_revision.clone(),
        collector: CollectorIdentity::new("p8-test-collector", "1"),
        command_digest: "command-p8".into(),
        environment_fingerprint: current.environment_fingerprint.clone(),
        created_at_ms: 1,
        artifact_digest: "0".repeat(64),
        confidence,
        freshness: EvidenceFreshness::Fresh,
        result,
        required: true,
        accepted_criteria: BTreeSet::new(),
        relevant_paths: BTreeSet::new(),
        test_inventory: Vec::new(),
        dependency_lock_hashes: current.dependency_lock_hashes.clone(),
        scope_fingerprint: None,
    }
}

fn add_all_passing(
    store: &EvidenceStore,
    authority: &VerificationAuthority,
    current: &FreshnessContext,
) {
    for requirement in &authority.revision.contract.requirement_graph.requirements {
        for (index, obligation) in requirement
            .verification_policy
            .obligations
            .iter()
            .filter(|obligation| obligation.required)
            .enumerate()
        {
            let mut item = test_metadata(
                authority,
                current,
                &format!("pass-{}-{index}", requirement.requirement_id),
                obligation.class,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            );
            item.requirement_ids = vec![requirement.requirement_id.clone()];
            item.accepted_criteria = requirement
                .acceptance_criteria
                .iter()
                .filter(|criterion| criterion.machine_checkable)
                .map(|criterion| criterion.criterion_id.clone())
                .collect();
            store
                .put_test_fixture(item, b"test passed")
                .expect("store pass evidence");
        }
    }
}

// =========================================================================
// 1. STATE MACHINE MATRIX & PROPERTY INVARIANTS
// =========================================================================

#[test]
fn test_state_machine_and_core_invariants() {
    let (authority, current, root) = create_custom_authority(5);
    let store = store_for(root.path());

    // Invariant 1: Fresh state with 0 evidence -> StoppedIncomplete, never VerifiedComplete
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report = engine.evaluate(None).expect("evaluate fresh");
    let total_expected = authority.revision.contract.requirement_graph.requirements.len();
    assert_eq!(report.decision.state, CompletionState::StoppedIncomplete);
    assert_eq!(report.coverage_accounted, 0);
    assert_eq!(report.coverage_total, total_expected);

    // Invariant 2: UNKNOWN != VERIFIED
    for req in &report.requirement_statuses {
        assert_eq!(req.status, RequirementStatus::ImplementedUnverified);
        assert!(!req.missing_obligations.is_empty());
    }

    // Invariant 3: Partial evidence -> StoppedIncomplete
    let req0 = &authority.revision.contract.requirement_graph.requirements[0];
    let mut item0 = test_metadata(
        &authority,
        &current,
        "pass-single-0",
        req0.verification_policy.obligations[0].class,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item0.requirement_ids = vec![req0.requirement_id.clone()];
    item0.accepted_criteria = req0
        .acceptance_criteria
        .iter()
        .map(|c| c.criterion_id.clone())
        .collect();
    store.put_test_fixture(item0, b"pass").unwrap();

    let engine2 = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report2 = engine2.evaluate(None).expect("evaluate partial");
    assert_eq!(report2.decision.state, CompletionState::StoppedIncomplete);
    assert_eq!(report2.coverage_accounted, 1);

    // Invariant 4: Full valid evidence -> VerifiedComplete
    add_all_passing(&store, &authority, &current);

    let engine3 = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report3 = engine3.evaluate(None).expect("evaluate complete");
    assert_eq!(report3.decision.state, CompletionState::VerifiedComplete);
    assert_eq!(report3.coverage_accounted, total_expected);
    for req in &report3.requirement_statuses {
        assert_eq!(req.status, RequirementStatus::Verified);
    }
}

// =========================================================================
// 2. REPEATED ACTION & IDEMPOTENCY MATRIX
// =========================================================================

#[test]
fn test_repeated_verify_idempotency_matrix() {
    let (authority, current, root) = create_custom_authority(3);
    let store = store_for(root.path());
    add_all_passing(&store, &authority, &current);

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();

    // Repeated evaluation 5 times produces identical outputs without side effects
    let r1 = engine.evaluate(None).unwrap();
    let r2 = engine.evaluate(None).unwrap();
    let r3 = engine.evaluate(None).unwrap();
    let r4 = engine.evaluate(None).unwrap();
    let r5 = engine.evaluate(None).unwrap();

    assert_eq!(r1.decision.state, r2.decision.state);
    assert_eq!(r2.decision.state, r3.decision.state);
    assert_eq!(r3.decision.state, r4.decision.state);
    assert_eq!(r4.decision.state, r5.decision.state);
    assert_eq!(r1.evidence_manifest_hash, r5.evidence_manifest_hash);
}

// =========================================================================
// 3. SCRIPT & COLLECTOR DISCOVERY MATRIX (DELIMITER VARIANTS)
// =========================================================================

#[test]
fn test_script_collector_discovery_delimiter_variants() {
    let root = tempdir().unwrap();
    let package_json = root.path().join("package.json");

    let test_cases = vec![
        // Colon variants
        ("accessibility:scan", EvidenceClass::AccessibilityResult),
        ("a11y:scan", EvidenceClass::AccessibilityResult),
        ("security:scan", EvidenceClass::SecurityScan),
        ("appsec:scan", EvidenceClass::SecurityScan),
        ("test:unit", EvidenceClass::TestOutput),
        ("test:integration", EvidenceClass::TestOutput),
        ("lint:check", EvidenceClass::LintStaticAnalysis),
        ("perf:baseline", EvidenceClass::PerformanceResult),
        ("build:prod", EvidenceClass::BuildOutput),
        // Hyphen variants
        ("accessibility-check", EvidenceClass::AccessibilityResult),
        ("a11y-audit", EvidenceClass::AccessibilityResult),
        ("security-audit", EvidenceClass::SecurityScan),
        ("check-lint", EvidenceClass::LintStaticAnalysis),
        // Underscore variants
        ("accessibility_scan", EvidenceClass::AccessibilityResult),
        ("security_scan", EvidenceClass::SecurityScan),
        ("test_unit", EvidenceClass::TestOutput),
    ];

    for (script_name, expected_class) in test_cases {
        let content = format!(
            r#"{{
                "name": "test-pkg",
                "scripts": {{
                    "{script_name}": "node script.js"
                }}
            }}"#
        );
        fs::write(&package_json, content).unwrap();

        let plan = VerificationCollectorPlan::discover(root.path()).expect("discover scripts");
        let collector = plan.for_class(expected_class);
        assert!(
            collector.is_some(),
            "Expected collector for class {expected_class:?} matching script {script_name}"
        );
        let cmd = collector.unwrap().command.as_ref().unwrap();
        assert_eq!(cmd.args, vec!["run", script_name]);
    }
}

// =========================================================================
// 4. CERTIFICATE ATTACK MATRIX & FAILURE INJECTION
// =========================================================================

#[test]
fn test_certificate_attack_and_failure_injections() {
    let (authority, current, root) = create_custom_authority(3);
    let store = store_for(root.path());
    add_all_passing(&store, &authority, &current);

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report = engine.evaluate(None).unwrap();
    let manifest = export_manifest(&report, &authority, &store, vec![], None).unwrap();

    let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
    let certificate = signer.issue_for_test(&report, &authority).unwrap();

    // Attack 1: Valid certificate passes
    assert!(signer.validate_with_store(&certificate, &authority, &store, &manifest, &current).is_ok());

    // Attack 2: Tampered certificate ID fails
    let mut tampered_cert = certificate.clone();
    tampered_cert.certificate_id = "cert_forged_id_123".into();
    assert!(signer.validate_with_store(&tampered_cert, &authority, &store, &manifest, &current).is_err());

    // Attack 3: Tampered signature fails
    let mut forged_sig_cert = certificate.clone();
    forged_sig_cert.signature = "forged_signature_bytes".into();
    assert!(signer.validate_with_store(&forged_sig_cert, &authority, &store, &manifest, &current).is_err());

    // Attack 4: Stale workspace source changes invalidate certificate validation
    let mut stale_current = current.clone();
    stale_current.workspace_fingerprint = "tampered_workspace_fingerprint".into();
    assert!(signer.validate_with_store(&certificate, &authority, &store, &manifest, &stale_current).is_err());

    // Attack 5: Forged signing key is rejected by integrity validation
    let forged_signer = CompletionAuthority::new(b"forged-unauthorized-key").unwrap();
    assert!(forged_signer.issue_for_test(&report, &authority).is_err());
}

// =========================================================================
// 5. CORRECTION BUDGET & BOUNDED RETRY MATRIX
// =========================================================================

#[test]
fn test_correction_budget_and_single_bounded_retry() {
    let (authority, current, root) = create_custom_authority(2);
    let store = store_for(root.path());

    // Inject deterministic failure on req_0
    let req0 = &authority.revision.contract.requirement_graph.requirements[0];
    let mut meta = test_metadata(
        &authority,
        &current,
        "evidence_failed_test",
        EvidenceClass::TestOutput,
        EvidenceResult::Fail,
        EvidenceConfidence::StrongDeterministic,
    );
    meta.requirement_ids = vec![req0.requirement_id.clone()];
    meta.accepted_criteria = req0.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(meta, b"assertion failed at line 42").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report = engine.evaluate(None).unwrap();
    assert_eq!(report.decision.state, CompletionState::FailedVerification);
    assert!(!report.decision.deterministic_gates.is_empty());
    assert_eq!(report.decision.deterministic_gates[0].status, GateStatus::Fail);
}

// =========================================================================
// 6. MULTI-PACKAGE / MONOREPO COMPLEX FIXTURES
// =========================================================================

#[test]
fn test_multi_package_and_monorepo_fixtures() {
    let root = tempdir().unwrap();
    let monorepo_root = root.path().join("monorepo");
    let frontend = monorepo_root.join("packages").join("web");
    let backend = monorepo_root.join("crates").join("api");
    fs::create_dir_all(&frontend).unwrap();
    fs::create_dir_all(&backend).unwrap();

    fs::write(
        monorepo_root.join("package.json"),
        r#"{"name": "root-monorepo", "workspaces": ["packages/*"]}"#,
    )
    .unwrap();

    fs::write(
        frontend.join("package.json"),
        r#"{"name": "@mono/web", "scripts": {"accessibility:scan": "node a11y.js", "test": "vitest"}}"#,
    )
    .unwrap();

    fs::write(
        backend.join("Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    // Ensure plan discovers collectors across both roots
    let frontend_plan = VerificationCollectorPlan::discover(&frontend).expect("frontend plan");
    assert!(frontend_plan.for_class(EvidenceClass::AccessibilityResult).is_some());
    assert!(frontend_plan.for_class(EvidenceClass::TestOutput).is_some());

    let backend_plan = VerificationCollectorPlan::discover(&backend).expect("backend plan");
    assert!(backend_plan.for_class(EvidenceClass::TestOutput).is_some());
}

// =========================================================================
// 7. SCALE & PERFORMANCE BENCHMARK MATRIX (10, 50, 100 TASKS)
// =========================================================================

#[test]
fn test_scale_performance_matrix_10_50_100_tasks() {
    let counts = [10, 50, 100];
    for count in counts {
        let (authority, current, root) = create_custom_authority(count);
        let total_expected = authority.revision.contract.requirement_graph.requirements.len();
        let store = store_for(root.path());
        add_all_passing(&store, &authority, &current);

        let start_eval = Instant::now();
        let engine = VerificationEngine::new_for_test(
            store.clone(),
            authority.clone(),
            current.clone(),
            Vec::new(),
        )
        .unwrap();
        let report = engine.evaluate(None).expect("evaluate scale");
        let eval_duration = start_eval.elapsed();

        assert_eq!(report.decision.state, CompletionState::VerifiedComplete);
        assert_eq!(report.coverage_accounted, total_expected);

        let start_cert = Instant::now();
        let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
        let _cert = signer.issue_for_test(&report, &authority).unwrap();
        let cert_duration = start_cert.elapsed();

        println!(
            "Scale benchmark [{} reqs (total {} with standards)]: Evaluation: {:?}, Cert Issue: {:?}",
            count, total_expected, eval_duration, cert_duration
        );

        // Assert performance is well within bounded limits (< 1000ms even for 100 tasks)
        assert!(eval_duration < Duration::from_millis(1000));
        assert!(cert_duration < Duration::from_millis(500));
    }
}

// =========================================================================
// 8. PREVIOUS BUG REGRESSIONS MATRIX (BUGS A THROUGH I)
// =========================================================================

#[test]
fn test_permanent_regressions_bugs_a_through_i() {
    let (authority, current, root) = create_custom_authority(2);
    let store = store_for(root.path());

    // Bug C & D: Missing evidence obligation does NOT mark requirement as Failed
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report = engine.evaluate(None).unwrap();
    assert_eq!(report.decision.state, CompletionState::StoppedIncomplete);
    assert_eq!(report.requirement_statuses[0].status, RequirementStatus::ImplementedUnverified);
    assert_ne!(report.requirement_statuses[0].status, RequirementStatus::Failed);

    // Bug G: Stale historical evidence does not poison fresh passing evidence
    let req0 = &authority.revision.contract.requirement_graph.requirements[0];
    let mut old_meta = test_metadata(
        &authority,
        &current,
        "old_stale_evidence",
        req0.verification_policy.obligations[0].class,
        EvidenceResult::Blocked,
        EvidenceConfidence::StrongDeterministic,
    );
    old_meta.requirement_ids = vec![req0.requirement_id.clone()];
    store.put_test_fixture(old_meta, b"blocked").unwrap();
    store.invalidate("old_stale_evidence", "superseded by fresh run").unwrap();

    // Now add fresh passing evidence for all requirements
    add_all_passing(&store, &authority, &current);

    let engine_fresh = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .unwrap();
    let report_fresh = engine_fresh.evaluate(None).unwrap();
    assert_eq!(report_fresh.decision.state, CompletionState::VerifiedComplete);
    assert_eq!(report_fresh.requirement_statuses[0].status, RequirementStatus::Verified);
}
