use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_antigravity::MockAdapter;
use relintor_execution::{
    ActionRequest, ActionResult, AttemptExecutionBoundary, ExecutionError, ExecutionRun,
    ExecutionRunState, ExecutionTaskState, FailureClass, LeaseStatus, MeasurementQuality,
    ProgressSnapshot, RetryDecision, SchedulerPolicy, TaskAttemptState, WatchdogState,
    WorkspaceArtifactChangeKind, ADAPTER_PROCESS_SAFETY_TIMEOUT_MS, BETA_TASK_EXECUTION_BUDGET_MS,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    MissionRevision, ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority,
    RequirementRisk, RequirementSource, StandardsRegistry, TrustedSigner, TrustedSignerSet,
    VerificationPolicy,
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
        revision: "p7-context-1".into(),
    }
}

fn evidence() -> EvidenceObligation {
    EvidenceObligation {
        class: EvidenceClass::TestOutput,
        minimum_confidence: EvidenceConfidence::StrongDeterministic,
        rationale: "P7 acceptance test output".into(),
        required: true,
    }
}

fn seed(id: &str, dependencies: Vec<&str>) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: format!("P7 {id}"),
        intent: format!("Execute the {id} orchestration objective"),
        source: RequirementSource::User {
            reference: format!("p7-test-{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "The deterministic execution behavior is observed".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![evidence()],
            p8_collector_required: false,
        },
        dependencies: dependencies.into_iter().map(str::to_owned).collect(),
        risk: RequirementRisk::Medium,
        requirement_type: "p7-test-scope".into(),
    }
}

fn sealed_fixture() -> (
    MissionRevision,
    relintor_standards::ExecutionHandoff,
    StandardsRegistry,
    TrustedSignerSet,
) {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let mut registry = builtin_registry();
    registry
        .sign("p7-test-signer", &signing_key)
        .expect("registry signs");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p7-test-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };
    let authority = ProjectAuthorityInput {
        requirements: vec![
            seed("A-01", vec!["A-02"]),
            seed("A-02", vec!["A-03"]),
            seed("A-03", vec![]),
        ],
        source_revision: "p7-fixture-revision".into(),
        source_fingerprint: "p7-fixture-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p7-test",
            "project-p7-test",
            "project-source-1",
            "workspace-source-1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p7-test".into())])),
            authority,
            Vec::new(),
        )
        .expect("build P6 draft");
    let sealed = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .expect("seal P6 fixture");
    (sealed.0, sealed.1, registry, trusted)
}

fn workspace() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target = if target.is_absolute() {
        target
    } else {
        std::env::current_dir()
            .expect("resolve P7 acceptance working directory")
            .join(target)
    };
    let root = target.join("p7-independent-acceptance");
    fs::create_dir_all(&root).expect("create disposable P7 workspace");
    root
}

fn run() -> ExecutionRun {
    run_with_context().0
}

fn run_with_context() -> (
    ExecutionRun,
    MissionRevision,
    relintor_standards::ExecutionHandoff,
) {
    let (revision, handoff, registry, trusted) = sealed_fixture();
    let root = workspace();
    let run = ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        root,
        "p7-workspace-fingerprint",
        SchedulerPolicy::default(),
        1_000,
    )
    .expect("construct P7 run");
    (run, revision, handoff)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}

fn task_id(run: &ExecutionRun, requirement_id: &str) -> String {
    run.tasks
        .values()
        .find(|task| task.requirement_ids.iter().any(|id| id == requirement_id))
        .map(|task| task.task_id.clone())
        .expect("fixture task exists")
}

fn first_runnable(run: &ExecutionRun) -> String {
    run.runnable_tasks()
        .into_iter()
        .next()
        .expect("runnable task")
}

fn action(root: &Path, operation: &str, path: Option<&Path>) -> ActionRequest {
    ActionRequest {
        tool: "workspace".into(),
        operation: operation.into(),
        arguments: vec![operation.into()],
        working_scope: root.display().to_string(),
        environment_identity: "p7-test-environment".into(),
        mutable: true,
        paths: path
            .map(|path| vec![path.display().to_string()])
            .unwrap_or_default(),
        external_authority: None,
    }
}

fn result(success: bool, failure_class: Option<FailureClass>, _root: &Path) -> ActionResult {
    ActionResult {
        success,
        failure_class,
        failure_message: (!success).then(|| "P7 fixture failure".into()),
        changed_paths: Vec::new(),
        workspace_fingerprint: "p7-workspace-after-action".into(),
        passing_tests: u64::from(success),
        useful_artifacts: u64::from(success),
        wall_time_ms: 10,
        estimated_cost_micros: Some(1),
    }
}

fn start_with_action(
    run: &mut ExecutionRun,
    task: &str,
    operation: &str,
) -> relintor_execution::TaskPacket {
    let packet = run.start_task(task, 1_100).expect("start task");
    let root = run.workspace.clone();
    let path = root.join("src").join("p7.rs");
    let request = action(&root, operation, Some(&path));
    run.authorize_action(task, &packet.lease_id, &packet, &request, 1_101)
        .expect("authorize fixture action");
    packet
}

#[test]
fn dependency_chain_is_scheduled_in_dependency_order() {
    let (mut run, revision, handoff) = run_with_context();
    let mut adapter = MockAdapter::supported();
    let c = task_id(&run, "A-03");
    let b = task_id(&run, "A-02");
    let a = task_id(&run, "A-01");
    assert!(run.runnable_tasks().contains(&c));
    assert!(!run.runnable_tasks().contains(&b));
    let root = run.workspace.clone();
    while run.tasks.get(&c).unwrap().state != ExecutionTaskState::FinishedAwaitingVerification {
        run.execute_next_with_adapter(&mut adapter, &revision, &handoff, &root, now_ms())
            .expect("execute runnable task");
    }
    assert_eq!(run.state, ExecutionRunState::Ready);
    assert!(run
        .leases
        .iter()
        .all(|lease| lease.status != LeaseStatus::Active));
    assert!(matches!(
        run.start_task(&c, now_ms()),
        Err(ExecutionError::DependencyNotReady(_))
    ));
    assert!(run.runnable_tasks().contains(&b));
    while run.tasks.get(&b).unwrap().state != ExecutionTaskState::FinishedAwaitingVerification {
        run.execute_next_with_adapter(&mut adapter, &revision, &handoff, &root, now_ms())
            .expect("execute runnable task");
    }
    assert!(run.runnable_tasks().contains(&a));
}

#[test]
fn clean_task_boundary_authorizes_one_fresh_automatic_continuation() {
    let (mut run, revision, handoff) = run_with_context();
    let root = run.workspace.clone();
    let mut adapter = MockAdapter::supported();
    let first = first_runnable(&run);
    run.execute_next_with_adapter(&mut adapter, &revision, &handoff, &root, now_ms())
        .expect("first task completes through the adapter boundary");
    assert_eq!(
        run.tasks[&first].state,
        ExecutionTaskState::FinishedAwaitingVerification
    );
    assert_eq!(run.state, ExecutionRunState::Ready);

    let attempt_count = run.attempts.len();
    let next = run
        .authorize_next_task_continuation(&revision, &handoff, now_ms())
        .expect("sealed identity and task boundary revalidate")
        .expect("dependency-valid next task is authorized");
    assert_ne!(next, first);
    assert_eq!(run.attempts.len(), attempt_count);
    run.start_task(&next, now_ms())
        .expect("continuation receives a fresh lease and attempt");
    assert_eq!(run.attempts.len(), attempt_count + 1);
    assert_eq!(
        run.attempts.last().expect("fresh attempt").attempt_number,
        1
    );
    assert!(run
        .events
        .iter()
        .any(|event| event.kind == relintor_execution::ExecutionEventKind::ContinuationAuthorized));
}

#[test]
fn unknown_test_signal_is_not_recorded_as_observed_zero() {
    let (mut run, revision, handoff) = run_with_context();
    let mut adapter = MockAdapter::supported();
    let root = run.workspace.clone();
    run.execute_next_with_adapter(&mut adapter, &revision, &handoff, &root, now_ms())
        .expect("execute task");
    let snapshot = run.progress.last().expect("progress snapshot");
    assert_eq!(snapshot.passing_tests, 0);
    assert!(!snapshot.passing_tests_observed);
    assert!(snapshot.artifacts_observed);
}

#[test]
fn stopped_continuation_dispatches_a_new_attempt_only_after_owned_process_start() {
    let (mut run, revision, handoff) = run_with_context();
    let task = first_runnable(&run);
    run.start_task(&task, 1_100)
        .expect("create original attempt");
    run.mark_stopped_incomplete("user requested emergency stop", 1_101)
        .expect("persist stopped incomplete state");
    run.resume_from_recovery(
        relintor_execution::RecoveryDisposition::StoppedIncomplete,
        1_102,
    )
    .expect("authorize continuation");
    assert_eq!(run.state, ExecutionRunState::Ready);
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(run.attempts[0].state, TaskAttemptState::SafeBoundaryStopped);

    let root = run.workspace.clone();
    let mut adapter = MockAdapter::supported();
    let mut observed = None;
    run.execute_next_with_adapter_with_started_callback(
        &mut adapter,
        &revision,
        &handoff,
        &root,
        1_103,
        |snapshot, identity| {
            let attempt = snapshot.attempts.last().expect("replacement attempt");
            observed = Some((
                attempt.attempt_id.clone(),
                attempt.execution_boundary,
                identity.pid,
            ));
            Ok(())
        },
    )
    .expect("dispatch continuation through the shared worker path");

    let (attempt_id, boundary, pid) = observed.expect("owned process start callback");
    assert_ne!(attempt_id, run.attempts[0].attempt_id);
    assert_eq!(boundary, AttemptExecutionBoundary::ExternalProcessStarted);
    assert_eq!(
        pid, 0,
        "the deterministic test adapter must expose its own identity"
    );
    assert_eq!(run.attempts.len(), 2);
    assert_eq!(run.attempts[0].state, TaskAttemptState::SafeBoundaryStopped);
}

#[test]
fn independent_nonconflicting_tasks_can_run_in_parallel() {
    let mut run = run();
    let ids = run.runnable_tasks();
    assert!(ids.len() >= 2);
    for (index, id) in ids.iter().take(2).enumerate() {
        let task = run.tasks.get_mut(id).expect("task");
        task.scope.file_scopes = vec![run
            .workspace
            .join(format!("file-{index}.rs"))
            .display()
            .to_string()];
        task.scope.directory_scopes.clear();
        task.scope.shared_resources.clear();
        task.scope.package_lockfiles.clear();
        task.scope.generated_files.clear();
    }
    assert_eq!(run.plan_parallel_batch().len(), 2);
}

#[test]
fn conflicting_scopes_are_serialized() {
    let run = run();
    assert_eq!(run.plan_parallel_batch().len(), 1);
}

#[test]
fn repeated_failing_command_injects_diagnostics_and_stops() {
    let mut run = run();
    let task = first_runnable(&run);
    let root = run.workspace.clone();
    run.tasks.get_mut(&task).unwrap().retry_policy.max_attempts = 4;
    for attempt in 0..3 {
        let packet = run
            .start_task(&task, 1_100 + attempt * 10)
            .expect("start attempt");
        let request = action(&root, "cargo test", Some(&root.join("src/p7.rs")));
        let decision = run
            .record_action(
                &task,
                &request,
                result(false, Some(FailureClass::Transient), &root),
                1_101 + attempt * 10,
            )
            .expect("record failed action");
        if attempt < 2 {
            assert_eq!(decision, RetryDecision::Retry);
        } else {
            assert_eq!(decision, RetryDecision::InjectDiagnostic);
            assert_eq!(run.watchdog_state, WatchdogState::RepeatedCommand);
            assert_eq!(run.diagnostics.len(), 1);
            assert_eq!(run.tasks[&task].state, ExecutionTaskState::BlockedExternal);
        }
        assert_eq!(packet.attempt_number, (attempt + 1) as u32);
    }
}

#[test]
fn deterministic_failures_are_not_blindly_retried() {
    let run = run();
    let task = first_runnable(&run);
    assert_eq!(
        run.retry_decision(&task, FailureClass::Deterministic)
            .unwrap(),
        RetryDecision::Stop
    );
}

#[test]
fn transient_failures_receive_a_bounded_retry() {
    let mut run = run();
    let task = first_runnable(&run);
    let root = run.workspace.clone();
    let packet = run.start_task(&task, 1_100).expect("start");
    let request = action(&root, "cargo check", Some(&root.join("src/p7.rs")));
    assert_eq!(
        run.record_action(
            &task,
            &request,
            result(false, Some(FailureClass::Transient), &root),
            1_101
        )
        .unwrap(),
        RetryDecision::Retry
    );
    assert_eq!(run.tasks[&task].state, ExecutionTaskState::WaitingRetry);
    assert_eq!(run.usage.retry_count, 1);
    let retry = run.start_task(&task, 1_200).expect("retry start");
    assert_eq!(retry.attempt_number, packet.attempt_number + 1);
}

#[test]
fn retry_limit_is_enforced() {
    let mut run = run();
    let task = first_runnable(&run);
    run.tasks.get_mut(&task).unwrap().retry_policy.max_attempts = 1;
    run.start_task(&task, 1_100).expect("start");
    let root = run.workspace.clone();
    let request = action(&root, "cargo check", Some(&root.join("src/p7.rs")));
    assert_eq!(
        run.record_action(
            &task,
            &request,
            result(false, Some(FailureClass::Transient), &root),
            1_101
        )
        .unwrap(),
        RetryDecision::Stop
    );
    assert_eq!(run.tasks[&task].state, ExecutionTaskState::Failed);
}

#[test]
fn edit_revert_oscillation_reaches_a_safe_boundary() {
    let mut run = run();
    for index in 0..4 {
        let (before, after) = if index % 2 == 0 {
            ("before", "after")
        } else {
            ("after", "before")
        };
        assert!(
            !run.record_oscillation(before, after, vec!["src/p7.rs".into()], 1_100)
                .unwrap_or(false)
                || run.watchdog_state == WatchdogState::OscillationDetected
        );
    }
    assert_eq!(run.watchdog_state, WatchdogState::OscillationDetected);
    assert_eq!(run.state, ExecutionRunState::SafeBoundaryReached);
}

#[test]
fn no_progress_loop_is_blocked_after_threshold() {
    let mut run = run();
    let snapshot = ProgressSnapshot {
        workspace_fingerprint: "same".into(),
        task_state_fingerprint: "same".into(),
        ..ProgressSnapshot::default()
    };
    for _ in 0..4 {
        run.record_workspace_observation(snapshot.clone(), 1_100)
            .expect("observe progress");
    }
    assert_eq!(run.watchdog_state, WatchdogState::NoProgress);
    assert_eq!(run.state, ExecutionRunState::BlockedExternal);
}

#[test]
fn early_agent_stop_creates_incomplete_continuation() {
    let mut run = run();
    let task = first_runnable(&run);
    run.start_task(&task, 1_100).expect("start");
    run.agent_stopped(&task, "agent turn ended", 1_101)
        .expect("stop agent");
    assert_eq!(run.state, ExecutionRunState::TurnEndedIncomplete);
    assert_eq!(run.continuations.len(), 1);
    assert_eq!(run.attempts[0].state, TaskAttemptState::TurnEndedIncomplete);
}

#[test]
fn continuation_starts_a_clean_next_turn() {
    let mut run = run();
    let task = first_runnable(&run);
    run.start_task(&task, 1_100).expect("start");
    run.agent_stopped(&task, "turn ended", 1_101).expect("stop");
    run.start_next_turn(2_000).expect("continue");
    assert_eq!(run.state, ExecutionRunState::Ready);
    assert_eq!(run.current_turn, 2);
}

#[test]
fn continuation_preserves_budget_attempt_and_loop_history() {
    let mut run = run();
    let task = first_runnable(&run);
    let packet = start_with_action(&mut run, &task, "cargo test");
    let root = run.workspace.clone();
    let request = action(&root, "cargo test", Some(&root.join("src/p7.rs")));
    run.record_action(&task, &request, result(true, None, &root), 1_102)
        .expect("record");
    run.agent_stopped(&task, "turn ended after one action", 1_103)
        .expect("stop");
    let continuation = run.continuations.last().expect("continuation");
    assert_eq!(continuation.attempt_number, packet.attempt_number);
    assert!(continuation.budget_remaining.tool_calls < run.policy.default_budget.tool_calls);
    assert_eq!(continuation.loop_history.len(), 0);
}

#[test]
fn task_drift_is_denied_before_mutation() {
    let mut run = run();
    let task = first_runnable(&run);
    let packet = run.start_task(&task, 1_100).expect("start");
    let root = run.workspace.clone();
    let request = action(&root, "write", Some(Path::new("D:/outside-relintor/p7.rs")));
    assert!(matches!(
        run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101),
        Err(ExecutionError::TaskDrift)
    ));
    assert_eq!(run.watchdog_state, WatchdogState::DriftDetected);
}

#[test]
fn excessive_tool_calls_are_counted_and_denied() {
    let mut run = run();
    let task = first_runnable(&run);
    run.tasks.get_mut(&task).unwrap().usage_budget.tool_calls = 1;
    let packet = run.start_task(&task, 1_100).expect("start");
    let root = run.workspace.clone();
    let request = action(&root, "write", Some(&root.join("src/p7.rs")));
    run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101)
        .expect("first authorization");
    run.record_action(&task, &request, result(true, None, &root), 1_102)
        .expect("first action");
    assert!(matches!(
        run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_103),
        Err(ExecutionError::BudgetExhausted)
    ));
    assert_eq!(run.watchdog_state, WatchdogState::BudgetExhausted);
}

#[test]
fn execution_step_budget_is_enforced() {
    let mut run = run();
    let task = first_runnable(&run);
    run.tasks
        .get_mut(&task)
        .unwrap()
        .usage_budget
        .execution_steps = 1;
    let packet = run.start_task(&task, 1_100).expect("start");
    let root = run.workspace.clone();
    let request = action(&root, "write", Some(&root.join("src/p7.rs")));
    run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101)
        .expect("first authorization");
    run.record_action(&task, &request, result(true, None, &root), 1_102)
        .expect("first action");
    assert!(matches!(
        run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_103),
        Err(ExecutionError::BudgetExhausted)
    ));
}

#[test]
fn unexpected_external_workspace_modification_blocks_execution() {
    let mut run = run();
    let expected = vec![run.workspace.display().to_string()];
    run.detect_external_modification(
        "before",
        "after",
        &expected,
        vec!["D:/unexpected/p7.rs".into()],
        1_100,
    )
    .expect("record modification");
    assert_eq!(run.state, ExecutionRunState::BlockedExternal);
    assert_eq!(run.watchdog_state, WatchdogState::ExternalModification);
    assert_eq!(run.external_modifications.len(), 1);
}

#[test]
fn authority_change_requires_revalidation() {
    let (revision, handoff, registry, trusted) = sealed_fixture();
    let mut run = ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        workspace(),
        "fp",
        SchedulerPolicy::default(),
        1_000,
    )
    .unwrap();
    let mut changed = revision.clone();
    changed.seal.contract_hash = "changed-seal-hash".into();
    assert!(matches!(
        run.validate_authority_identity(&changed, &handoff, 1_100),
        Err(ExecutionError::RevalidationRequired(_))
    ));
    assert_eq!(run.state, ExecutionRunState::RevalidationRequired);
}

#[test]
fn stale_task_packet_is_rejected() {
    let mut run = run();
    let task = first_runnable(&run);
    let mut packet = run.start_task(&task, 1_100).expect("start");
    packet.objective.push_str(" forged");
    let root = run.workspace.clone();
    let request = action(&root, "write", Some(&root.join("src/p7.rs")));
    assert!(matches!(
        run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101),
        Err(ExecutionError::StaleTaskPacket)
    ));
}

#[test]
fn expired_lease_is_rejected() {
    let mut run = run();
    let task = first_runnable(&run);
    let packet = run.start_task(&task, 1_100).expect("start");
    let root = run.workspace.clone();
    let request = action(&root, "write", Some(&root.join("src/p7.rs")));
    assert!(matches!(
        run.authorize_action(
            &task,
            &packet.lease_id,
            &packet,
            &request,
            1_100 + relintor_execution::BETA_TASK_EXECUTION_BUDGET_MS,
        ),
        Err(ExecutionError::LeaseExpired)
    ));
    assert_eq!(run.leases[0].status, LeaseStatus::Active);
}

#[test]
fn beta_execution_policy_is_single_finite_and_coherent() {
    let policy = SchedulerPolicy::default();
    assert_eq!(
        policy.default_budget.wall_clock_ms,
        BETA_TASK_EXECUTION_BUDGET_MS
    );
    assert_eq!(
        policy.execution_time_policy.task_wall_clock_ms,
        BETA_TASK_EXECUTION_BUDGET_MS
    );
    assert_eq!(
        policy.execution_time_policy.lease_duration_ms,
        BETA_TASK_EXECUTION_BUDGET_MS
    );
    assert_eq!(
        policy.execution_time_policy.adapter_safety_timeout_ms,
        ADAPTER_PROCESS_SAFETY_TIMEOUT_MS
    );
    policy.execution_time_policy.validate().unwrap();
}

#[test]
fn successful_execution_exports_exact_attempt_lease_process_and_workspace_attribution() {
    let (revision, handoff, registry, trusted) = sealed_fixture();
    let root = std::env::temp_dir().join(format!(
        "identity-attribution-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("modified.txt"), b"before").unwrap();
    fs::write(root.join("deleted.txt"), b"delete me").unwrap();
    let mut run = ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        root.clone(),
        "p7-workspace-fingerprint",
        SchedulerPolicy::default(),
        now_ms(),
    )
    .unwrap();
    let mut adapter = MockAdapter::supported();
    run.execute_next_with_adapter_with_started_callback(
        &mut adapter,
        &revision,
        &handoff,
        &root,
        now_ms(),
        |_, _| {
            fs::write(root.join("modified.txt"), b"after")
                .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
            fs::write(root.join("new.txt"), b"new")
                .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
            fs::remove_file(root.join("deleted.txt"))
                .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
            Ok(())
        },
    )
    .unwrap();

    let identities = run.successful_execution_identities().unwrap();
    assert_eq!(identities.len(), 1);
    let identity = &identities[0];
    let attempt = run
        .attempts
        .iter()
        .find(|attempt| attempt.state == TaskAttemptState::Succeeded)
        .unwrap();
    assert_eq!(identity.attempt_id, attempt.attempt_id);
    assert_eq!(identity.lease_id, attempt.lease_id);
    assert_eq!(identity.process_digest.len(), 64);
    assert!(identity.ended_at_ms < identity.lease_expires_at_ms);
    assert!(identity.artifact_changes.iter().any(|change| {
        change.path == "new.txt" && change.kind == WorkspaceArtifactChangeKind::New
    }));
    assert!(identity.artifact_changes.iter().any(|change| {
        change.path == "modified.txt" && change.kind == WorkspaceArtifactChangeKind::Modified
    }));
    assert!(identity.artifact_changes.iter().any(|change| {
        change.path == "deleted.txt" && change.kind == WorkspaceArtifactChangeKind::Deleted
    }));
}

#[test]
fn lease_cannot_be_reused_for_another_task() {
    let mut run = run();
    let first = first_runnable(&run);
    let second = run
        .runnable_tasks()
        .into_iter()
        .find(|id| id != &first)
        .expect("second runnable");
    for (index, id) in [&first, &second].into_iter().enumerate() {
        let task = run.tasks.get_mut(id).expect("task");
        task.scope.file_scopes = vec![run
            .workspace
            .join(format!("lease-task-{index}.rs"))
            .display()
            .to_string()];
        task.scope.directory_scopes.clear();
        task.scope.shared_resources.clear();
        task.scope.package_lockfiles.clear();
    }
    let first_packet = run.start_task(&first, 1_100).expect("first start");
    let second_packet = run.start_task(&second, 1_101).expect("second start");
    let root = run.workspace.clone();
    let request = action(&root, "write", Some(&root.join("src/p7.rs")));
    assert!(matches!(
        run.authorize_action(
            &second,
            &first_packet.lease_id,
            &second_packet,
            &request,
            1_102
        ),
        Err(ExecutionError::LeaseBindingMismatch)
    ));
}

#[test]
fn diagnostic_tasks_are_bounded_and_system_provenanced() {
    let mut run = run();
    let identity = (
        run.mission_id.clone(),
        run.mission_revision,
        run.seal_hash.clone(),
    );
    let task = first_runnable(&run);
    let id = run
        .inject_diagnostic_task(&task, "collect compiler diagnostics", 1_100)
        .expect("inject diagnostic");
    let diagnostic = run
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.diagnostic_id == id)
        .expect("diagnostic");
    assert_eq!(diagnostic.provenance, "SYSTEM_DIAGNOSTIC");
    assert!(diagnostic.budget.execution_steps <= 5);
    assert!(diagnostic.budget.tool_calls <= 3);
    assert_eq!(
        (run.mission_id, run.mission_revision, run.seal_hash),
        identity
    );
}

#[test]
fn safe_stop_reaches_a_terminal_boundary_without_completion_authority() {
    let mut run = run();
    let task = first_runnable(&run);
    run.start_task(&task, 1_100).expect("start");
    run.request_stop(Some(&task), "user stop", 1_101)
        .expect("request stop");
    run.reach_safe_boundary(1_102).expect("reach boundary");
    assert_eq!(run.state, ExecutionRunState::Stopped);
    assert!(run
        .safe_boundary
        .as_ref()
        .is_some_and(|boundary| boundary.reached));
}

#[test]
fn all_tasks_finish_awaiting_verification_only() {
    let (mut run, revision, handoff) = run_with_context();
    let mut adapter = MockAdapter::supported();
    loop {
        let runnable = run.runnable_tasks();
        if runnable.is_empty() {
            break;
        }
        for _ in runnable {
            let root = run.workspace.clone();
            run.execute_next_with_adapter(&mut adapter, &revision, &handoff, &root, now_ms())
                .expect("execute task");
        }
    }
    assert_eq!(
        run.state,
        ExecutionRunState::ExecutionTasksFinishedAwaitingVerification
    );
    assert!(run
        .tasks
        .values()
        .all(|task| task.state == ExecutionTaskState::FinishedAwaitingVerification));
}

#[test]
fn execution_ledger_round_trips_without_changing_authority() {
    let mut run = run();
    let task = first_runnable(&run);
    let root = run.workspace.clone();
    let packet = run.start_task(&task, 1_100).expect("start");
    let request = action(&root, "cargo test", Some(&root.join("src/p7.rs")));
    run.authorize_action(&task, &packet.lease_id, &packet, &request, 1_101)
        .expect("authorize");
    run.record_action(&task, &request, result(true, None, &root), 1_102)
        .expect("record");
    let json = run.snapshot_json().expect("serialize execution ledger");
    let restored = ExecutionRun::restore_json(&json).expect("restore execution ledger");
    let ledger_path = root.join("p7-execution-ledger.json");
    run.persist_snapshot(&ledger_path)
        .expect("persist execution ledger");
    let restored_from_disk =
        ExecutionRun::restore_snapshot(&ledger_path).expect("restore persisted ledger");
    assert_eq!(restored.ledger_version, "p7-execution-ledger-v1");
    assert_eq!(restored.mission_id, run.mission_id);
    assert_eq!(restored.seal_hash, run.seal_hash);
    assert_eq!(restored_from_disk.run_id, run.run_id);
    assert_eq!(run.usage.tool_calls, 1);
    assert_eq!(run.usage.execution_steps, 1);
    assert_eq!(run.usage.attempt_count, 1);
    assert_eq!(run.usage.estimated_cost_micros, Some(1));
    assert_eq!(run.attempts[0].usage.quality, MeasurementQuality::Estimated);
}
