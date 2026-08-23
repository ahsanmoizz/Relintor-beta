use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_execution::{
    ActionRequest, ActionResult, ExecutionError, ExecutionRun, SchedulerPolicy,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    MissionRevision, ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority,
    RequirementRisk, RequirementSource, StandardsRegistry, TrustedSigner, TrustedSignerSet,
    VerificationPolicy,
};
use serde_json::Value;
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
        revision: "p7-independent-source-audit-context".into(),
    }
}

fn evidence() -> EvidenceObligation {
    EvidenceObligation {
        class: EvidenceClass::TestOutput,
        minimum_confidence: EvidenceConfidence::StrongDeterministic,
        rationale: "independent P7 audit".into(),
        required: true,
    }
}

fn seed(id: &str, deps: Vec<&str>) -> ProjectRequirementSeed {
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
            obligations: vec![evidence()],
            p8_collector_required: true,
        },
        dependencies: deps.into_iter().map(str::to_owned).collect(),
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    }
}

fn sealed_fixture() -> (
    MissionRevision,
    relintor_standards::ExecutionHandoff,
    StandardsRegistry,
    TrustedSignerSet,
) {
    let key = SigningKey::from_bytes(&[61_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p7-independent-audit", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p7-independent-audit".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        }],
    };
    let authority = ProjectAuthorityInput {
        requirements: vec![
            seed(
                "requirement_aaaaaaaaaaaaaaaa",
                vec!["requirement_bbbbbbbbbbbbbbbb"],
            ),
            seed(
                "requirement_bbbbbbbbbbbbbbbb",
                vec!["requirement_cccccccccccccccc"],
            ),
            seed("requirement_cccccccccccccccc", vec![]),
        ],
        source_revision: "audit-source-v1".into(),
        source_fingerprint: "audit-source-fingerprint-v1".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p7-independent-audit",
            "project-p7-independent-audit",
            "source-v1",
            "workspace-v1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "audit".into())])),
            authority,
            Vec::new(),
        )
        .unwrap();
    AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .map(|sealed| (sealed.0, sealed.1, registry, trusted))
        .unwrap()
}

fn workspace() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target = if target.is_absolute() {
        target
    } else {
        std::env::current_dir()
            .expect("resolve P7 source-audit working directory")
            .join(target)
    };
    target.join("p7-independent-source-audit")
}

fn run() -> ExecutionRun {
    let (revision, handoff, registry, trusted) = sealed_fixture();
    let workspace = workspace();
    fs::create_dir_all(&workspace).expect("source-audit workspace");
    ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        workspace,
        "audit-workspace-fingerprint",
        SchedulerPolicy::default(),
        1_000,
    )
    .unwrap()
}

fn task_for(run: &ExecutionRun, requirement: &str) -> String {
    run.tasks
        .values()
        .find(|task| {
            task.requirement_ids.iter().any(|id| id == requirement)
                || task.objective == format!("Implement {requirement}")
        })
        .unwrap()
        .task_id
        .clone()
}

fn action(root: &Path, paths: Vec<String>) -> ActionRequest {
    ActionRequest {
        tool: "workspace".into(),
        operation: "write".into(),
        arguments: vec!["write".into()],
        working_scope: root.display().to_string(),
        environment_identity: "independent-audit".into(),
        mutable: true,
        paths,
        external_authority: None,
    }
}

fn successful_result(cost: Option<u64>) -> ActionResult {
    ActionResult {
        success: true,
        failure_class: None,
        failure_message: None,
        changed_paths: Vec::new(),
        workspace_fingerprint: "after-action".into(),
        passing_tests: 0,
        useful_artifacts: 0,
        wall_time_ms: 1,
        estimated_cost_micros: cost,
    }
}

#[test]
fn pending_dependent_task_cannot_be_finished_without_execution_authority() {
    let mut run = run();
    let dependent = task_for(&run, "requirement_aaaaaaaaaaaaaaaa");
    assert!(!run.runnable_tasks().contains(&dependent));
    assert!(
        run.finish_task(&dependent, 1_100).is_err(),
        "finish_task bypassed dependencies/running attempt/lease"
    );
}

#[test]
fn mutable_path_traversal_outside_workspace_is_denied() {
    let mut run = run();
    let task = run.runnable_tasks().into_iter().next().unwrap();
    let packet = run.start_task(&task, 1_100).unwrap();
    let root = run.workspace.clone();
    let escaped = root.join("..").join("outside-p7-audit.rs");
    let request = action(&root, vec![escaped.display().to_string()]);
    assert!(
        matches!(
            run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101),
            Err(ExecutionError::TaskDrift) | Err(ExecutionError::PolicyDenied(_))
        ),
        "lexical ../ traversal escaped the leased workspace"
    );
}

#[test]
fn mutable_filesystem_action_without_declared_paths_is_denied() {
    let mut run = run();
    let task = run.runnable_tasks().into_iter().next().unwrap();
    let packet = run.start_task(&task, 1_100).unwrap();
    let root = run.workspace.clone();
    let request = action(&root, Vec::new());
    assert!(
        matches!(
            run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101),
            Err(ExecutionError::TaskDrift) | Err(ExecutionError::PolicyDenied(_))
        ),
        "mutable action with no bounded path declaration was authorized"
    );
}

#[test]
fn clean_turn_continuation_does_not_reset_remaining_task_budget() {
    let mut run = run();
    let task = run.runnable_tasks().into_iter().next().unwrap();
    run.tasks.get_mut(&task).unwrap().usage_budget.tool_calls = 2;

    let packet = run.start_task(&task, 1_100).unwrap();
    let root = run.workspace.clone();
    let request = action(&root, vec![root.join("src/audit.rs").display().to_string()]);

    run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101)
        .unwrap();
    run.record_action(&task, &request, successful_result(None), 1_102)
        .unwrap();
    run.agent_stopped(&task, "clean turn ended", 1_103).unwrap();

    let remaining = run
        .continuations
        .last()
        .unwrap()
        .budget_remaining
        .tool_calls;
    assert_eq!(remaining, 1);

    run.start_next_turn(1_200).unwrap();
    let next = run.start_task(&task, 1_201).unwrap();
    assert_eq!(
        next.tool_call_budget, remaining,
        "next clean turn regained the task's original full tool-call budget"
    );
}

#[test]
fn configured_cost_budget_is_enforced_before_next_mutation() {
    let mut run = run();
    let task = run.runnable_tasks().into_iter().next().unwrap();
    run.tasks.get_mut(&task).unwrap().usage_budget.cost_micros = Some(1);

    let packet = run.start_task(&task, 1_100).unwrap();
    let root = run.workspace.clone();
    let request = action(&root, vec![root.join("src/audit.rs").display().to_string()]);

    run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101)
        .unwrap();
    run.record_action(&task, &request, successful_result(Some(1)), 1_102)
        .unwrap();

    assert!(
        matches!(
            run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_103),
            Err(ExecutionError::BudgetExhausted)
        ),
        "configured cost budget was modeled but not enforced"
    );
}

#[test]
fn tampered_persisted_task_state_is_rejected_on_restore() {
    let run = run();
    let json = run.snapshot_json().unwrap();
    let mut value: Value = serde_json::from_str(&json).unwrap();

    let tasks = value
        .get_mut("tasks")
        .and_then(Value::as_object_mut)
        .expect("tasks object");
    let (_, task) = tasks.iter_mut().next().expect("task");
    task["state"] = Value::String("FINISHED_AWAITING_VERIFICATION".into());

    let tampered = serde_json::to_string(&value).unwrap();
    assert!(
        ExecutionRun::restore_json(&tampered).is_err(),
        "tampered durable execution ledger was accepted as trusted state"
    );
}
