import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ExecutionStatus } from "./backend";

const mocks = vi.hoisted(() => ({
  readExecutionStatus: vi.fn(),
  readAntigravityHealth: vi.fn(),
  revalidateExecution: vi.fn(),
  verificationStatus: vi.fn(),
}));

vi.mock("./backend", async () => {
  const actual = await vi.importActual<typeof import("./backend")>("./backend");
  return {
    ...actual,
    readExecutionStatus: mocks.readExecutionStatus,
    readAntigravityHealth: mocks.readAntigravityHealth,
    revalidateExecution: mocks.revalidateExecution,
    verificationStatus: mocks.verificationStatus,
  };
});

import { Activity } from "./App";

describe("existing mission authority recovery", () => {
  beforeEach(() => {
    mocks.readExecutionStatus.mockReset();
    mocks.readExecutionStatus.mockRejectedValue(
      "AUTHORITY_READ_LEGACY_EXECUTION_STATE: legacy execution record needs a restart",
    );
    mocks.readAntigravityHealth.mockReset();
    mocks.revalidateExecution.mockReset();
    mocks.verificationStatus.mockReset();
    mocks.verificationStatus.mockRejectedValue("Execution evidence is not ready");
    mocks.readAntigravityHealth.mockResolvedValue({
      status: "healthy",
      version: "1.0.0",
      executable: "agy",
      compatibility: "compatible",
      cli_invocation_capability: true,
      plugin_hook_capability: true,
      ide_status: "ready",
      cli_status: "ready",
      authentication_status: "ready",
      executable_detectable: true,
      adapter_ready: true,
      setup_required: false,
      environment: "test",
      platform: "windows-x64",
      detected_at_ms: 1,
      detail: "ready",
    });
  });

  it("makes a repeated authority read visibly complete instead of silently no-oping", async () => {
    render(
      <Activity
        handoff={{
          mission_id: "mission-takeover-project-takeover_legacy",
          revision: 1,
          contract_hash: "seal",
          state: "READY_FOR_EXECUTION",
          task_order: [],
          scheduler_owner: "P7_RUST_SCHEDULER",
        }}
      />,
    );

    expect(
      await screen.findByText("Authority read attempt 1 completed without a usable mission snapshot."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry authority read" }));
    expect(
      await screen.findByText("Authority read attempt 2 completed without a usable mission snapshot."),
    ).toBeTruthy();
    expect(mocks.readExecutionStatus).toHaveBeenCalledTimes(2);
  });

  it("shows restart interruption recovery and a reviewed retry action", async () => {
    const status: ExecutionStatus = {
      project_id: "project-recovery",
      project_name: "Recovery project",
      mission_id: "mission-project-recovery",
      revision: 1,
      state: "RevalidationRequired",
      watchdog_state: "BudgetExhausted",
      current_turn: 1,
      active_task: null,
      current_task_objective: "Projected next task that is not the retry target",
      recovery_task_id: "task-recovery",
      recovery_task_objective: "Repair the interrupted task",
      runnable_tasks: ["task-next"],
      total_tasks: 2,
      finished_tasks: 0,
      tool_calls: 1,
      execution_steps: 1,
      estimated_cost_micros: null,
      safe_boundary_reached: false,
      last_event: "adapter completion rejected after lease or wall-clock budget boundary",
      ledger_path: "D:/execution/mission.json",
      recovery_state: "RevalidationRequired",
      last_safe_checkpoint: null,
      resume_disposition: "RevalidationRequired",
      resume_blocker: "checkpoint records durable state but not a safe automatic-resume boundary",
      external_changes: [],
      recovery_detected: true,
      recovery_action: "CHECK_SAFETY",
      dispatch_active: false,
      execution_phase: "READY",
      execution_time_limit_ms: 300_000,
      events: [{
        sequence: 16,
        occurred_at_ms: 1,
        task_id: null,
        kind: "BudgetWarning",
        detail: "adapter completion rejected after lease or wall-clock budget boundary",
      }],
    };
    mocks.readExecutionStatus.mockResolvedValue(status);
    mocks.readAntigravityHealth.mockResolvedValue({
      adapter_ready: false,
      setup_required: true,
      detail: "Antigravity setup required",
      ide_status: "unknown",
      cli_status: "missing",
      authentication_status: "unknown",
      executable_detectable: false,
    });
    let resolveRevalidation!: (value: typeof status) => void;
    mocks.revalidateExecution.mockReturnValue(new Promise((resolve) => {
      resolveRevalidation = resolve as (value: typeof status) => void;
    }));

    render(<Activity handoff={{ mission_id: status.mission_id, revision: 1, contract_hash: "seal", state: "READY_FOR_EXECUTION", task_order: [], scheduler_owner: "P7_RUST_SCHEDULER" }} />);
    expect((await screen.findAllByText("A fresh safety check is required before this mission can continue.")).length).toBeGreaterThan(0);
    expect(screen.getByRole("heading", { name: "Repair the interrupted task" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Run next task" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Check recovery safety" }));
    expect(screen.getAllByText("Checking recovery safety…").length).toBeGreaterThan(0);
    resolveRevalidation({
      ...status,
      resume_blocker: "an external process started for the selected attempt; Relintor cannot prove that it left no workspace or external side effects",
      recovery_action: "MANUAL_REVIEW_RETRY",
      events: [...status.events, {
        sequence: 17,
        occurred_at_ms: 2,
        task_id: "task_42b0139b3f6440d115950a8d",
        kind: "AuthorityRevalidationRequired",
        detail: "recovery decision RevalidationRequired for attempt-timeout",
      }],
    });
    await waitFor(() => expect(screen.getAllByText("The previous Antigravity run stopped during restart recovery. Relintor found workspace changes but no trusted completion record; review them below before retrying this task.").length).toBeGreaterThan(0));
    expect(screen.getByRole("button", { name: "Review changes and retry this task" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Run next task" })).toBeNull();
    expect(mocks.revalidateExecution).toHaveBeenCalledTimes(1);
  }, 15_000);

  it("does not present a fresh run as an unsafe retry", async () => {
    const status: ExecutionStatus = {
      project_id: "project-fresh",
      project_name: "Fresh project",
      mission_id: "mission-project-fresh",
      revision: 1,
      state: "Ready",
      watchdog_state: "Healthy",
      current_turn: 1,
      active_task: null,
      current_task_objective: "Create the first verified feature",
      recovery_task_id: null,
      recovery_task_objective: null,
      runnable_tasks: ["task-next"],
      total_tasks: 1,
      finished_tasks: 0,
      tool_calls: 0,
      execution_steps: 0,
      estimated_cost_micros: null,
      safe_boundary_reached: false,
      last_event: "P6 handoff validated",
      ledger_path: "D:/execution/mission-fresh.json",
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
      events: [{
        sequence: 1,
        occurred_at_ms: 1,
        task_id: null,
        kind: "RunStarted",
        detail: "P6 handoff validated",
      }],
    };
    mocks.readExecutionStatus.mockResolvedValue(status);

    render(<Activity handoff={{ mission_id: status.mission_id, revision: 1, contract_hash: "seal", state: "READY_FOR_EXECUTION", task_order: ["task-next"], scheduler_owner: "P7_RUST_SCHEDULER" }} />);

    expect(await screen.findByRole("heading", { name: "Ready to run" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Create the first verified feature" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: /recovery/i })).toBeNull();
    expect(screen.queryByText(/cannot prove that the previous attempt was safe/i)).toBeNull();
    expect(screen.getByRole("button", { name: "Run next task" }).hasAttribute("disabled")).toBe(false);
  });

  it("announces a running task and removes every second-dispatch action", async () => {
    const status: ExecutionStatus = {
      project_id: "project-running",
      project_name: "Running project",
      mission_id: "mission-project-running",
      revision: 1,
      state: "Running",
      watchdog_state: "Healthy",
      current_turn: 1,
      active_task: "task-running",
      current_task_objective: "Prepare accessible interaction requirements",
      recovery_task_id: null,
      recovery_task_objective: null,
      runnable_tasks: [],
      total_tasks: 3,
      finished_tasks: 0,
      tool_calls: 1,
      execution_steps: 1,
      estimated_cost_micros: null,
      safe_boundary_reached: false,
      last_event: "task attempt started",
      ledger_path: "D:\\execution\\mission-running.json",
      recovery_state: "NO_RECOVERY_REQUIRED",
      last_safe_checkpoint: null,
      resume_disposition: null,
      resume_blocker: null,
      external_changes: [],
      recovery_detected: false,
      recovery_action: "NONE",
      dispatch_active: true,
      execution_phase: "RUNNING",
      execution_time_limit_ms: 300_000,
      events: [],
    };
    mocks.readExecutionStatus.mockResolvedValue(status);

    render(<Activity handoff={{ mission_id: status.mission_id, revision: 1, contract_hash: "seal", state: "READY_FOR_EXECUTION", task_order: ["task-running"], scheduler_owner: "P7_RUST_SCHEDULER" }} />);

    const heading = await screen.findByRole("heading", { name: "Antigravity is working" });
    expect(heading.closest('[role="status"]')?.getAttribute("aria-live")).toBe("polite");
    expect(screen.queryByRole("button", { name: "Run next task" })).toBeNull();
    expect(screen.getByRole("button", { name: "Stop safely" })).toBeTruthy();
  });
});
