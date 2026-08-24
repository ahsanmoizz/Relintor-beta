import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App, MissionCockpit, mergeVerificationRefresh, verificationActionError } from "./App";
import type { AntigravityHealth, ExecutionStatus, VerificationStatus } from "./backend";

function finishedExecution(): ExecutionStatus {
  return {
    project_id: "project-1",
    project_name: "Test project",
    mission_id: "mission-project-1",
    revision: 1,
    state: "ExecutionTasksFinishedAwaitingVerification",
    watchdog_state: "Healthy",
    current_turn: 1,
    active_task: null,
    current_task_objective: null,
    recovery_task_id: null,
    recovery_task_objective: null,
    runnable_tasks: [],
    total_tasks: 11,
    finished_tasks: 11,
    tool_calls: 0,
    execution_steps: 0,
    estimated_cost_micros: null,
    safe_boundary_reached: true,
    last_event: "TASK_IMPLEMENTATION_FINISHED",
    ledger_path: "D:\\Relintor\\mission.json",
    recovery_state: "NO_RECOVERY_REQUIRED",
    last_safe_checkpoint: null,
    resume_disposition: null,
    resume_blocker: null,
    external_changes: [],
    recovery_detected: false,
    recovery_action: "NONE",
    dispatch_active: false,
    execution_phase: "FINISHED_AWAITING_VERIFICATION",
    execution_time_limit_ms: 600_000,
    events: [],
  };
}

function pendingVerification(): VerificationStatus {
  return {
    project_id: "project-1",
    mission_id: "mission-project-1",
    revision: 1,
    execution_run_id: "run-1",
    state: "VERIFICATION_FINISHED",
    completion_state: "StoppedIncomplete",
    requirements_verified: 2,
    requirements_total: 11,
    missing_evidence: ["requirement-human: missing HUMAN_DECISION"],
    failed_checks: [],
    skipped_checks: [],
    stale_evidence: [],
    blocked_external: [],
    accepted_risks: [],
    evidence_count: 2,
    certificate: null,
    workflow_stage: "READY_TO_VERIFY",
    summary: "Verification is ready.",
    human_decisions: [],
    collector_activity: [],
    collection_failures: [],
    detail: "requirements remain unverified",
  };
}

const readyAntigravity: AntigravityHealth = {
  status: "ready",
  version: "1",
  executable: "antigravity.exe",
  compatibility: "compatible",
  cli_invocation_capability: true,
  plugin_hook_capability: true,
  ide_status: "installed",
  cli_status: "installed",
  authentication_status: "authenticated",
  executable_detectable: true,
  adapter_ready: true,
  setup_required: false,
  environment: "test",
  platform: "windows",
  detected_at_ms: 1,
  detail: "ready",
};

describe("desktop shell foundation", () => {
  beforeEach(() => {
    window.location.hash = "#home";
    localStorage.clear();
  });

  it("exposes exactly four primary destinations and a truthful new-project launcher", () => {
    render(<App />);

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    expect(navigation).toBeTruthy();

    const destinationNames = ["Home", "Projects", "Activity", "Account"];
    const destinations = destinationNames.map((name) =>
      screen.getByRole("button", { name }),
    );

    expect(destinations).toHaveLength(4);
    for (const destination of destinations) {
      expect(destination.tabIndex).not.toBe(-1);
    }

    expect(screen.getByRole("link", { name: "Skip to content" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /New project/ })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    expect(screen.getByRole("heading", { name: "Make the idea legible." })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "What are you building?" })).toBeTruthy();
  });

  it("changes the active view and exposes aria-current", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Projects" }));
    expect(screen.getByRole("heading", { name: "Continue a project." })).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Projects" }).getAttribute("aria-current"),
    ).toBe("page");
  });

  it("runs the browser-safe investigator preview without exposing provider configuration", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), {
      target: { value: "A private project tracker" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Important questions")).toBeTruthy();
    expect(screen.queryByText(/provider|api key|raw prompt/i)).toBeNull();
  });

  it("keeps browser preview outside sealing and verification", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), {
      target: { value: "A private project tracker" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Important questions")).toBeTruthy();
    expect(await screen.findByText("Choose a real project workspace before standards review.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Seal mission" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Verify work" })).toBeNull();
  });

  it("keeps takeover identity through the reality report and goal step", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /Take over existing project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Project folder" }), { target: { value: "C:\\Projects\\existing-app" } });
    fireEvent.click(screen.getByRole("button", { name: "Scan project" }));
    expect(await screen.findByText("REALITY REPORT")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Continue" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { name: "Set the next project goal." })).toBeTruthy();
    expect(screen.getByText("C:\\Projects\\existing-app")).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "What do you want Relintor to change?" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Back to reality report" })).toBeTruthy();
  });

  it("makes a selected investigation answer visually and semantically explicit", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), { target: { value: "A private project tracker" } });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Important questions")).toBeTruthy();
    const option = screen.getByRole("button", { name: "Single role" });
    fireEvent.click(option);
    await waitFor(() => expect(option.getAttribute("aria-pressed")).toBe("true"));
    expect(option.className).toContain("selected");
  });

  it("reports browser preview health as unavailable instead of checking forever", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(
      await screen.findByText("Browser preview has no Rust authority process."),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Browser preview cannot verify bundled specification resources.",
      ),
    ).toBeTruthy();
  });

  it("explains executor setup without exposing terminal instructions", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(await screen.findByRole("heading", { name: "Antigravity setup required" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Set up Antigravity" })).toBeTruthy();
    expect(await screen.findByText(/Antigravity setup runs only from the installed desktop app/)).toBeTruthy();
    expect(screen.getByText(/At least 5 GB/)).toBeTruthy();
    expect(screen.queryByText(/PowerShell|manual CLI/i)).toBeNull();
  });

  it("persists an explicit theme selection with aria-pressed state", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    const dark = screen.getByRole("button", { name: "Dark" });

    fireEvent.click(dark);

    expect(localStorage.getItem("relintor-theme")).toBe("dark");
    expect(dark.getAttribute("aria-pressed")).toBe("true");
  });

  it("shows a truthful signed-out account and unavailable entitlement state", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(await screen.findByRole("heading", { name: "Signed out" })).toBeTruthy();
    expect(screen.getByText("Not confirmed")).toBeTruthy();
    expect(screen.getByText(/Local project work remains available/)).toBeTruthy();
  });

  it("explains the no-terminal first-run path and remembers dismissal", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "A calm path from idea to proof." })).toBeTruthy();
    expect(screen.getByText(/No terminal or documentation reading is required/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue to workspace" }));
    expect(screen.queryByRole("heading", { name: "A calm path from idea to proof." })).toBeNull();
    expect(localStorage.getItem("relintor-onboarding-seen")).toBe("true");
  });

  it("shows investigator risks and the authority sealing boundary", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /New project/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What are you building?" }), { target: { value: "A private project tracker" } });
    fireEvent.click(screen.getByRole("button", { name: "Investigate idea" }));
    expect(await screen.findByText("Risks")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "What the authority will lock" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Continue to standards review" })).toBeTruthy();
  });

  it("keeps an empty activity view explicit instead of inventing events", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    expect(screen.getByTestId("activity-empty")).toBeTruthy();
    expect(screen.getByText(/There is no frontend-only activity/)).toBeTruthy();
  });

  it("routes Verify work through the P8 action callback after all tasks finish", () => {
    const onVerify = vi.fn();
    const rendered = render(
      <MissionCockpit
        status={finishedExecution()}
        verification={pendingVerification()}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={onVerify}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Verify work" }));
    expect(onVerify).toHaveBeenCalledTimes(1);
  });

  it("turns a failed verification invocation into visible user-facing text", () => {
    expect(verificationActionError(new Error("verification authority command failed"))).toMatch(/couldn't verify this work/i);
  });

  it("captures a genuine human decision in plain language without exposing IDs in primary copy", () => {
    const onDecision = vi.fn();
    const decision = pendingVerification();
    decision.workflow_stage = "WAITING_FOR_USER_DECISION";
    decision.summary = "Automated checks are complete. Relintor needs your decision before verification can continue.";
    decision.human_decisions = [{
      requirement_id: "requirement-secret-internal-id",
      title: "Approved outcome",
      question: "Does the completed result match the outcome you approved for this mission?",
      summary: "Review the completed project outcome before approving it.",
      criterion_ids: ["criterion-secret-internal-id"],
    }];
    const rendered = render(
      <MissionCockpit
        status={finishedExecution()}
        verification={decision}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={onDecision}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Relintor needs your decision" })).toBeTruthy();
    expect(screen.getByText("Technical evidence details")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Optional notes"), { target: { value: "The result matches." } });
    const staleRefresh: VerificationStatus = {
      ...decision,
      workflow_stage: "READY_TO_VERIFY",
      summary: "Verification is ready.",
      human_decisions: [],
      missing_evidence: ["requirement-secret-internal-id: missing HUMAN_DECISION"],
    };
    rendered.rerender(
      <MissionCockpit
        status={finishedExecution()}
        verification={mergeVerificationRefresh(decision, staleRefresh)}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={onDecision}
        onRefresh={vi.fn()}
      />,
    );
    expect((screen.getByLabelText("Optional notes") as HTMLTextAreaElement).value).toBe("The result matches.");
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(onDecision).toHaveBeenCalledWith("requirement-secret-internal-id", true, "The result matches.");
    expect(screen.queryByRole("button", { name: "Verify work" })).toBeNull();
  });

  it("keeps the pending decision stable across multiple five-second polling refreshes", async () => {
    vi.useFakeTimers();
    try {
      const pending = pendingVerification();
      pending.workflow_stage = "WAITING_FOR_USER_DECISION";
      pending.summary = "Automated checks are complete. Relintor needs your decision before verification can continue.";
      pending.human_decisions = [{
        requirement_id: "requirement-human",
        title: "Approved outcome",
        question: "Does the completed result satisfy the approved outcome for this mission?",
        summary: "Review the completed project outcome before approving it.",
        criterion_ids: ["criterion-human"],
      }];
      const staleRefresh: VerificationStatus = {
        ...pending,
        workflow_stage: "READY_TO_VERIFY",
        summary: "Verification is ready.",
        human_decisions: [],
        missing_evidence: ["requirement-human: missing HumanDecision"],
      };
      let current = pending;
      const notes = "My decision remains attached to this mission.";
      const timer = window.setInterval(() => {
        current = mergeVerificationRefresh(current, staleRefresh);
      }, 5_000);

      await vi.advanceTimersByTimeAsync(15_000);

      expect(current.workflow_stage).toBe("WAITING_FOR_USER_DECISION");
      expect(current.human_decisions[0]?.requirement_id).toBe("requirement-human");
      expect(notes).toBe("My decision remains attached to this mission.");
      window.clearInterval(timer);
    } finally {
      vi.useRealTimers();
    }
  });

  it("allows an authoritative decision transition to replace the pending card", () => {
    const pending = pendingVerification();
    pending.workflow_stage = "WAITING_FOR_USER_DECISION";
    pending.human_decisions = [{
      requirement_id: "requirement-human",
      title: "Approved outcome",
      question: "Does the completed result satisfy the approved outcome for this mission?",
      summary: "Review the completed project outcome before approving it.",
      criterion_ids: ["criterion-human"],
    }];
    const rejected: VerificationStatus = {
      ...pending,
      workflow_stage: "USER_DECISION_REJECTED",
      human_decisions: [],
      summary: "You rejected the completed result.",
      missing_evidence: [],
    };

    expect(mergeVerificationRefresh(pending, rejected).workflow_stage).toBe("USER_DECISION_REJECTED");
    expect(mergeVerificationRefresh(pending, rejected).human_decisions).toHaveLength(0);
  });

  it("exposes privacy controls and safe diagnostics without sensitive fields", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    expect(await screen.findByRole("heading", { name: "Choose what leaves this device." })).toBeTruthy();
    expect(screen.getByLabelText(/Keep project evidence local/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Export safe diagnostics" })).toBeTruthy();
    expect(screen.queryByText(/database_path/i)).toBeNull();
    fireEvent.click(screen.getByLabelText(/Allow cloud account\/team state/));
    await waitFor(() => expect(localStorage.getItem("relintor-privacy-cloud")).toBe("true"));
  });
});
