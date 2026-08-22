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

  it("treats pre-evidence verification as waiting rather than failed", () => {
    const view = verificationPresentation(verification());
    expect(view.label).toBe("Waiting for evidence");
    expect(view.tone).toBe("info");
  });

  it("keeps canonical Windows paths internally while removing the extended prefix for display", () => {
    expect(displayWindowsPath("\\\\?\\D:\\Projects\\Relintor Beta")).toBe("D:\\Projects\\Relintor Beta");
    expect(displayProjectName("\\\\?\\D:\\Projects\\Relintor Beta")).toBe("Relintor Beta");
  });
});
