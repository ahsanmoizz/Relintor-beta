use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_execution::{
    ActionRequest, ActionResult, ExecutionRun, ExecutionRunState, SchedulerPolicy,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    MissionRevision, ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority,
    RequirementRisk, RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        revision: "p7-final-source-closure".into(),
    }
}

fn seed(id: &str) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: id.into(),
        intent: format!("Implement {id}"),
        source: RequirementSource::User {
            reference: format!("audit://{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "Runtime behavior is demonstrated".into(),
            criterion_type: "functional".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "final P7 source closure".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    }
}

fn sealed_fixture() -> (
    MissionRevision,
    relintor_standards::ExecutionHandoff,
    relintor_standards::StandardsRegistry,
    TrustedSignerSet,
) {
    let key = SigningKey::from_bytes(&[71_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p7-final-source-closure", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p7-final-source-closure".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        }],
    };
    let authority = ProjectAuthorityInput {
        requirements: vec![seed("requirement_abcdef1234567890")],
        source_revision: "closure-source".into(),
        source_fingerprint: "closure-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p7-final-source-closure",
            "project-p7-final-source-closure",
            "source-v1",
            "workspace-v1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "closure".into())])),
            authority,
            Vec::new(),
        )
        .unwrap();
    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .unwrap();
    (revision, handoff, registry, trusted)
}

fn workspace() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target = if target.is_absolute() {
        target
    } else {
        std::env::current_dir()
            .expect("resolve P7 source-closure working directory")
            .join(target)
    };
    target.join("p7-final-source-closure")
}

fn run() -> ExecutionRun {
    let (revision, handoff, registry, trusted) = sealed_fixture();
    let workspace = workspace();
    fs::create_dir_all(&workspace).expect("create bounded test workspace");
    ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        workspace,
        "closure-workspace-fingerprint",
        SchedulerPolicy::default(),
        1_000,
    )
    .unwrap()
}

fn action(root: &Path) -> ActionRequest {
    ActionRequest {
        tool: "workspace".into(),
        operation: "write".into(),
        arguments: vec!["write".into()],
        working_scope: root.display().to_string(),
        environment_identity: "closure-audit".into(),
        mutable: true,
        paths: vec![root.join("src/closure.rs").display().to_string()],
        external_authority: None,
    }
}

#[test]
fn caller_supplied_success_result_cannot_authorize_task_completion() {
    let mut run = run();
    let task = run.runnable_tasks().into_iter().next().unwrap();
    let packet = run.start_task(&task, 1_100).unwrap();
    let root = run.workspace.clone();
    let request = action(&root);

    run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101)
        .unwrap();

    // This is intentionally caller-created, not returned by the adapter path.
    let result = ActionResult {
        success: true,
        failure_class: None,
        failure_message: None,
        changed_paths: vec![],
        workspace_fingerprint: "caller-fabricated-success".into(),
        passing_tests: 0,
        useful_artifacts: 0,
        wall_time_ms: 1,
        estimated_cost_micros: None,
    };
    run.record_action(&task, &request, result, 1_102).unwrap();

    assert!(
        run.finish_task(&task, 1_103).is_err(),
        "caller-created ActionResult was enough to forge FinishedAwaitingVerification"
    );
}

#[test]
fn prefix_sibling_change_is_external_modification() {
    let mut run = run();
    let expected = run.workspace.display().to_string();
    let changed = format!("{}-evil\\outside.rs", expected);

    run.detect_external_modification("before", "after", &[expected], vec![changed], 1_200)
        .unwrap();

    assert!(
        matches!(
            run.state,
            ExecutionRunState::BlockedExternal | ExecutionRunState::RevalidationRequired
        ),
        "prefix-sibling external modification was treated as an expected leased change"
    );
}
