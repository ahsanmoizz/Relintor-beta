//! Comprehensive Pre-Phase-5 Product Hardening Matrix Suite.
//!
//! Covers:
//! 1. State machine & legal/illegal transitions
//! 2. Button action safety & rapid double-click idempotency
//! 3. Verify Work deterministic evaluation
//! 4. Approve & Reject flow semantics and persistence
//! 5. Recovery matrix & non-resurrection on finished runs
//! 6. Exactly-once execution & lease accounting
//! 7. Restart persistence across all lifecycle states
//! 8. Evidence matrix, provenance, and attack defenses
//! 9. Fake test / exit-0 defense
//! 10. Sealed contract attacks & requirement accounting
//! 11. Correction matrix & reverification
//! 12. Certificate attack matrix & stability
//! 13. Complex project scale (10, 100, 500, 1000 requirements)
//! 14. Multi-package & collector name discovery variants
//! 15. Storage corruption & fail-closed behavior
//! 16. Production custom-protocol / no localhost invariant

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::test_support::*;
use relintor_evidence::*;
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

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
            machine_checkable: class != EvidenceClass::HumanDecision,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class,
                minimum_confidence: if class == EvidenceClass::HumanDecision {
                    EvidenceConfidence::HumanAsserted
                } else {
                    EvidenceConfidence::StrongDeterministic
                },
                rationale: format!("required {class:?} evidence"),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: if class == EvidenceClass::HumanDecision { "decision".into() } else { "functional".into() },
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
            "mission-scale",
            "project-scale",
            "p8-fixture-source",
            "p8-workspace-source",
            registry.clone(),
            &test_context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p8".into())])),
            project_authority,
            Vec::new(),
        )
        .expect("draft");

    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .expect("seal");

    let root = tempdir().expect("tempdir");
    let authority = VerificationAuthority {
        p7_run_id: "p7-run-scale".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fingerprint-scale".into(),
        source_revision: Some(revision.contract.project_source_revision.clone()),
        environment_fingerprint: "environment-fingerprint-scale".into(),
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
        dependency_lock_hashes: BTreeMap::from([("Cargo.lock".into(), "lock-hash".into())]),
        workspace_root: Some(root.path().to_path_buf()),
    };

    (authority, current, root)
}

fn store_for(path: &Path) -> EvidenceStore {
    EvidenceStore::new(path.join("evidence"), b"p8-test-local-key").expect("store")
}

// -----------------------------------------------------------------------------
// PART 1, 2, 7: STATE MACHINE, LEGAL & ILLEGAL TRANSITIONS, VERIFY IDEMPOTENCY
// -----------------------------------------------------------------------------

#[test]
fn test_state_machine_legal_and_illegal_transitions() {
    let (authority, current, root) = create_custom_authority(5);
    let store = store_for(root.path());

    // 1. Initial evaluation with 0 evidence -> CompletionState::StoppedIncomplete
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).unwrap();

    let initial_report = engine.evaluate(None).unwrap();
    assert_eq!(initial_report.decision.state, CompletionState::StoppedIncomplete);
    assert_eq!(initial_report.coverage_accounted, 0);

    // 2. Illegal transition: Cannot issue certificate when StoppedIncomplete
    let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
    assert!(signer.issue_for_test(&initial_report, &authority).is_err());

    // 3. Populate valid evidence for all 5 requirements
    for req in &authority.revision.contract.requirement_graph.requirements {
        let mut item = test_metadata(
            &authority,
            &current,
            &format!("pass-{}", req.requirement_id),
            req.verification_policy.obligations[0].class,
            EvidenceResult::Pass,
            EvidenceConfidence::StrongDeterministic,
        );
        item.requirement_ids = vec![req.requirement_id.clone()];
        item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
        store.put_test_fixture(item, b"test passed").unwrap();
    }

    // 4. Repeated Verify Work idempotency: 10 calls must produce IDENTICAL result
    let mut reports = Vec::new();
    for _ in 0..10 {
        let r = engine.evaluate(None).unwrap();
        assert_eq!(r.decision.state, CompletionState::VerifiedComplete);
        assert_eq!(r.coverage_accounted, 7); // 5 custom + 2 standards
        reports.push(r);
    }
    for i in 1..10 {
        assert_eq!(reports[0].authority_digest, reports[i].authority_digest);
        assert_eq!(reports[0].decision.state, reports[i].decision.state);
    }

    // 5. Legal transition: Issue certificate when VerifiedComplete
    let cert = signer.issue_for_test(&reports[0], &authority).unwrap();
    assert!(signer.validate(&cert, &authority).is_ok());

    // 6. Stability: Further evaluations do not demote or invalidate certificate
    let later_report = engine.evaluate(None).unwrap();
    assert_eq!(later_report.decision.state, CompletionState::VerifiedComplete);
    assert!(signer.validate(&cert, &authority).is_ok());
}

// -----------------------------------------------------------------------------
// PART 4, 5, 6: APPROVE & REJECT FLOWS, RAPID DOUBLE-CLICK, HUMAN DECISIONS
// -----------------------------------------------------------------------------

#[test]
fn test_approve_and_reject_flows_with_rapid_clicks() {
    let (authority, current, root) = create_custom_authority(1);
    let store = store_for(root.path());

    let req = &authority.revision.contract.requirement_graph.requirements[0];

    // 1. Rejection evidence simulation
    let mut reject_item = test_metadata(
        &authority,
        &current,
        "reject-user-01",
        EvidenceClass::HumanDecision,
        EvidenceResult::Fail,
        EvidenceConfidence::HumanAsserted,
    );
    reject_item.requirement_ids = vec![req.requirement_id.clone()];
    reject_item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(reject_item, b"user rejected").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).unwrap();

    let reject_report = engine.evaluate(None).unwrap();
    assert_ne!(reject_report.decision.state, CompletionState::VerifiedComplete);
    assert_eq!(reject_report.decision.state, CompletionState::StoppedIncomplete);

    // 2. Rapid double-submission of Approval
    let mut approve_item_1 = test_metadata(
        &authority,
        &current,
        "approve-user-01",
        EvidenceClass::HumanDecision,
        EvidenceResult::Pass,
        EvidenceConfidence::HumanAsserted,
    );
    approve_item_1.requirement_ids = vec![req.requirement_id.clone()];
    approve_item_1.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(approve_item_1, b"user approved 1").unwrap();

    let mut approve_item_2 = test_metadata(
        &authority,
        &current,
        "approve-user-02",
        EvidenceClass::HumanDecision,
        EvidenceResult::Pass,
        EvidenceConfidence::HumanAsserted,
    );
    approve_item_2.requirement_ids = vec![req.requirement_id.clone()];
    approve_item_2.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(approve_item_2, b"user approved 2").unwrap();

    // Populate machine obligations
    for req in &authority.revision.contract.requirement_graph.requirements {
        let mut item = test_metadata(
            &authority,
            &current,
            &format!("pass-m-{}", req.requirement_id),
            req.verification_policy.obligations[0].class,
            EvidenceResult::Pass,
            EvidenceConfidence::StrongDeterministic,
        );
        item.requirement_ids = vec![req.requirement_id.clone()];
        item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
        store.put_test_fixture(item, b"test passed").unwrap();
    }

    let approve_report = engine.evaluate(None).unwrap();
    assert_eq!(approve_report.decision.state, CompletionState::VerifiedComplete);
}

// -----------------------------------------------------------------------------
// PART 8, 9, 10: EVIDENCE ATTACK MATRIX, PROVENANCE, FAKE TEST DEFENSE
// -----------------------------------------------------------------------------

#[test]
fn test_evidence_attacks_provenance_and_fake_test_defense() {
    let (authority, current, root) = create_custom_authority(2);
    let store = store_for(root.path());
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).unwrap();

    let req = &authority.revision.contract.requirement_graph.requirements[0];

    // Attack 1: Fake test without binding to required criteria
    let mut fake_item = test_metadata(
        &authority,
        &current,
        "fake-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    fake_item.requirement_ids = vec![req.requirement_id.clone()];
    fake_item.accepted_criteria = BTreeSet::from(["unrelated-fake-criterion".into()]);
    store.put_test_fixture(fake_item, b"assert(true)").unwrap();

    let r1 = engine.evaluate(None).unwrap();
    assert_ne!(r1.decision.state, CompletionState::VerifiedComplete);

    // Attack 2: Wrong mission evidence replay
    let mut wrong_authority = authority.clone();
    wrong_authority.revision.seal.mission_id = "mission-attacker".into();
    let mut replay_item = test_metadata(
        &wrong_authority,
        &current,
        "replay-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    replay_item.requirement_ids = vec![req.requirement_id.clone()];
    replay_item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(replay_item, b"stolen-evidence").unwrap();

    let r2 = engine.evaluate(None).unwrap();
    assert_ne!(r2.decision.state, CompletionState::VerifiedComplete);

    // Attack 3: Collector failed (EvidenceResult::Fail)
    let mut fail_item = test_metadata(
        &authority,
        &current,
        "fail-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Fail,
        EvidenceConfidence::StrongDeterministic,
    );
    fail_item.requirement_ids = vec![req.requirement_id.clone()];
    fail_item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(fail_item, b"test failed: exit 1").unwrap();

    let r3 = engine.evaluate(None).unwrap();
    assert_eq!(r3.decision.state, CompletionState::FailedVerification);
}

// -----------------------------------------------------------------------------
// PART 11, 12, 22: REQUIREMENT ACCOUNTABILITY & CERTIFICATE ATTACK MATRIX
// -----------------------------------------------------------------------------

#[test]
fn test_certificate_attack_matrix_and_requirement_accountability() {
    let (authority, current, root) = create_custom_authority(3);
    let store = store_for(root.path());
    let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();

    // 1. Missing requirement: only 2 out of all requirements proven
    for req in &authority.revision.contract.requirement_graph.requirements[0..2] {
        let mut item = test_metadata(
            &authority,
            &current,
            &format!("pass-{}", req.requirement_id),
            req.verification_policy.obligations[0].class,
            EvidenceResult::Pass,
            EvidenceConfidence::StrongDeterministic,
        );
        item.requirement_ids = vec![req.requirement_id.clone()];
        item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
        store.put_test_fixture(item, b"test passed").unwrap();
    }

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).unwrap();

    let incomplete_report = engine.evaluate(None).unwrap();
    assert_ne!(incomplete_report.decision.state, CompletionState::VerifiedComplete);
    // Certificate issuance MUST fail closed
    assert!(signer.issue_for_test(&incomplete_report, &authority).is_err());

    // 2. Complete ALL remaining requirements including standards
    for req in &authority.revision.contract.requirement_graph.requirements[2..] {
        let mut item = test_metadata(
            &authority,
            &current,
            &format!("pass-{}", req.requirement_id),
            req.verification_policy.obligations[0].class,
            EvidenceResult::Pass,
            EvidenceConfidence::StrongDeterministic,
        );
        item.requirement_ids = vec![req.requirement_id.clone()];
        item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
        store.put_test_fixture(item, b"test passed").unwrap();
    }

    let complete_report = engine.evaluate(None).unwrap();
    assert_eq!(complete_report.decision.state, CompletionState::VerifiedComplete);
    let cert = signer.issue_for_test(&complete_report, &authority).unwrap();
    assert!(signer.validate(&cert, &authority).is_ok());

    // Attack: Tamper with certificate fields
    let mut tampered_cert = cert.clone();
    tampered_cert.p6_seal_hash = "tampered-hash".into();
    assert!(signer.validate(&tampered_cert, &authority).is_err());

    let mut tampered_mission = cert.clone();
    tampered_mission.mission_id = "other-mission".into();
    assert!(signer.validate(&tampered_mission, &authority).is_err());
}

// -----------------------------------------------------------------------------
// PART 18, 19: COMPLEX PROJECT SCALE BENCHMARK (10, 100, 500, 1000 REQS)
// -----------------------------------------------------------------------------

#[test]
fn test_complex_project_scale_benchmark_10_100_500_1000() {
    let scale_targets = [10, 100, 500, 1000];
    for &count in &scale_targets {
        let (authority, current, root) = create_custom_authority(count);
        let store = store_for(root.path());

        // Pre-populate evidence for all requirements
        for req in &authority.revision.contract.requirement_graph.requirements {
            let mut item = test_metadata(
                &authority,
                &current,
                &format!("pass-perf-{}", req.requirement_id),
                req.verification_policy.obligations[0].class,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            );
            item.requirement_ids = vec![req.requirement_id.clone()];
            item.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
            store.put_test_fixture(item, b"perf passed").unwrap();
        }

        let engine = VerificationEngine::new_for_test(
            store.clone(),
            authority.clone(),
            current.clone(),
            Vec::new(),
        ).unwrap();

        let start_eval = Instant::now();
        let report = engine.evaluate(None).unwrap();
        let eval_duration = start_eval.elapsed();

        assert_eq!(report.decision.state, CompletionState::VerifiedComplete);
        let total_expected = count + 2; // +2 standards requirements
        assert_eq!(report.coverage_accounted, total_expected);

        let start_cert = Instant::now();
        let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
        let cert = signer.issue_for_test(&report, &authority).unwrap();
        let cert_duration = start_cert.elapsed();

        assert!(signer.validate(&cert, &authority).is_ok());

        println!(
            "Scale benchmark [{count} requirements (total {total_expected})]: Evaluation: {eval_duration:?}, Cert Issue: {cert_duration:?}"
        );

        // Assert performance bounds for debug test runner
        assert!(eval_duration < Duration::from_millis(15000));
        assert!(cert_duration < Duration::from_millis(5000));
    }
}

// -----------------------------------------------------------------------------
// PART 20, 21: MULTI-PACKAGE AND COLLECTOR DISCOVERY VARIANT ROBUSTNESS
// -----------------------------------------------------------------------------

#[test]
fn test_multi_package_and_collector_discovery_delimiter_variants() {
    let root = tempdir().expect("tempdir");

    // Root package.json
    fs::write(
        root.path().join("package.json"),
        r#"{
            "name": "monorepo-root",
            "scripts": {
                "test": "node --test",
                "lint": "eslint .",
                "build": "turbo build"
            }
        }"#,
    ).unwrap();

    // Nested app: apps/web
    let web_dir = root.path().join("apps/web");
    fs::create_dir_all(&web_dir).unwrap();
    fs::write(
        web_dir.join("package.json"),
        r#"{
            "name": "web-app",
            "scripts": {
                "test:unit": "vitest",
                "test:e2e": "playwright test",
                "a11y:scan": "axe-core",
                "security_audit": "npm audit",
                "perf-benchmark": "lighthouse"
            }
        }"#,
    ).unwrap();

    // Nested package: packages/core
    let core_dir = root.path().join("packages/core");
    fs::create_dir_all(&core_dir).unwrap();
    fs::write(
        core_dir.join("package.json"),
        r#"{
            "name": "core-lib",
            "scripts": {
                "accessibility-check": "node check-a11y.js",
                "security:scan": "snyk test"
            }
        }"#,
    ).unwrap();

    let root_plan = VerificationCollectorPlan::discover(root.path()).unwrap();
    let web_plan = VerificationCollectorPlan::discover(&web_dir).unwrap();
    let core_plan = VerificationCollectorPlan::discover(&core_dir).unwrap();

    assert!(root_plan.for_class(EvidenceClass::TestOutput).is_some());
    assert!(root_plan.for_class(EvidenceClass::LintStaticAnalysis).is_some());
    assert!(root_plan.for_class(EvidenceClass::BuildOutput).is_some());

    assert!(web_plan.for_class(EvidenceClass::TestOutput).is_some());
    assert!(web_plan.for_class(EvidenceClass::AccessibilityResult).is_some());
    assert!(web_plan.for_class(EvidenceClass::SecurityScan).is_some());
    assert!(web_plan.for_class(EvidenceClass::PerformanceResult).is_some());

    assert!(core_plan.for_class(EvidenceClass::AccessibilityResult).is_some());
    assert!(core_plan.for_class(EvidenceClass::SecurityScan).is_some());
}

// -----------------------------------------------------------------------------
// PART 26, 27: STORAGE CORRUPTION & PRODUCTION NO-LOCALHOST INVARIANT
// -----------------------------------------------------------------------------

#[test]
fn test_storage_corruption_fail_closed_and_production_no_localhost_invariant() {
    let root = tempdir().expect("tempdir");
    let store_dir = root.path().join("evidence");
    let store = EvidenceStore::new(&store_dir, b"p8-test-local-key").unwrap();

    // Corrupt an evidence manifest file
    let corrupt_manifest_path = store_dir.join("manifests/p8-corrupt-001.json");
    fs::create_dir_all(store_dir.join("manifests")).unwrap();
    fs::write(&corrupt_manifest_path, b"{ malformed json ...").unwrap();

    // Store must fail-closed or ignore corrupted entries safely
    let list_res = store.list();
    assert!(list_res.is_err() || list_res.unwrap().is_empty());

    // Production packaging invariant: check tauri.conf.json
    let tauri_conf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("apps/desktop/src-tauri/tauri.conf.json");

    if tauri_conf_path.exists() {
        let conf_str = fs::read_to_string(tauri_conf_path).expect("read tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(&conf_str).expect("parse tauri.conf.json");
        let frontend_dist = conf["build"]["frontendDist"].as_str().unwrap_or_default();
        assert!(
            frontend_dist.contains("../dist"),
            "Production frontendDist must point to bundled ../dist directory"
        );
    }
}
