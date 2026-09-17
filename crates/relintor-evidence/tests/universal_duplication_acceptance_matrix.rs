//! Generic Duplication Acceptance Matrix (15 deterministic acceptance tests).
//!
//! Verifies Universal Duplication Policy across all 7 classes (A through G):
//! 1. duplicate checkpoint with unique authenticated authority -> resolve correctly
//! 2. duplicate checkpoint with ambiguous authority -> fail closed
//! 3. duplicate retry trigger -> exactly one retry
//! 4. duplicate auto/manual dispatch -> exactly one task attempt
//! 5. duplicate lease creation race -> exactly one authoritative lease
//! 6. duplicate requirement from two discovery sources -> one canonical requirement if semantically identical, provenance preserved
//! 7. similar-looking but actually distinct requirements -> remain separate
//! 8. duplicate evidence file -> counted once
//! 9. same valid evidence legitimately supporting two requirements -> allowed only by explicit evidence applicability semantics
//! 10. duplicate project implementation -> detected/classified, not automatically deleted
//! 11. generated/vendor duplicates -> not falsely treated as implementation defects
//! 12. stale discovery generation -> never contaminates current generation
//! 13. conflicting derived caches -> canonical authority wins
//! 14. duplicate certificate issuance trigger -> exactly one canonical certificate identity
//! 15. duplicate HumanDecision submission -> deterministic idempotent result, no double authority event

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::test_support::FixtureStoreExt;
use relintor_evidence::*;
use relintor_execution::{
    CheckpointContent, CheckpointKind, ExecutionLease, ExecutionRun, ExecutionRunState,
    ExecutionTask, ExecutionTaskState, GitWorktreeSnapshot, LeaseScope, LeaseStatus,
    RecoveryAuthority, RecoveryError, RecoveryStore, RetryPolicy, SchedulerPolicy,
    TaskAttempt, UntrackedFileSnapshot, UsageBudget, WorkspaceSnapshot,
};
use relintor_standards::duplication::*;
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    ApplicabilityOutcome, AuthorityEngine, EvidenceClass, EvidenceConfidence,
    EvidenceObligation, FactValue, ProjectAuthorityInput, ProjectRequirementSeed, Requirement,
    RequirementPriority, RequirementRisk, RequirementSource, RequirementStatus, TrustedSigner,
    TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
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
        revision: "p8-context-dup".into(),
    }
}

fn authority_fixture_seeds(
    with_human: bool,
    extra_seed: Option<ProjectRequirementSeed>,
) -> (VerificationAuthority, FreshnessContext, TempDir) {
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

    let req1 = ProjectRequirementSeed {
        requirement_id: Some("REQ-DUP-01".into()),
        title: "Deterministic Test Req".into(),
        intent: "Verify test output obligation".into(),
        source: RequirementSource::User {
            reference: "user-req-1".into(),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "crit-dup-1".into(),
            statement: "Collector verifies output".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "requires test output".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    };

    let mut seeds = vec![req1];
    if let Some(extra) = extra_seed {
        seeds.push(extra);
    }
    if with_human {
        seeds.push(ProjectRequirementSeed {
            requirement_id: Some("REQ-HUMAN-01".into()),
            title: "Human Review Req".into(),
            intent: "Explicit review".into(),
            source: RequirementSource::User {
                reference: "user-req-human".into(),
            },
            priority: RequirementPriority::P1,
            acceptance_criteria: vec![AcceptanceCriterion {
                criterion_id: "crit-human-1".into(),
                statement: "Reviewer approves visually".into(),
                criterion_type: "user-acceptance".into(),
                machine_checkable: false,
            }],
            verification_policy: VerificationPolicy {
                obligations: vec![EvidenceObligation {
                    class: EvidenceClass::HumanDecision,
                    minimum_confidence: EvidenceConfidence::HumanAsserted,
                    rationale: "requires human decision".into(),
                    required: true,
                }],
                p8_collector_required: true,
            },
            dependencies: Vec::new(),
            risk: RequirementRisk::High,
            requirement_type: "decision".into(),
        });
    }

    let project_authority = ProjectAuthorityInput {
        requirements: seeds,
        decisions: Vec::new(),
        source_revision: "source-rev-dup".into(),
        source_fingerprint: "source-fp-dup".into(),
        ..ProjectAuthorityInput::default()
    };

    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-dup-test",
            "project-dup-test",
            "source-rev-dup",
            "workspace-dup",
            registry.clone(),
            &test_context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p8".into())])),
            project_authority,
            Vec::new(),
        )
        .expect("build draft");

    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-09-05T00:00:00Z")
        .expect("seal draft");

    let root = tempdir().expect("temp workspace");
    let authority = VerificationAuthority {
        p7_run_id: "run-dup-fixture".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fp-dup".into(),
        source_revision: Some(revision.contract.project_source_revision.clone()),
        environment_fingerprint: "env-fp-dup".into(),
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
        dependency_lock_hashes: BTreeMap::from([("Cargo.lock".into(), "lock-hash-dup".into())]),
        workspace_root: Some(root.path().to_path_buf()),
    };
    (authority, current, root)
}

fn authority_fixture(with_human: bool) -> (VerificationAuthority, FreshnessContext, TempDir) {
    authority_fixture_seeds(with_human, None)
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

fn test_checkpoint_content(root: &Path, reason: &str, time_ms: u64) -> CheckpointContent {
    let workspace = WorkspaceSnapshot::capture(root, 1).expect("capture workspace");
    let untracked = UntrackedFileSnapshot::capture(root, 1).expect("capture untracked");
    let git = GitWorktreeSnapshot::capture(root).expect("capture git state");
    CheckpointContent {
        authority: RecoveryAuthority::new(
            "project-dup",
            "mission-dup",
            1,
            "seal-dup",
            "registry-dup",
            "run-dup",
            workspace.fingerprint.clone(),
            "source-dup",
            None,
            "P8_PENDING",
            1,
        ),
        kind: CheckpointKind::AfterTaskPersistence,
        run_snapshot_json: "{}".into(),
        workspace,
        untracked,
        git,
        processes: Vec::new(),
        verification_references: vec!["p8-evidence-pending".into()],
        reason: reason.into(),
        created_at_ms: time_ms,
    }
}

fn test_execution_run(workspace: &Path) -> ExecutionRun {
    let mut tasks = BTreeMap::new();
    tasks.insert(
        "task-1".to_string(),
        ExecutionTask {
            task_id: "task-1".into(),
            objective: "Perform task 1".into(),
            requirement_ids: vec!["REQ-DUP-01".into()],
            dependency_ids: vec![],
            priority: RequirementPriority::P1,
            state: ExecutionTaskState::Ready,
            scope: LeaseScope {
                workspace: workspace.to_path_buf(),
                file_scopes: vec![],
                directory_scopes: vec![],
                shared_resources: vec![],
                package_lockfiles: vec![],
                generated_files: vec![],
                allowed_tools: BTreeSet::new(),
                external_authority: BTreeSet::new(),
                scope_known: true,
            },
            usage_budget: UsageBudget {
                wall_clock_ms: 60_000,
                ..UsageBudget::default()
            },
            retry_policy: RetryPolicy::default(),
            evidence_obligations: vec![],
            attempt_number: 0,
        },
    );

    ExecutionRun {
        ledger_version: "p7-execution-ledger-v1".into(),
        run_id: "run-dup-test".into(),
        mission_id: "mission-dup-test".into(),
        mission_revision: 1,
        seal_hash: "seal-hash-dup".into(),
        project_id: "project-dup-test".into(),
        workspace: workspace.to_path_buf(),
        workspace_fingerprint: "ws-fp-dup".into(),
        state: ExecutionRunState::Ready,
        tasks,
        attempts: vec![],
        leases: vec![],
        events: vec![],
        usage: relintor_execution::UsageTelemetry::default(),
        loop_signals: vec![],
        oscillation_signals: vec![],
        continuations: vec![],
        diagnostics: vec![],
        external_modifications: vec![],
        progress: vec![],
        watchdog_state: relintor_execution::WatchdogState::Healthy,
        policy: SchedulerPolicy::default(),
        safe_boundary: None,
        current_turn: 1,
        last_error: None,
        no_progress_occurrences: 0,
        reviewed_recovery_deltas: vec![],
        integrity_version: "p7-ledger-integrity-v1".into(),
        integrity_tag: String::new(),
    }
}

// -------------------------------------------------------------------------
// Case 1: duplicate checkpoint with unique authenticated authority -> resolve correctly
// -------------------------------------------------------------------------
#[test]
fn test_1_duplicate_checkpoint_with_unique_authenticated_authority_resolves() {
    let temp = tempdir().expect("tempdir");
    let recovery_root = temp.path().join(".relintor-recovery");
    let key = vec![42_u8; 32];
    let store = RecoveryStore::new(&recovery_root, key).expect("create store");

    let cp1 = test_checkpoint_content(temp.path(), "initial checkpoint", 1000);
    let rec1 = store.write_checkpoint(cp1).expect("write cp1");
    assert_eq!(rec1.sequence, 1);

    let cp2 = test_checkpoint_content(temp.path(), "second checkpoint", 2000);
    let rec2 = store.write_checkpoint(cp2).expect("write cp2");
    assert_eq!(rec2.sequence, 2);

    // Create an orphaned/duplicate checkpoint file for sequence 2
    let orphan_path = recovery_root.join("checkpoint-00000000000000000002-orphan_checkpoint_id.json");
    let original_path = recovery_root.join(format!(
        "checkpoint-00000000000000000002-{}.json",
        rec2.checkpoint_id
    ));
    fs::copy(&original_path, &orphan_path).expect("copy duplicate checkpoint");

    // Load latest disambiguates against signed checkpoint-index.json
    let latest = store.load_latest().expect("load latest");
    assert!(latest.is_some());
    let latest_rec = latest.unwrap();
    assert_eq!(latest_rec.sequence, 2);
    assert_eq!(latest_rec.checkpoint_id, rec2.checkpoint_id);

    // Non-selected duplicate was preserved for forensics
    assert!(orphan_path.exists());
}

// -------------------------------------------------------------------------
// Case 2: duplicate checkpoint with ambiguous authority -> fail closed
// -------------------------------------------------------------------------
#[test]
fn test_2_duplicate_checkpoint_with_ambiguous_authority_fails_closed() {
    let temp = tempdir().expect("tempdir");
    let recovery_root = temp.path().join(".relintor-recovery");
    let key = vec![42_u8; 32];
    let store = RecoveryStore::new(&recovery_root, key).expect("create store");

    let cp1 = test_checkpoint_content(temp.path(), "initial checkpoint", 1000);
    store.write_checkpoint(cp1).expect("write cp1");
    let cp2 = test_checkpoint_content(temp.path(), "second checkpoint", 2000);
    let rec2 = store.write_checkpoint(cp2).expect("write cp2");
    let cp3 = test_checkpoint_content(temp.path(), "third checkpoint", 3000);
    store.write_checkpoint(cp3).expect("write cp3");

    // Create duplicate file for intermediate sequence 2
    let orphan_path = recovery_root.join("checkpoint-00000000000000000002-orphan_ambiguous.json");
    let original_path = recovery_root.join(format!(
        "checkpoint-00000000000000000002-{}.json",
        rec2.checkpoint_id
    ));
    fs::copy(&original_path, &orphan_path).expect("copy duplicate");

    // Must fail closed with RecoveryError::Chain because sequence 2 has ambiguous lineage candidates
    let result = store.load_latest();
    assert!(result.is_err());
    match result {
        Err(RecoveryError::Chain(msg)) => {
            assert!(
                msg.contains("ambiguous authority") || msg.contains("duplicate"),
                "error message must cite ambiguous authority: {msg}"
            );
        }
        other => panic!("expected RecoveryError::Chain, got {other:?}"),
    }

    // Both files preserved for forensics
    assert!(original_path.exists());
    assert!(orphan_path.exists());
}

// -------------------------------------------------------------------------
// Case 3: duplicate retry trigger -> exactly one retry
// -------------------------------------------------------------------------
#[test]
fn test_3_duplicate_retry_trigger_results_in_exactly_one_retry() {
    let temp = tempdir().expect("tempdir");
    let mut run = test_execution_run(temp.path());

    // Task starts in WaitingRetry
    let task = run.tasks.get_mut("task-1").unwrap();
    task.state = ExecutionTaskState::WaitingRetry;
    task.attempt_number = 1;

    // Trigger 1 (e.g. user clicks Retry)
    let packet1 = run.start_task("task-1", 10_000);
    assert!(packet1.is_ok());
    assert_eq!(run.tasks.get("task-1").unwrap().attempt_number, 2);
    assert_eq!(run.tasks.get("task-1").unwrap().state, ExecutionTaskState::Running);

    // Trigger 2 (racing duplicate retry request e.g. rapid double-click)
    let packet2 = run.start_task("task-1", 10_001);
    // Must be rejected because task is already running
    assert!(packet2.is_err());

    // Total attempt count must remain exactly 2 (only one retry executed)
    assert_eq!(run.tasks.get("task-1").unwrap().attempt_number, 2);
}

// -------------------------------------------------------------------------
// Case 4: duplicate auto/manual dispatch -> exactly one task attempt
// -------------------------------------------------------------------------
#[test]
fn test_4_duplicate_auto_manual_dispatch_results_in_exactly_one_task_attempt() {
    let temp = tempdir().expect("tempdir");
    let mut run = test_execution_run(temp.path());

    // Task is Ready
    assert_eq!(run.tasks.get("task-1").unwrap().state, ExecutionTaskState::Ready);

    // Auto continuation triggers start_task
    let auto_dispatch = run.start_task("task-1", 10_000);
    assert!(auto_dispatch.is_ok());
    assert_eq!(run.tasks.get("task-1").unwrap().attempt_number, 1);

    // Manual user clicks "Run next task" concurrently
    let manual_dispatch = run.start_task("task-1", 10_000);
    assert!(manual_dispatch.is_err());

    // Attempts list contains exactly one attempt
    let task1_attempts = run
        .attempts
        .iter()
        .filter(|a| a.task_id == "task-1")
        .collect::<Vec<_>>();
    assert_eq!(task1_attempts.len(), 1);
}

// -------------------------------------------------------------------------
// Case 5: duplicate lease creation race -> exactly one authoritative lease
// -------------------------------------------------------------------------
#[test]
fn test_5_duplicate_lease_creation_race_results_in_exactly_one_authoritative_lease() {
    let temp = tempdir().expect("tempdir");
    let mut run = test_execution_run(temp.path());

    // First attempt starts
    let _ = run.start_task("task-1", 10_000).expect("start 1");
    let active_leases = run
        .leases
        .iter()
        .filter(|l| l.task_id == "task-1" && l.status == LeaseStatus::Active)
        .collect::<Vec<_>>();
    assert_eq!(active_leases.len(), 1);

    // Simulate task ending turn and subsequent retry attempt
    let task = run.tasks.get_mut("task-1").unwrap();
    task.state = ExecutionTaskState::WaitingRetry;
    let _ = run.start_task("task-1", 20_000).expect("start 2");

    // Prior lease revoked, exactly ONE active lease remains
    let active_leases_after = run
        .leases
        .iter()
        .filter(|l| l.task_id == "task-1" && l.status == LeaseStatus::Active)
        .collect::<Vec<_>>();
    assert_eq!(active_leases_after.len(), 1);
    assert_eq!(active_leases_after[0].attempt_number, 2);
}

// -------------------------------------------------------------------------
// Case 6: duplicate requirement from two discovery sources -> one canonical requirement if semantically identical, provenance preserved
// -------------------------------------------------------------------------
#[test]
fn test_6_duplicate_requirement_from_two_sources_merges_with_provenance_preserved() {
    let req1 = Requirement {
        requirement_id: "req-auth-jwt".into(),
        title: "Enforce JWT authentication".into(),
        intent: "Protect API routes".into(),
        source: RequirementSource::Standard {
            registry_id: "reg-1".into(),
            registry_version: 1,
            pack_id: "backend-api".into(),
            rule_id: "jwt-auth".into(),
            rule_digest: "digest-1".into(),
            applicability_id: "app-1".into(),
        },
        priority: RequirementPriority::P1,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "crit-jwt-1".into(),
            statement: "JWT tokens must be verified".into(),
            criterion_type: "security".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::High,
        status: RequirementStatus::ImplementedUnverified,
        implementation_links: vec![],
        evidence_links: vec![],
        explicit_exceptions: vec![],
        sealed_hash: None,
        requirement_type: "security".into(),
        origin_rule_id: Some("jwt-auth".into()),
        revision: 1,
        schema_version: 1,
    };

    let req2 = Requirement {
        requirement_id: "req-takeover-jwt".into(),
        title: "Enforce JWT authentication".into(),
        intent: "Protect API routes from takeover finding".into(),
        source: RequirementSource::TakeoverFinding {
            reference: "finding-jwt-leak".into(),
        },
        priority: RequirementPriority::P1,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "crit-jwt-1".into(),
            statement: "JWT tokens must be verified".into(),
            criterion_type: "security".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::High,
        status: RequirementStatus::ImplementedUnverified,
        implementation_links: vec![],
        evidence_links: vec![],
        explicit_exceptions: vec![],
        sealed_hash: None,
        requirement_type: "security".into(),
        origin_rule_id: Some("jwt-auth".into()),
        revision: 1,
        schema_version: 1,
    };

    let deduplicated = RequirementDeduplicator::deduplicate(&[req1, req2]);
    assert_eq!(deduplicated.len(), 1, "must yield exactly one canonical requirement");

    let canonical = &deduplicated[0];
    assert!(canonical.implementation_links.contains(&"standards://backend-api/jwt-auth".to_string()));
    assert!(canonical.implementation_links.contains(&"takeover://finding-jwt-leak".to_string()));
}

// -------------------------------------------------------------------------
// Case 7: similar-looking but actually distinct requirements -> remain separate
// -------------------------------------------------------------------------
#[test]
fn test_7_similar_looking_distinct_requirements_remain_separate() {
    let req1 = Requirement {
        requirement_id: "req-login-validation".into(),
        title: "Validate user input on login form".into(),
        intent: "Prevent malformed login submissions".into(),
        source: RequirementSource::User {
            reference: "user-req-1".into(),
        },
        priority: RequirementPriority::P2,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: vec![],
        verification_policy: VerificationPolicy {
            obligations: vec![],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::Medium,
        status: RequirementStatus::ImplementedUnverified,
        implementation_links: vec![],
        evidence_links: vec![],
        explicit_exceptions: vec![],
        sealed_hash: None,
        requirement_type: "login_form_validation".into(),
        origin_rule_id: None,
        revision: 1,
        schema_version: 1,
    };

    let req2 = Requirement {
        requirement_id: "req-register-validation".into(),
        title: "Validate user input on registration form".into(),
        intent: "Prevent malformed registration submissions".into(),
        source: RequirementSource::User {
            reference: "user-req-2".into(),
        },
        priority: RequirementPriority::P2,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: vec![],
        verification_policy: VerificationPolicy {
            obligations: vec![],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::Medium,
        status: RequirementStatus::ImplementedUnverified,
        implementation_links: vec![],
        evidence_links: vec![],
        explicit_exceptions: vec![],
        sealed_hash: None,
        requirement_type: "registration_form_validation".into(),
        origin_rule_id: None,
        revision: 1,
        schema_version: 1,
    };

    let deduplicated = RequirementDeduplicator::deduplicate(&[req1, req2]);
    assert_eq!(
        deduplicated.len(),
        2,
        "distinct requirements must not be merged merely because of similar wording"
    );
}

// -------------------------------------------------------------------------
// Case 8: duplicate evidence file -> counted once
// -------------------------------------------------------------------------
#[test]
fn test_8_duplicate_evidence_file_counted_once() {
    let temp = tempdir().expect("tempdir");
    let store_root = temp.path().join(".relintor-evidence");
    let key = vec![11_u8; 32];
    let store = EvidenceStore::new(&store_root, &key).expect("create store");

    let (authority, current, _dir) = authority_fixture(false);
    let metadata = test_metadata(
        &authority,
        &current,
        "evidence-dup-test-1",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    let bytes = b"test output log contents";

    // Put once
    let art1 = store.put_test_fixture(metadata.clone(), bytes).expect("put 1");
    // Put twice with identical receipt/bytes
    let art2 = store.put_test_fixture(metadata, bytes).expect("put 2");

    assert_eq!(art1.digest, art2.digest);
    let list = store.list().expect("list evidence");
    assert_eq!(list.len(), 1, "duplicate evidence file must count exactly once");
}

// -------------------------------------------------------------------------
// Case 9: same valid evidence legitimately supporting two requirements -> allowed only by explicit evidence applicability semantics
// -------------------------------------------------------------------------
#[test]
fn test_9_same_valid_evidence_legitimately_supporting_two_requirements() {
    let temp = tempdir().expect("tempdir");
    let store_root = temp.path().join(".relintor-evidence");
    let key = vec![11_u8; 32];
    let store = EvidenceStore::new(&store_root, &key).expect("create store");

    let extra_req = ProjectRequirementSeed {
        requirement_id: Some("REQ-SHARED-TARGET".into()),
        title: "Shared Target Req".into(),
        intent: "Verify shared test output obligation".into(),
        source: RequirementSource::User {
            reference: "user-req-shared".into(),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "crit-shared-1".into(),
            statement: "Collector verifies output".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "requires shared test output".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    };

    let (authority, current, _dir) = authority_fixture_seeds(false, Some(extra_req));

    // Artifact metadata explicitly declares support for BOTH REQ-DUP-01 and REQ-SHARED-TARGET
    let mut metadata = test_metadata(
        &authority,
        &current,
        "shared-evidence-1",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    metadata.requirement_ids = vec!["REQ-DUP-01".into(), "REQ-SHARED-TARGET".into()];

    let _ = store.put_test_fixture(metadata, b"shared integration tests pass").expect("put");

    let engine = VerificationEngine::new_for_test(store, authority.clone(), current, Vec::new()).unwrap();
    let report = engine.evaluate(None).expect("evaluate");

    // Both requirements have the evidence ID in their verification
    let v1 = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-DUP-01").unwrap();
    let v2 = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-SHARED-TARGET").unwrap();
    assert!(v1.evidence_ids.contains(&"shared-evidence-1".to_string()));
    assert!(v2.evidence_ids.contains(&"shared-evidence-1".to_string()));
}

// -------------------------------------------------------------------------
// Case 10: duplicate project implementation -> detected/classified, not automatically deleted
// -------------------------------------------------------------------------
#[test]
fn test_10_duplicate_project_implementation_detected_and_classified_not_deleted() {
    let primary = "src/controllers/auth_controller.ts";
    let duplicate = "src/controllers/auth_controller_copy.ts";
    let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    let finding = ProjectDuplicateClassifier::classify(primary, duplicate, hash, None);

    assert_eq!(finding.classification, ProjectDuplicateClass::ActualDefect);
    assert!(finding.requires_executor_remediation);
    // Crucial product invariant: classifier reports and suggests remediation; does NOT delete files
    assert_eq!(finding.primary_path, primary);
    assert_eq!(finding.duplicate_path, duplicate);
}

// -------------------------------------------------------------------------
// Case 11: generated/vendor duplicates -> not falsely treated as implementation defects
// -------------------------------------------------------------------------
#[test]
fn test_11_generated_and_vendor_duplicates_not_treated_as_implementation_defects() {
    // Vendored package duplicate
    let vendor_finding = ProjectDuplicateClassifier::classify(
        "src/util.ts",
        "node_modules/lodash/util.js",
        "hash-1",
        None,
    );
    assert_eq!(vendor_finding.classification, ProjectDuplicateClass::VendoredDuplicate);
    assert!(!vendor_finding.requires_executor_remediation);

    // Generated file duplicate
    let gen_content = "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage pb\n";
    let gen_finding = ProjectDuplicateClassifier::classify(
        "proto/service.proto",
        "src/gen/service.pb.go",
        "hash-2",
        Some(gen_content),
    );
    assert_eq!(gen_finding.classification, ProjectDuplicateClass::GeneratedDuplicate);
    assert!(!gen_finding.requires_executor_remediation);

    // Build artifact duplicate
    let build_finding = ProjectDuplicateClassifier::classify(
        "src/index.js",
        "dist/bundle.js",
        "hash-3",
        None,
    );
    assert_eq!(build_finding.classification, ProjectDuplicateClass::MirrorCache);
    assert!(!build_finding.requires_executor_remediation);
}

// -------------------------------------------------------------------------
// Case 12: stale discovery generation -> never contaminates current generation
// -------------------------------------------------------------------------
#[test]
fn test_12_stale_discovery_generation_never_contaminates_current_generation() {
    let current_gen = DiscoveryGenerationIdentity {
        workspace: "D:/project".into(),
        source_fingerprint: "fingerprint-gen-2".into(),
        scanner_version: "1.0.0".into(),
        generation: 2,
    };

    let old_gen = DiscoveryGenerationIdentity {
        workspace: "D:/project".into(),
        source_fingerprint: "fingerprint-gen-1".into(),
        scanner_version: "1.0.0".into(),
        generation: 1,
    };

    let findings = vec![
        (old_gen.clone(), "old_obsolete_finding"),
        (current_gen.clone(), "active_current_finding_1"),
        (current_gen.clone(), "active_current_finding_2"),
    ];

    let (active, historical) = DiscoveryGenerationPolicy::select_active_findings(&current_gen, &findings);

    assert_eq!(active.len(), 2);
    assert_eq!(active[0], &"active_current_finding_1");
    assert_eq!(active[1], &"active_current_finding_2");

    assert_eq!(historical.len(), 1);
    assert_eq!(historical[0], &"old_obsolete_finding");
}

// -------------------------------------------------------------------------
// Case 13: conflicting derived caches -> canonical authority wins
// -------------------------------------------------------------------------
#[test]
fn test_13_conflicting_derived_caches_canonical_authority_wins() {
    let canonical_state = ExecutionRunState::StoppedIncomplete;
    let stale_cached_state = ExecutionRunState::Running;

    let (resolved, invalidated) = CacheAuthorityPolicy::resolve(&canonical_state, Some(&stale_cached_state));
    assert_eq!(resolved, ExecutionRunState::StoppedIncomplete, "canonical authority must win");
    assert!(invalidated, "conflicting cache must be marked invalidated");

    // When cache matches authority:
    let (clean_resolved, clean_invalidated) = CacheAuthorityPolicy::resolve(&canonical_state, Some(&canonical_state));
    assert_eq!(clean_resolved, ExecutionRunState::StoppedIncomplete);
    assert!(!clean_invalidated, "matching cache should not be invalidated");
}

// -------------------------------------------------------------------------
// Case 14: duplicate certificate issuance trigger -> exactly one canonical certificate identity
// -------------------------------------------------------------------------
#[test]
fn test_14_duplicate_certificate_issuance_trigger_produces_identical_canonical_identity() {
    let (authority, current, root) = authority_fixture(false);
    let auth_key = b"p8-test-local-key";
    let store = EvidenceStore::new(root.path().join(".relintor-evidence"), auth_key).unwrap();

    // Populate evidence so evaluation passes
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
        item.accepted_criteria = req
            .acceptance_criteria
            .iter()
            .filter(|c| c.machine_checkable)
            .map(|c| c.criterion_id.clone())
            .collect();
        store.put_test_fixture(item, b"test passed").unwrap();
    }

    let engine = VerificationEngine::new_for_test(store, authority.clone(), current, Vec::new()).unwrap();
    let report = engine.evaluate(None).expect("evaluate");

    let cert_authority = CompletionAuthority::new(auth_key).expect("create cert authority");

    // Trigger issuance 1
    let cert1 = cert_authority.issue_for_test(&report, &authority).expect("issue 1");
    // Trigger duplicate issuance 2
    let cert2 = cert_authority.issue_for_test(&report, &authority).expect("issue 2");

    // Must yield identical canonical certificate identity
    assert_eq!(cert1.certificate_id, cert2.certificate_id);
    assert_eq!(cert1.final_state, cert2.final_state);
    assert_eq!(cert1.evidence_manifest_hash, cert2.evidence_manifest_hash);
    assert_eq!(cert1.mission_id, cert2.mission_id);
}

// -------------------------------------------------------------------------
// Case 15: duplicate HumanDecision submission -> deterministic idempotent result, no double authority event
// -------------------------------------------------------------------------
#[test]
fn test_15_duplicate_human_decision_submission_is_deterministic_and_idempotent() {
    let temp = tempdir().expect("tempdir");
    let store_root = temp.path().join(".relintor-evidence");
    let key = vec![77_u8; 32];
    let store = EvidenceStore::new(&store_root, &key).expect("create store");

    // Setup authority with a human decision requirement
    let (authority, current, root) = authority_fixture(true);

    let task_id = "task-human-1";
    let packet_digest = "0".repeat(64);
    let attempt_id = "attempt-human-1";
    let lease_id = "lease-human-1";
    let process_digest = "1".repeat(64);
    let started_at_ms = 1000;
    let ended_at_ms = 2000;
    let expires_at_ms = 10000;

    let lease_raw = serde_json::json!({
        "lease_id": lease_id,
        "mission_id": authority.revision.seal.mission_id,
        "mission_revision": authority.revision.revision,
        "task_id": task_id,
        "task_packet_digest": packet_digest,
        "scope": {
            "workspace": root.path(),
            "file_scopes": [],
            "directory_scopes": [],
            "shared_resources": [],
            "package_lockfiles": [],
            "generated_files": [],
            "allowed_tools": [],
            "external_authority": [],
            "scope_known": true
        },
        "issued_at_ms": started_at_ms,
        "expires_at_ms": expires_at_ms,
        "step_budget": 10,
        "tool_call_budget": 10,
        "usage_budget": {
            "wall_clock_ms": 60000,
            "execution_steps": 10,
            "tool_calls": 10,
            "retry_attempts": 1,
            "cost_micros": null
        },
        "attempt_number": 1,
        "status": "CONSUMED",
        "lease_digest": ""
    });
    let mut lease: ExecutionLease = serde_json::from_value(lease_raw).expect("lease");
    lease.lease_digest = lease.compute_digest().expect("lease digest");

    let attempt_raw = serde_json::json!({
        "attempt_id": attempt_id,
        "task_id": task_id,
        "attempt_number": 1,
        "packet_digest": packet_digest,
        "lease_id": lease_id,
        "state": "SUCCEEDED",
        "started_at_ms": started_at_ms,
        "ended_at_ms": ended_at_ms,
        "failure_class": null,
        "failure_fingerprint": null,
        "usage": {
            "tool_calls": 0,
            "execution_steps": 0,
            "wall_time_ms": 1000,
            "retry_count": 0,
            "estimated_cost_micros": null,
            "actual_cost_micros": null,
            "attempt_count": 1,
            "quality": "MEASURED"
        },
        "termination_reason": null,
        "execution_boundary": "EXTERNAL_PROCESS_STARTED",
        "extensions_granted": 0,
        "completion_authority": {
            "task_id": task_id,
            "attempt_id": attempt_id,
            "packet_digest": packet_digest,
            "lease_id": lease_id,
            "process_digest": process_digest,
            "started_at_ms": started_at_ms,
            "ended_at_ms": ended_at_ms
        },
        "workspace_before": {},
        "workspace_after": {}
    });
    let attempt: TaskAttempt = serde_json::from_value(attempt_raw).expect("attempt");

    let mut tasks = BTreeMap::new();
    tasks.insert(
        task_id.to_string(),
        ExecutionTask {
            task_id: task_id.into(),
            objective: "Human decision task".into(),
            requirement_ids: vec!["REQ-HUMAN-01".into()],
            dependency_ids: vec![],
            priority: RequirementPriority::P1,
            state: ExecutionTaskState::FinishedAwaitingVerification,
            scope: lease.scope.clone(),
            usage_budget: lease.usage_budget.clone(),
            retry_policy: RetryPolicy::default(),
            evidence_obligations: vec![],
            attempt_number: 1,
        },
    );

    let run = ExecutionRun {
        ledger_version: "p7-execution-ledger-v1".into(),
        run_id: authority.p7_run_id.clone(),
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        seal_hash: authority.revision.seal.contract_hash.clone(),
        project_id: authority.revision.seal.project_id.clone(),
        workspace: root.path().to_path_buf(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        state: ExecutionRunState::ExecutionTasksFinishedAwaitingVerification,
        tasks,
        attempts: vec![attempt],
        leases: vec![lease],
        events: vec![],
        usage: relintor_execution::UsageTelemetry::default(),
        loop_signals: vec![],
        oscillation_signals: vec![],
        continuations: vec![],
        diagnostics: vec![],
        external_modifications: vec![],
        progress: vec![],
        watchdog_state: relintor_execution::WatchdogState::Healthy,
        policy: SchedulerPolicy::default(),
        safe_boundary: None,
        current_turn: 1,
        last_error: None,
        no_progress_occurrences: 0,
        reviewed_recovery_deltas: vec![],
        integrity_version: "p7-ledger-integrity-v1".into(),
        integrity_tag: String::new(),
    };
    let p7_exec = AuthenticatedP7Execution::from_run(run).expect("p7_exec");

    let recorder = ExplicitUserDecisionRecorder;
    let input = ExplicitUserDecisionInput {
        requirement_id: "REQ-HUMAN-01",
        approved: true,
        notes: "Approved layout as compliant",
    };

    // First submission
    let art1 = recorder
        .record(&authority, &current, &store, &p7_exec, input.clone())
        .expect("record 1");

    // Second (duplicate) submission (e.g. rapid double-click or replay)
    let art2 = recorder
        .record(&authority, &current, &store, &p7_exec, input)
        .expect("record 2");

    // Must be completely identical and idempotent
    assert_eq!(art1.metadata.evidence_id, art2.metadata.evidence_id);
    assert_eq!(art1.digest, art2.digest);
    assert_eq!(art1.integrity_tag, art2.integrity_tag);

    // Exactly one evidence artifact exists in store (no double authority event)
    let list = store.list().expect("list evidence");
    assert_eq!(list.len(), 1);
}
