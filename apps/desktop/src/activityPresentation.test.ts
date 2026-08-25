import { describe, expect, it } from "vitest";
import type { ExecutionStatus, VerificationStatus } from "./backend";
import { displayProjectName, displayWindowsPath, missionPresentation, verificationPresentation } from "./activityPresentation";

function execution(overrides: Partial<ExecutionStatus> = {}): ExecutionStatus {
  return {
    project_id: "project-1",
    project_name: "Accessible mission",
    mission_id: "mission-project-1",
    revision: 1,
    state: "Ready",
    watchdog_state: "Healthy",
    current_turn: 1,
    active_task: null,
    current_task_objective: "Prepare accessible interaction requirements",
    recovery_task_id: null,
    recovery_task_objective: null,
    runnable_tasks: ["task-1"],
    total_tasks: 3,
    finished_tasks: 0,
    tool_calls: 0,
    execution_steps: 0,
    estimated_cost_micros: null,
    safe_boundary_reached: false,
    last_event: "P6 handoff validated",
    ledger_path: "D:\\Relintor\\mission.json",
    recovery_state: "NO_RECOVERY_REQUIRED",
    last_safe_checkpoint: null,
    resume_disposition: null,
    resume_blocker: null,
    external_changes: [],
    recovery_detected: false,
    recovery_action: "NONE",
    dispatch_active: false,
    execution_phase: "READY",
    execution_time_limit_ms: 300_000,
    events: [],
    ...overrides,
  };
}

function verification(overrides: Partial<VerificationStatus> = {}): VerificationStatus {
  return {
    project_id: "project-1",
    mission_id: "mission-project-1",
    revision: 1,
    execution_run_id: "run-1",
    state: "VERIFICATION_FINISHED",
    completion_state: "Incomplete",
    requirements_verified: 0,
    requirements_total: 3,
    missing_evidence: [],
    failed_checks: [],
    skipped_checks: [],
    stale_evidence: [],
    blocked_external: [],
    accepted_risks: [],
    evidence_count: 0,
    certificate: null,
    workflow_stage: "READY_TO_VERIFY",
    summary: "Verification is ready.",
    human_decisions: [],
    collector_activity: [],
    collection_failures: [],
    detail: "",
    ...overrides,
  };
}

describe("user-visible mission state model", () => {
  it("shows a fresh mission as ready with no previous-attempt warning", () => {
    const view = missionPresentation(execution(), null, true);
    expect(view.headline).toBe("Ready to run");
    expect(view.primaryAction).toBe("run");
    expect(view.recoveryRequired).toBe(false);
  });

  it("never enables another dispatch while execution is running", () => {
    const view = missionPresentation(execution({ state: "Running", execution_phase: "RUNNING", dispatch_active: true }), null, true);
    expect(view.badge).toBe("Running");
    expect(view.primaryAction).toBe("none");
  });

  it("keeps the primary action disabled while a controlled continuation is dispatching", () => {
    const view = missionPresentation(execution({ finished_tasks: 1, dispatch_active: true, execution_phase: "CONTINUATION_DISPATCHING" }), null, true);
    expect(view.headline).toBe("Starting Antigravity");
    expect(view.primaryAction).toBe("none");
  });

  it("requires an explicit recovery review instead of automatic retry", () => {
    const view = missionPresentation(execution({ state: "RevalidationRequired", recovery_state: "RECOVERY_NOT_ALLOWED", recovery_detected: true }), null, true);
    expect(view.primaryAction).toBe("recover");
    expect(view.recoveryRequired).toBe(true);
  });

  it("does not render READY when executor setup is required", () => {
    const view = missionPresentation(execution(), null, false);
    expect(view.badge).toBe("Setup required");
    expect(view.primaryAction).toBe("setup");
  });

  it("rejects a contradictory verified-complete state with missing evidence", () => {
    const view = verificationPresentation(verification({ completion_state: "VerifiedComplete", requirements_verified: 3, missing_evidence: ["REQ-1: evidence missing"], evidence_count: 3 }));
    expect(view.verifiedComplete).toBe(false);
    expect(view.label).toBe("Verification needs attention");
  });

  it("does not show Verified Complete until the authority certificate is present", () => {
    const view = verificationPresentation(verification({
      completion_state: "VerifiedComplete",
      requirements_verified: 3,
      requirements_total: 3,
      evidence_count: 3,
      certificate: null,
    }));
    expect(view.verifiedComplete).toBe(false);
  });

  it("pauses the one-click flow for a real user decision instead of offering Verify again", () => {
    const view = missionPresentation(
      execution({
        state: "ExecutionTasksFinishedAwaitingVerification",
        execution_phase: "FINISHED_AWAITING_VERIFICATION",
        finished_tasks: 3,
        runnable_tasks: [],
      }),
      verification({
        workflow_stage: "WAITING_FOR_USER_DECISION",
        summary: "Relintor needs your decision.",
        evidence_count: 2,
        human_decisions: [{
          requirement_id: "requirement-human",
          title: "Approved outcome",
          question: "Does this match the approved outcome?",
          summary: "Review the result.",
          criterion_ids: ["criterion-human"],
        }],
      }),
      true,
    );
    expect(view.primaryAction).toBe("none");
    expect(view.headline).toBe("Your decision is needed");
  });

  it("treats pre-evidence verification as waiting rather than failed", () => {
    const view = verificationPresentation(verification());
    expect(view.label).toBe("Waiting for evidence");
    expect(view.tone).toBe("info");
  });

  it("surfaces incomplete verification instead of reporting ready", () => {
    const view = verificationPresentation(verification({
      requirements_verified: 2,
      requirements_total: 11,
      evidence_count: 2,
      missing_evidence: ["requirement-human: missing HUMAN_DECISION"],
    }));
    expect(view.label).toBe("Verification needs attention");
    expect(view.tone).toBe("warning");
    expect(view.supporting).toMatch(/explicit human decision/i);
  });

  it("keeps Verify work as the P8 action after all tasks finish", () => {
    const view = missionPresentation(
      execution({
        state: "ExecutionTasksFinishedAwaitingVerification",
        execution_phase: "FINISHED_AWAITING_VERIFICATION",
        finished_tasks: 11,
        total_tasks: 11,
        runnable_tasks: [],
      }),
      verification({
        requirements_verified: 2,
        requirements_total: 11,
        evidence_count: 2,
        missing_evidence: ["requirement-human: missing HUMAN_DECISION"],
      }),
      true,
    );
    expect(view.primaryAction).toBe("verify");
    expect(view.headline).toBe("Verification needs attention");
  });

  it("keeps canonical Windows paths internally while removing the extended prefix for display", () => {
    expect(displayWindowsPath("\\\\?\\D:\\Projects\\Relintor Beta")).toBe("D:\\Projects\\Relintor Beta");
    expect(displayProjectName("\\\\?\\D:\\Projects\\Relintor Beta")).toBe("Relintor Beta");
  });
});
