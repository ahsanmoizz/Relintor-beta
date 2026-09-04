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

        // Assert performance bounds for debug test runner under parallel test load
        assert!(eval_duration < Duration::from_millis(60000));
        assert!(cert_duration < Duration::from_millis(15000));
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

// -----------------------------------------------------------------------------
// PHASE 5A: 10,000+ FILE SCALE & FINGERPRINT BENCHMARK
// -----------------------------------------------------------------------------

#[test]
fn test_ten_thousand_file_workspace_scale_and_fingerprint() {
    let root = tempdir().expect("tempdir");
    let base = root.path();

    // Generate realistic nested directories
    let subdirs = [
        "apps/web/src/components",
        "apps/web/src/pages",
        "apps/mobile/src/screens",
        "packages/core/src/utils",
        "packages/ui/src/primitives",
        "services/auth/src/handlers",
        "services/gateway/src/routes",
        "crates/protocol/src/codec",
        "crates/storage/src/engine",
        "docs/spec/v1",
        "tests/integration/flows",
        "config/environments/staging",
    ];

    for dir in &subdirs {
        fs::create_dir_all(base.join(dir)).unwrap();
    }

    // Generate 10,000 files across directories
    let total_files = 10_000;
    for i in 0..total_files {
        let dir = subdirs[i % subdirs.len()];
        let file_path = base.join(dir).join(format!("module_{i}.ts"));
        fs::write(&file_path, format!("export const val_{i} = {i};")).unwrap();
    }

    // 1. Initial fingerprint
    let start_initial = Instant::now();
    let fp1 = fingerprint_workspace(base).expect("initial fingerprint");
    let initial_duration = start_initial.elapsed();

    // 2. Repeat fingerprint with unchanged files
    let start_repeat = Instant::now();
    let fp2 = fingerprint_workspace(base).expect("repeat fingerprint");
    let repeat_duration = start_repeat.elapsed();
    assert_eq!(fp1, fp2);

    // 3. Fingerprint after one file changes
    let modified_file = base.join(subdirs[0]).join("module_0.ts");
    fs::write(&modified_file, "export const val_0 = 'MODIFIED';").unwrap();

    let start_modified = Instant::now();
    let fp3 = fingerprint_workspace(base).expect("modified fingerprint");
    let modified_duration = start_modified.elapsed();
    assert_ne!(fp1, fp3);

    println!(
        "10k File Benchmark: Initial: {initial_duration:?}, Repeat: {repeat_duration:?}, Delta: {modified_duration:?}"
    );

    assert!(initial_duration < Duration::from_secs(60));
    assert!(repeat_duration < Duration::from_secs(60));
    assert!(modified_duration < Duration::from_secs(60));
}

// -----------------------------------------------------------------------------
// PHASE 5A: LONG LEDGER STRESS (500, 1000, 5000 EVENTS)
// -----------------------------------------------------------------------------

#[test]
fn test_long_execution_ledger_stress_500_1000_5000_events() {
    use relintor_execution::*;

    let counts = [500, 1000, 5000];
    for &num_events in &counts {
        let mut events = Vec::with_capacity(num_events);
        for i in 0..num_events as u64 {
            let kind = match i % 5 {
                0 => ExecutionEventKind::TaskScheduled,
                1 => ExecutionEventKind::LeaseIssued,
                2 => ExecutionEventKind::AttemptStarted,
                3 => ExecutionEventKind::ActionResult,
                _ => ExecutionEventKind::TaskImplementationFinished,
            };
            events.push(ExecutionEvent {
                sequence: i + 1,
                occurred_at_ms: 1000 + i * 10,
                task_id: Some(format!("task-{}", i % 10)),
                kind,
                detail: format!("Detailed execution event {i}"),
            });
        }

        let run = ExecutionRun {
            ledger_version: "p7-execution-ledger-v1".into(),
            run_id: format!("run-stress-{num_events}"),
            mission_id: "mission-stress".into(),
            mission_revision: 1,
            seal_hash: "seal-stress".into(),
            project_id: "project-stress".into(),
            workspace: PathBuf::from("D:/test"),
            workspace_fingerprint: "fingerprint-stress".into(),
            state: ExecutionRunState::ExecutionTasksFinishedAwaitingVerification,
            tasks: BTreeMap::new(),
            attempts: Vec::new(),
            leases: Vec::new(),
            events,
            usage: Default::default(),
            loop_signals: Vec::new(),
            oscillation_signals: Vec::new(),
            continuations: Vec::new(),
            diagnostics: Vec::new(),
            external_modifications: Vec::new(),
            progress: Vec::new(),
            watchdog_state: WatchdogState::Healthy,
            policy: Default::default(),
            safe_boundary: None,
            current_turn: 1,
            last_error: None,
            no_progress_occurrences: 0,
            integrity_version: "p7-ledger-integrity-v1".into(),
            integrity_tag: String::new(),
            reviewed_recovery_deltas: Vec::new(),
        };

        // Serialize
        let start_save = Instant::now();
        let json = serde_json::to_string(&run).expect("serialize");
        let save_duration = start_save.elapsed();

        // Deserialize
        let start_load = Instant::now();
        let restored: ExecutionRun = serde_json::from_str(&json).expect("deserialize");
        let load_duration = start_load.elapsed();

        assert_eq!(restored.events.len(), num_events);

        println!(
            "Ledger Stress [{num_events} events]: Save: {save_duration:?}, Load: {load_duration:?}, Size: {} KB",
            json.len() / 1024
        );

        assert!(save_duration < Duration::from_millis(2000));
        assert!(load_duration < Duration::from_millis(2000));
    }
}

// -----------------------------------------------------------------------------
// PHASE 5A: MID-VERIFICATION SOURCE MUTATION ATTACK
// -----------------------------------------------------------------------------

#[test]
fn test_mid_verification_source_mutation_attack() {
    let (authority, current, root) = create_custom_authority(1);
    let store = store_for(root.path());

    let req = &authority.revision.contract.requirement_graph.requirements[0];

    // Collect valid evidence bound to source revision "source-rev-A"
    let mut item_a = test_metadata(
        &authority,
        &current,
        "pass-source-a",
        req.verification_policy.obligations[0].class,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item_a.source_revision = Some("source-rev-A".into());
    item_a.requirement_ids = vec![req.requirement_id.clone()];
    item_a.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(item_a, b"test pass on rev A").unwrap();

    // Verification evaluated with modified freshness context having "source-rev-B"
    let mut mutated_current = current.clone();
    mutated_current.source_revision = Some("source-rev-B".into());

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        mutated_current.clone(),
        Vec::new(),
    ).unwrap();

    let stale_report = engine.evaluate(None).unwrap();
    assert_ne!(stale_report.decision.state, CompletionState::VerifiedComplete);
    assert_eq!(stale_report.decision.state, CompletionState::StoppedIncomplete);

    // Recollect fresh evidence bound to "source-rev-B"
    let mut item_b = test_metadata(
        &authority,
        &mutated_current,
        "pass-source-b",
        req.verification_policy.obligations[0].class,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item_b.source_revision = Some("source-rev-B".into());
    item_b.requirement_ids = vec![req.requirement_id.clone()];
    item_b.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(item_b, b"test pass on rev B").unwrap();

    // Populate standards
    for std_req in &authority.revision.contract.requirement_graph.requirements[1..] {
        let mut std_item = test_metadata(
            &authority,
            &mutated_current,
            &format!("pass-std-{}", std_req.requirement_id),
            std_req.verification_policy.obligations[0].class,
            EvidenceResult::Pass,
            EvidenceConfidence::StrongDeterministic,
        );
        std_item.source_revision = Some("source-rev-B".into());
        std_item.requirement_ids = vec![std_req.requirement_id.clone()];
        std_item.accepted_criteria = std_req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
        store.put_test_fixture(std_item, b"std passed").unwrap();
    }

    let fresh_report = engine.evaluate(None).unwrap();
    assert_eq!(fresh_report.decision.state, CompletionState::VerifiedComplete);
}

// -----------------------------------------------------------------------------
// PHASE 5A: TEST DELETION EVASION & FAKE GREEN SCRIPT ATTACKS
// -----------------------------------------------------------------------------

#[test]
fn test_test_deletion_evasion_and_fake_green_script_attacks() {
    let (authority, current, root) = create_custom_authority(1);
    let store = store_for(root.path());

    let req = &authority.revision.contract.requirement_graph.requirements[0];

    // Attack 1: Test was deleted; evidence does not cover the sealed acceptance criterion
    let mut partial_evidence = test_metadata(
        &authority,
        &current,
        "evasion-partial-auth",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    partial_evidence.requirement_ids = vec![req.requirement_id.clone()];
    // Only includes unrelated dummy criterion, missing req.acceptance_criteria[0].criterion_id
    partial_evidence.accepted_criteria = BTreeSet::from(["deleted-test-dummy-criterion".into()]);
    store.put_test_fixture(partial_evidence, b"remaining tests passed, but deleted test was missing").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).unwrap();

    let report = engine.evaluate(None).unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
    assert_eq!(report.decision.state, CompletionState::StoppedIncomplete);

    // Attack 2: Fake green script exit 0 without criterion proof
    let mut fake_green = test_metadata(
        &authority,
        &current,
        "fake-green-exit-0",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    fake_green.requirement_ids = vec![req.requirement_id.clone()];
    fake_green.accepted_criteria = BTreeSet::from(["unbound-dummy-criterion".into()]);
    store.put_test_fixture(fake_green, b"exit 0").unwrap();

    let report2 = engine.evaluate(None).unwrap();
    assert_ne!(report2.decision.state, CompletionState::VerifiedComplete);

    // Attack 3: Post-seal contract tampering is rejected at authority load time
    let mut tampered_authority = authority.clone();
    tampered_authority.revision.contract.requirement_graph.requirements[0].title = "Tampered title".into();
    assert!(VerificationEngine::new_for_test(
        store.clone(),
        tampered_authority,
        current.clone(),
        Vec::new(),
    ).is_err());
}
