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

  it("surfaces incomplete verification with human decision when eligible", () => {
    const view = verificationPresentation(verification({
      requirements_verified: 2,
      requirements_total: 11,
      evidence_count: 2,
      final_human_acceptance_eligible: true,
      missing_evidence: ["requirement-human: missing HUMAN_DECISION"],
    }));
    expect(view.label).toBe("Verification needs attention");
    expect(view.tone).toBe("warning");
    expect(view.supporting).toMatch(/explicit human decision/i);
  });

  it("never prompts for human decision while machine-verifiable obligations remain incomplete", () => {
    const view = verificationPresentation(verification({
      requirements_verified: 2,
      requirements_total: 11,
      evidence_count: 2,
      final_human_acceptance_eligible: false,
      missing_evidence: ["requirement-human: missing HUMAN_DECISION", "requirement-a11y: missing ACCESSIBILITY_RESULT"],
    }));
    expect(view.label).toBe("Verification needs attention");
    expect(view.tone).toBe("warning");
    expect(view.supporting).not.toMatch(/explicit human decision/i);
    expect(view.supporting).toMatch(/missing evidence|dependency issues/i);
  });

  it("routes to view_verification rather than exposing a manual verify CTA after all tasks finish", () => {
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
    expect(view.primaryAction).not.toBe("verify");
    expect(view.primaryAction).toBe("view_verification");
    expect(view.headline).toBe("Verification needs attention");
  });

  it("renders Verified Complete when valid certificate is present despite historical stale evidence", () => {
    const view = verificationPresentation(verification({
      completion_state: "VerifiedComplete",
      requirements_verified: 6,
      requirements_total: 6,
      evidence_count: 6,
      stale_evidence: ["p8-collector-old-blocked-evidence"],
      certificate: {
        certificate_id: "cert-test-id",
        final_state: "VerifiedComplete",
        digest: "cert-digest",
      },
    }));
    expect(view.verifiedComplete).toBe(true);
    expect(view.label).toBe("Verified Complete");
    expect(view.tone).toBe("success");
  });

  it("presents the exact preserved mission state as Verified Complete with primaryAction view_verification", () => {
    const cert = {
      certificate_id: "cert-21166a95cb5dea453f739ab10fa946bb0b16d10cf0e6f2657c28e13c3f69ad99",
      final_state: "VERIFIED_COMPLETE",
      digest: "certificate-digest",
    };
    const ver = verification({
      project_id: "takeover-project-takeover_b40c6248d6502fccdd411472",
      mission_id: "mission-takeover-project-takeover_b40c6248d6502fccdd411472",
      completion_state: "VerifiedComplete",
      workflow_stage: "VERIFIED_COMPLETE",
      summary: "All required evidence passed and the completion certificate is valid.",
      requirements_verified: 6,
      requirements_total: 6,
      missing_evidence: [],
      failed_checks: [],
      skipped_checks: [],
      stale_evidence: [
        "p8-collector-requirement_7556731cb7688c60a0b96fa1-criterion_82177a1c52dc868514fb7e3d-84cb368d1037d2d1-blocked-778a3152bc99bb61",
        "p8-collector-requirement_81ed3fe05bc59f2050683fee-criterion-project-blueprint-candidate-requirement_38644ce6fdb1b61413ad8c75-0391f0a6654eb134-blocked-778a3152bc99bb61",
      ],
      blocked_external: [],
      accepted_risks: [],
      evidence_count: 6,
      certificate: cert,
      human_decisions: [],
    });
    const exec = execution({
      project_id: "takeover-project-takeover_b40c6248d6502fccdd411472",
      mission_id: "mission-takeover-project-takeover_b40c6248d6502fccdd411472",
      state: "ExecutionTasksFinishedAwaitingVerification",
      execution_phase: "FINISHED_AWAITING_VERIFICATION",
      finished_tasks: 0,
      total_tasks: 0,
      runnable_tasks: [],
    });
    const m = missionPresentation(exec, ver, true);
    const v = verificationPresentation(ver);
    expect(v.verifiedComplete).toBe(true);
    expect(v.label).toBe("Verified Complete");
    expect(v.tone).toBe("success");
    expect(m.headline).toBe("Verified Complete");
    expect(m.badge).toBe("Verified");
    expect(m.tone).toBe("success");
    expect(m.primaryAction).toBe("view_verification");
    expect(m.verifiedComplete).toBe(true);
  });

  it("keeps canonical Windows paths internally while removing the extended prefix for display", () => {
    expect(displayWindowsPath("\\\\?\\D:\\Projects\\Relintor Beta")).toBe("D:\\Projects\\Relintor Beta");
    expect(displayProjectName("\\\\?\\D:\\Projects\\Relintor Beta")).toBe("Relintor Beta");
  });

  describe("UI/Backend Contradiction Matrix", () => {
    it("never renders Verified Complete alongside Verification needs attention", () => {
      const v = verificationPresentation(verification({
        completion_state: "VerifiedComplete",
        requirements_verified: 6,
        requirements_total: 6,
        evidence_count: 6,
        certificate: { certificate_id: "c1", final_state: "VerifiedComplete", digest: "d1" },
        missing_evidence: [],
        failed_checks: [],
      }));
      expect(v.verifiedComplete).toBe(true);
      expect(v.label).not.toContain("needs attention");
      expect(v.label).toBe("Verified Complete");

      const vContradiction = verificationPresentation(verification({
        completion_state: "VerifiedComplete",
        requirements_verified: 5,
        requirements_total: 6,
        evidence_count: 6,
        certificate: { certificate_id: "c1", final_state: "VerifiedComplete", digest: "d1" },
        missing_evidence: ["req_6: missing evidence"],
      }));
      expect(vContradiction.verifiedComplete).toBe(false);
      expect(vContradiction.label).toBe("Verification needs attention");
    });

    it("never enables Verify work when mission is already Verified Complete", () => {
      const m = missionPresentation(
        execution({ state: "ExecutionTasksFinishedAwaitingVerification", execution_phase: "FINISHED_AWAITING_VERIFICATION" }),
        verification({
          completion_state: "VerifiedComplete",
          requirements_verified: 6,
          requirements_total: 6,
          evidence_count: 6,
          certificate: { certificate_id: "c1", final_state: "VerifiedComplete", digest: "d1" },
        }),
        true,
      );
      expect(m.primaryAction).toBe("view_verification");
      expect(m.primaryAction).not.toBe("verify");
      expect(m.verifiedComplete).toBe(true);
    });

    it("never enables Verify work while waiting for a human decision", () => {
      const m = missionPresentation(
        execution({ state: "ExecutionTasksFinishedAwaitingVerification", execution_phase: "FINISHED_AWAITING_VERIFICATION" }),
        verification({
          workflow_stage: "WAITING_FOR_USER_DECISION",
          summary: "Human decision required.",
          human_decisions: [{
            requirement_id: "req_human",
            title: "Decision Title",
            question: "Is this correct?",
            summary: "Decision summary",
            criterion_ids: ["crit_human"],
          }],
        }),
        true,
      );
      expect(m.primaryAction).toBe("none");
      expect(m.headline).toBe("Your decision is needed");
    });

    it("never enables Verify work while correction is actively running", () => {
      const m = missionPresentation(
        execution({ state: "Running", execution_phase: "RUNNING", dispatch_active: true }),
        verification({
          workflow_stage: "CORRECTING_FAILED_REQUIREMENT",
          summary: "Correcting failed requirement",
        }),
        true,
      );
      expect(m.primaryAction).toBe("none");
      expect(m.badge).toBe("Running");
    });

    it("requires recovery review on terminal failure rather than generic blind retry", () => {
      const m = missionPresentation(
        execution({
          state: "RevalidationRequired",
          recovery_state: "REVALIDATION_REQUIRED",
          recovery_detected: true,
        }),
        verification(),
        true,
      );
      expect(m.primaryAction).toBe("recover");
      expect(m.recoveryRequired).toBe(true);
    });

    it("never shows Verified Complete when 0 tasks are runnable but requirements are unverified", () => {
      const m = missionPresentation(
        execution({
          state: "ExecutionTasksFinishedAwaitingVerification",
          execution_phase: "FINISHED_AWAITING_VERIFICATION",
          finished_tasks: 0,
          total_tasks: 0,
          runnable_tasks: [],
        }),
        verification({
          completion_state: "StoppedIncomplete",
          requirements_verified: 0,
          requirements_total: 10,
          evidence_count: 0,
          certificate: null,
        }),
        true,
      );
      expect(m.verifiedComplete).toBe(false);
      expect(m.headline).not.toBe("Verified Complete");
      expect(m.badge).not.toBe("Verified");
    });

    it("shows truthful verification state and not completion certificate when 10/10 tasks complete but requirements unverified", () => {
      const m = missionPresentation(
        execution({
          state: "ExecutionTasksFinishedAwaitingVerification",
          execution_phase: "FINISHED_AWAITING_VERIFICATION",
          finished_tasks: 10,
          total_tasks: 10,
          runnable_tasks: [],
        }),
        verification({
          completion_state: "StoppedIncomplete",
          requirements_verified: 9,
          requirements_total: 10,
          evidence_count: 9,
          missing_evidence: ["requirement_10: missing AccessibilityResult"],
          certificate: null,
        }),
        true,
      );
      expect(m.verifiedComplete).toBe(false);
      expect(m.primaryAction).toBe("view_verification");
      expect(m.headline).toBe("Verification needs attention");
      expect(m.badge).not.toBe("Verified");
    });

    it("presents Correction required and disables actions when human decision was rejected", () => {
      const ver = verification({
        workflow_stage: "USER_DECISION_REJECTED",
        summary: "You rejected the completed result.",
        human_decisions: [],
      });
      const exec = execution({
        state: "ExecutionTasksFinishedAwaitingVerification",
        execution_phase: "FINISHED_AWAITING_VERIFICATION",
      });
      const v = verificationPresentation(ver);
      expect(v.label).toBe("Correction required");
      expect(v.verifiedComplete).toBe(false);

      const m = missionPresentation(exec, ver, true);
      expect(m.headline).toBe("Correction required");
      expect(m.badge).toBe("Needs attention");
      expect(m.primaryAction).toBe("review_correction");
      expect(m.verifiedComplete).toBe(false);
    });

    it("forbids Run next task and Check recovery safety between healthy tasks (Defect #61)", () => {
      const exec = execution({
        finished_tasks: 14,
        total_tasks: 15,
        state: "Ready",
        execution_phase: "READY",
        recovery_state: "NO_RECOVERY_REQUIRED",
        recovery_action: "NONE",
        recovery_detected: false,
      });
      const m = missionPresentation(exec, null, true);
      expect(m.badge).toBe("Continuing");
      expect(m.headline).toBe("Starting Antigravity");
      expect(m.supporting).toBe("Relintor is continuing work under sealed authority.");
      expect(m.primaryAction).toBe("none");
      expect(m.recoveryRequired).toBe(false);
    });

    it("does not demand recovery review when pre-execution retry is authorized (Defect #61)", () => {
      const exec = execution({
        finished_tasks: 14,
        total_tasks: 15,
        state: "Ready",
        execution_phase: "READY",
        recovery_state: "PreExecutionRetryAuthorized",
        recovery_action: "NONE",
        recovery_detected: false,
      });
      const m = missionPresentation(exec, null, true);
      expect(m.recoveryRequired).toBe(false);
      expect(m.primaryAction).toBe("none");
      expect(m.badge).toBe("Continuing");
    });

    it("displays reviewed retry ready when recovery safety check passed (Defect #61)", () => {
      const exec = execution({
        finished_tasks: 14,
        total_tasks: 15,
        state: "RevalidationRequired",
        execution_phase: "REVALIDATION_REQUIRED",
        recovery_state: "REVALIDATION_REQUIRED",
        recovery_action: "MANUAL_REVIEW_RETRY",
        recovery_detected: true,
      });
      const m = missionPresentation(exec, null, true);
      expect(m.recoveryRequired).toBe(true);
      expect(m.primaryAction).toBe("recover");
      expect(m.headline).toBe("Recovery reviewed — ready to retry");
      expect(m.supporting).toContain("Review changes and retry to continue");
    });
  });
});
