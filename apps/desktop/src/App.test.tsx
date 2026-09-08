import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { Activity, App, MissionCockpit, mergeVerificationRefresh, verificationActionError } from "./App";
import type { AntigravityHealth, ExecutionStatus, VerificationStatus } from "./backend";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const tauriInvoke = vi.mocked(invoke);

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
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    tauriInvoke.mockReset();
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
  }, 15_000);

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

  it("drives the native approval command through Tauri and renders the returned certificate state", async () => {
    const pending = pendingVerification();
    pending.workflow_stage = "WAITING_FOR_USER_DECISION";
    pending.human_decisions = [{
      requirement_id: "requirement-human",
      title: "Approved outcome",
      question: "Does the completed result satisfy the approved outcome for this mission?",
      summary: "Review the completed project outcome before approving it.",
      criterion_ids: ["criterion-human"],
    }];
    const completed: VerificationStatus = {
      ...pending,
      completion_state: "VerifiedComplete",
      workflow_stage: "VERIFIED_COMPLETE",
      summary: "All required evidence passed and the completion certificate is valid.",
      missing_evidence: [],
      requirements_verified: 11,
      requirements_total: 11,
      evidence_count: 11,
      human_decisions: [],
      certificate: {
        certificate_id: "cert-native-closure",
        final_state: "VerifiedComplete",
        digest: "certificate-digest",
      },
    };
    Reflect.defineProperty(window, "__TAURI_INTERNALS__", { value: {} });
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "execution_status") return finishedExecution();
      if (command === "health_antigravity") return readyAntigravity;
      if (command === "antigravity_setup_status") return { active: false, adapter_ready: true };
      if (command === "verification_status") return pending;
      if (command === "verification_submit_human_decision") {
        expect(args).toEqual({
          projectId: "project-1",
          requirementId: "requirement-human",
          approved: true,
          notes: "The result satisfies the approved outcome.",
        });
        return completed;
      }
      throw new Error(`unexpected Tauri command: ${command}`);
    });

    render(<Activity handoff={{
      mission_id: "mission-project-1",
      revision: 1,
      contract_hash: "contract-hash",
      state: "SEALED",
      task_order: [],
      scheduler_owner: "relintor",
    }} />);

    expect(await screen.findByRole("heading", { name: "Relintor needs your decision" })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Optional notes"), {
      target: { value: "The result satisfies the approved outcome." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(screen.getByText("Verification finished successfully. The completion certificate is valid and saved.")).toBeTruthy());
    expect(screen.getAllByText("Verified Complete").length).toBeGreaterThan(0);
    expect(tauriInvoke).toHaveBeenCalledWith("verification_submit_human_decision", {
      projectId: "project-1",
      requirementId: "requirement-human",
      approved: true,
      notes: "The result satisfies the approved outcome.",
    });
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
    expect(screen.getByText(/Technical Evidence/i)).toBeTruthy();
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

  it("keeps the pending decision and notes stable across six five-second polling refreshes", async () => {
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
      const onDecision = vi.fn();
      let current = pending;
      const rendered = render(
        <MissionCockpit
          status={finishedExecution()}
          verification={current}
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
      fireEvent.change(screen.getByLabelText("Optional notes"), {
        target: { value: "My decision remains attached to this mission." },
      });
      const timer = window.setInterval(() => {
        current = mergeVerificationRefresh(current, staleRefresh);
        rendered.rerender(
          <MissionCockpit
            status={finishedExecution()}
            verification={current}
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
      }, 5_000);

      await act(async () => vi.advanceTimersByTimeAsync(30_000));

      expect(current.workflow_stage).toBe("WAITING_FOR_USER_DECISION");
      expect(current.human_decisions[0]?.requirement_id).toBe("requirement-human");
      expect(screen.getByRole("heading", { name: "Relintor needs your decision" })).toBeTruthy();
      expect((screen.getByLabelText("Optional notes") as HTMLTextAreaElement).value)
        .toBe("My decision remains attached to this mission.");
      window.clearInterval(timer);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps a pending decision when status polling returns the same prompts with READY_TO_VERIFY", () => {
    const pending = pendingVerification();
    pending.workflow_stage = "WAITING_FOR_USER_DECISION";
    pending.human_decisions = [{
      requirement_id: "requirement-human",
      title: "Approved outcome",
      question: "Does the completed result satisfy the approved outcome for this mission?",
      summary: "Review the completed project outcome before approving it.",
      criterion_ids: ["criterion-human"],
    }];
    pending.missing_evidence = ["requirement-human: missing HumanDecision"];
    const polled = {
      ...pending,
      workflow_stage: "READY_TO_VERIFY",
      summary: "Verification is ready.",
    };

    const merged = mergeVerificationRefresh(pending, polled);

    expect(merged.workflow_stage).toBe("WAITING_FOR_USER_DECISION");
    expect(merged.human_decisions).toEqual(pending.human_decisions);
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

  it("submits the exact rejection and retains the card and notes while persistence is pending or fails", () => {
    const onDecision = vi.fn();
    const pending = pendingVerification();
    pending.workflow_stage = "WAITING_FOR_USER_DECISION";
    pending.human_decisions = [{
      requirement_id: "requirement-human",
      title: "Approved outcome",
      question: "Does the completed result satisfy the approved outcome for this mission?",
      summary: "Review the completed project outcome before approving it.",
      criterion_ids: ["criterion-human"],
    }];
    const props = {
      status: finishedExecution(),
      verification: pending,
      antigravity: readyAntigravity,
      revalidating: false,
      onCommand: vi.fn(),
      onVerify: vi.fn(),
      onDecision,
      onRefresh: vi.fn(),
    };
    const rendered = render(
      <MissionCockpit {...props} busy={false} verificationNotice={null} />,
    );
    fireEvent.change(screen.getByLabelText("Optional notes"), {
      target: { value: "The result does not meet the approved outcome." },
    });

    rendered.rerender(
      <MissionCockpit {...props} busy verificationNotice="Recording your rejection…" />,
    );
    expect(screen.getByRole("heading", { name: "Relintor needs your decision" })).toBeTruthy();
    expect((screen.getByLabelText("Optional notes") as HTMLTextAreaElement).value)
      .toBe("The result does not meet the approved outcome.");
    expect((screen.getAllByRole("button", { name: "Recording decision…" })[0] as HTMLButtonElement).disabled)
      .toBe(true);
    fireEvent.click(screen.getAllByRole("button", { name: "Recording decision…" })[0]);
    expect(onDecision).not.toHaveBeenCalled();

    rendered.rerender(
      <MissionCockpit {...props} busy={false} verificationNotice={null} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Reject" }));
    expect(onDecision).toHaveBeenCalledWith(
      "requirement-human",
      false,
      "The result does not meet the approved outcome.",
    );
  });

  it("removes the decision card only after an authoritative successful submission response", () => {
    const pending = pendingVerification();
    pending.workflow_stage = "WAITING_FOR_USER_DECISION";
    pending.human_decisions = [{
      requirement_id: "requirement-human",
      title: "Approved outcome",
      question: "Does the completed result satisfy the approved outcome for this mission?",
      summary: "Review the completed project outcome before approving it.",
      criterion_ids: ["criterion-human"],
    }];
    const rendered = render(
      <MissionCockpit
        status={finishedExecution()}
        verification={pending}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );
    const resumed = {
      ...pending,
      workflow_stage: "VERIFICATION_NEEDS_ATTENTION",
      human_decisions: [],
      missing_evidence: ["requirement-test: missing TestOutput"],
    };
    rendered.rerender(
      <MissionCockpit
        status={finishedExecution()}
        verification={resumed}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice="Verification finished."
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.queryByRole("heading", { name: "Relintor needs your decision" })).toBeNull();
  });

  it("renders correction scope preview and calls onAuthorizeCorrection when user clicks Authorize scoped correction", () => {
    const onAuthorize = vi.fn();
    const rejectedVer = pendingVerification();
    rejectedVer.workflow_stage = "USER_DECISION_REJECTED";
    rejectedVer.summary = "You rejected the completed result.";
    rejectedVer.correction_scope = {
      mission_id: "mission-test",
      revision: 1,
      originating_evidence_id: "p8-rejection-art",
      user_rejection_notes: "Needs explicit retry backoff test",
      failed_requirement_ids: ["REQ-A-OUTCOME"],
      blocked_requirement_ids: ["REQ-ACCESSIBILITY"],
      affected_task_ids: ["task-1"],
      preserved_task_ids: ["task-2", "task-3"],
      scope_hash: "test-scope-hash-123",
      authorized: false,
    };

    const rendered = render(
      <MissionCockpit
        status={finishedExecution()}
        verification={rejectedVer}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
        onAuthorizeCorrection={onAuthorize}
      />,
    );

    expect(screen.getByRole("heading", { name: /Review Correction Scope/i })).toBeTruthy();
    expect(screen.getByText(/Needs explicit retry backoff test/i)).toBeTruthy();
    expect(screen.getByText(/REQ-A-OUTCOME/i)).toBeTruthy();
    expect(screen.getByText(/REQ-ACCESSIBILITY/i)).toBeTruthy();
    expect(screen.getByText(/2 historical tasks/i)).toBeTruthy();

    const authBtn = screen.getByRole("button", { name: "Authorize scoped correction" });
    expect(authBtn).toBeTruthy();
    fireEvent.click(authBtn);
    expect(onAuthorize).toHaveBeenCalledWith("test-scope-hash-123");

    // Re-render as authorized
    const authorizedVer = {
      ...rejectedVer,
      correction_scope: {
        ...rejectedVer.correction_scope,
        authorized: true,
      },
    };
    rendered.rerender(
      <MissionCockpit
        status={finishedExecution()}
        verification={authorizedVer}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
        onAuthorizeCorrection={onAuthorize}
      />,
    );
    expect(screen.getByText(/Scoped correction authorized/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Authorize scoped correction" })).toBeNull();
  });

  it("renders human-reviewable correction task cards with objectives, triggering requirements, scope boundaries, and fresh evidence expectations (Defect #48)", () => {
    const onAuthorize = vi.fn();
    const rejectedVer = pendingVerification();
    rejectedVer.workflow_stage = "USER_DECISION_REJECTED";
    rejectedVer.summary = "You rejected the completed result.";
    rejectedVer.correction_scope = {
      mission_id: "mission-test",
      revision: 1,
      originating_evidence_id: "p8-rejection-art",
      user_rejection_notes: "Route error states are not surfaced cleanly.",
      failed_requirement_ids: ["REQ-A-OUTCOME", "REQ-B-PURPOSE"],
      failed_requirement_titles: ["User problem outcome", "User product purpose"],
      blocked_requirement_ids: ["REQ-ACCESSIBILITY"],
      blocked_requirement_titles: ["NFR: accessibility"],
      affected_task_ids: ["task-1", "task-2", "task-3"],
      preserved_task_ids: ["task-4", "task-5", "task-6", "task-7", "task-8", "task-9", "task-10", "task-11", "task-12", "task-13", "task-14", "task-15"],
      preserved_task_count: 12,
      proposed_tasks: [
        {
          task_id: "task-1",
          title: "Implement: User problem outcome",
          objective: "Implement transport health and route status plan.",
          why_included: "Directly implements failed requirement: User problem outcome",
          triggering_requirement_ids: ["REQ-A-OUTCOME"],
          triggering_requirement_titles: ["User problem outcome"],
          scope_relation: "DIRECT",
          dependency_reason: null,
          authorized_scope: {
            workspace: "D:/TestWorkspace",
            file_scopes: [],
            directory_scopes: [],
            package_lockfiles: ["Cargo.lock"],
            allowed_tools: ["antigravity", "workspace"],
            suggested_scope: "decision",
          },
          expected_outcome: "A reviewable evidence record demonstrates: User problem outcome",
          required_fresh_evidence: ["HumanDecision (A genuine project decision requires explicit human review.)"],
        },
        {
          task_id: "task-2",
          title: "Implement: User product purpose",
          objective: "Expose structured transport route health information.",
          why_included: "Directly implements failed requirement: User product purpose",
          triggering_requirement_ids: ["REQ-B-PURPOSE"],
          triggering_requirement_titles: ["User product purpose"],
          scope_relation: "DIRECT",
          dependency_reason: null,
          authorized_scope: {
            workspace: "D:/TestWorkspace",
            file_scopes: [],
            directory_scopes: [],
            package_lockfiles: ["Cargo.lock"],
            allowed_tools: ["antigravity", "workspace"],
            suggested_scope: "decision",
          },
          expected_outcome: "A reviewable evidence record demonstrates: User product purpose",
          required_fresh_evidence: ["HumanDecision (A genuine project decision requires explicit human review.)"],
        },
        {
          task_id: "task-3",
          title: "Implement: NFR: accessibility",
          objective: "Primary flows must be keyboard navigable.",
          why_included: "Directly implements blocked requirement: NFR: accessibility",
          triggering_requirement_ids: ["REQ-ACCESSIBILITY"],
          triggering_requirement_titles: ["NFR: accessibility"],
          scope_relation: "DIRECT",
          dependency_reason: null,
          authorized_scope: {
            workspace: "D:/TestWorkspace",
            file_scopes: [],
            directory_scopes: [],
            package_lockfiles: ["Cargo.lock"],
            allowed_tools: ["antigravity", "workspace"],
            suggested_scope: "accessibility",
          },
          expected_outcome: "A reviewable evidence record demonstrates: NFR: accessibility",
          required_fresh_evidence: ["AccessibilityResult (Accessibility obligations require an accessibility result.)"],
        },
      ],
      scope_hash: "test-scope-hash-48",
      authorized: false,
    };

    render(
      <MissionCockpit
        status={finishedExecution()}
        verification={rejectedVer}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
        onAuthorizeCorrection={onAuthorize}
      />,
    );

    // Requirements inspectability: titles and IDs
    expect(screen.getAllByText(/User problem outcome/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/User product purpose/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/NFR: accessibility/).length).toBeGreaterThan(0);

    // 3 task cards rendered
    expect(screen.getByTestId("correction-task-card-task-1")).toBeTruthy();
    expect(screen.getByTestId("correction-task-card-task-2")).toBeTruthy();
    expect(screen.getByTestId("correction-task-card-task-3")).toBeTruthy();

    // Human-readable titles rendered
    expect(screen.getByText("Implement: User problem outcome")).toBeTruthy();
    expect(screen.getByText("Implement: User product purpose")).toBeTruthy();
    expect(screen.getByText("Implement: NFR: accessibility")).toBeTruthy();

    // Objectives rendered
    expect(screen.getByText("Implement transport health and route status plan.")).toBeTruthy();
    expect(screen.getByText("Expose structured transport route health information.")).toBeTruthy();
    expect(screen.getByText("Primary flows must be keyboard navigable.")).toBeTruthy();

    // Direct badges
    const badges = screen.getAllByText("Direct Target");
    expect(badges).toHaveLength(3);

    // Fresh evidence expectations rendered
    expect(screen.getAllByText(/HumanDecision \(A genuine project decision/).length).toBeGreaterThan(0);
    expect(screen.getByText(/AccessibilityResult \(Accessibility obligations/)).toBeTruthy();

    // 12 preserved historical tasks notice
    expect(screen.getByText(/12 historical tasks preserved/)).toBeTruthy();

    // Status hint before authorization
    expect(screen.getAllByText(/Will become runnable only after explicit user authorization/)).toHaveLength(3);
  });

  it("renders Verified Complete without attention panel when valid certificate and verified requirements coexist with historical stale records", () => {
    const verified = {
      project_id: "project-1",
      mission_id: "mission-project-1",
      revision: 1,
      execution_run_id: "run-1",
      state: "VERIFICATION_FINISHED",
      completion_state: "VerifiedComplete",
      requirements_verified: 6,
      requirements_total: 6,
      missing_evidence: [],
      failed_checks: [],
      skipped_checks: [],
      stale_evidence: ["p8-collector-old-blocked-evidence"],
      blocked_external: [],
      accepted_risks: [],
      evidence_count: 6,
      certificate: {
        certificate_id: "cert-12345",
        final_state: "VERIFIED_COMPLETE",
        digest: "cert-digest",
      },
      workflow_stage: "VERIFIED_COMPLETE",
      summary: "All required evidence passed.",
      human_decisions: [],
      collector_activity: [],
      collection_failures: [],
      detail: "",
    };

    render(
      <MissionCockpit
        status={finishedExecution()}
        verification={verified}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Verified Complete" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Verification needs attention" })).toBeNull();
    expect(screen.queryByText("What needs attention")).toBeNull();
  });

  it("renders Verification needs attention when certificate is missing or current evidence is missing", () => {
    const incomplete = {
      project_id: "project-1",
      mission_id: "mission-project-1",
      revision: 1,
      execution_run_id: "run-1",
      state: "VERIFICATION_FINISHED",
      completion_state: "StoppedIncomplete",
      requirements_verified: 2,
      requirements_total: 6,
      missing_evidence: ["requirement-test: missing TEST_OUTPUT"],
      failed_checks: [],
      skipped_checks: [],
      stale_evidence: [],
      blocked_external: [],
      accepted_risks: [],
      evidence_count: 2,
      certificate: null,
      workflow_stage: "VERIFICATION_NEEDS_ATTENTION",
      summary: "Missing evidence.",
      human_decisions: [],
      collector_activity: [],
      collection_failures: [],
      detail: "",
    };

    render(
      <MissionCockpit
        status={finishedExecution()}
        verification={incomplete}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.queryByRole("heading", { name: "Verified Complete" })).toBeNull();
    expect(screen.getAllByRole("heading", { name: "Verification needs attention" })).toHaveLength(2);
  });

  it("renders precise bounded correction scope with structured findings, deduplication notice, bounded files, and human refinement guard (Defect #49)", () => {
    const onAuthorize = vi.fn();
    const rejectedVer = pendingVerification();
    rejectedVer.workflow_stage = "USER_DECISION_REJECTED";
    rejectedVer.summary = "You rejected the completed result.";
    rejectedVer.correction_scope = {
      mission_id: "mission-test-49",
      revision: 1,
      originating_evidence_id: "p8-rejection-art",
      user_rejection_notes: "REJECTED: 1. Documentation falsely claims approval. 2. State transitions unreliable. 3. Accessibility blocked.",
      failed_requirement_ids: ["REQ-A-OUTCOME", "REQ-B-PURPOSE"],
      failed_requirement_titles: ["User problem outcome", "User product purpose"],
      blocked_requirement_ids: ["REQ-ACCESSIBILITY"],
      blocked_requirement_titles: ["NFR: accessibility"],
      affected_task_ids: ["task-1", "task-3"],
      deduplicated_task_ids: ["task-2"],
      preserved_task_ids: ["task-2", "task-4", "task-5"],
      preserved_task_count: 3,
      semantic_correction_authorities: 1,
      duplicate_correction_work: 0,
      unrelated_tasks: 0,
      project_wide_unbounded_authority: false,
      technical_findings_with_only_humandecision_evidence: 0,
      human_refinement_required: false,
      correction_units: [
        {
          correction_id: "corr-1-01",
          semantic_finding: "Documentation falsely claims approval.",
          triggering_human_decision: "p8-rejection-art",
          affected_requirements: ["REQ-A-OUTCOME"],
          affected_tasks: ["task-1"],
          affected_source_or_artifact_scope: ["docs/PLAN.md"],
          why_scope_is_included: "Documentation correction required.",
          required_fresh_evidence: ["TEST_OUTPUT (Documentation integrity check)", "Source/policy inspection"],
          dependencies: [],
        },
        {
          correction_id: "corr-1-02",
          semantic_finding: "Accessibility blocked.",
          triggering_human_decision: "p8-rejection-art",
          affected_requirements: ["REQ-ACCESSIBILITY"],
          affected_tasks: ["task-3"],
          affected_source_or_artifact_scope: ["docs/ACCESSIBILITY.md"],
          why_scope_is_included: "Accessibility obligation remains blocked.",
          required_fresh_evidence: ["ACCESSIBILITY_RESULT"],
          dependencies: ["task-1"],
        },
      ],
      proposed_tasks: [
        {
          task_id: "task-1",
          title: "Implement: User problem outcome",
          objective: "Implement transport health and route status plan.",
          why_included: "Directly implements failed requirement: User problem outcome",
          triggering_requirement_ids: ["REQ-A-OUTCOME"],
          triggering_requirement_titles: ["User problem outcome"],
          scope_relation: "DIRECT",
          dependency_reason: null,
          authorized_scope: {
            workspace: "D:/TestWorkspace",
            file_scopes: [],
            directory_scopes: [],
            package_lockfiles: ["Cargo.lock"],
            allowed_tools: ["antigravity", "workspace"],
            suggested_scope: "decision",
            bounded_file_scopes: ["crates/routing/src/lib.rs", "docs/PLAN.md"],
            authority_boundary_type: "PROVENANCE_BOUNDED",
            is_bounded: true,
          },
          expected_outcome: "Technical proofs pass and owner ratifies outcome",
          required_fresh_evidence: [
            "TEST_OUTPUT (Documentation integrity check)",
            "HUMAN_DECISION (Genuine final owner acceptance after technical proofs pass)",
          ],
        },
        {
          task_id: "task-3",
          title: "Implement: NFR: accessibility",
          objective: "Primary flows must be keyboard navigable.",
          why_included: "Directly implements blocked requirement: NFR: accessibility",
          triggering_requirement_ids: ["REQ-ACCESSIBILITY"],
          triggering_requirement_titles: ["NFR: accessibility"],
          scope_relation: "DIRECT",
          dependency_reason: null,
          authorized_scope: {
            workspace: "D:/TestWorkspace",
            file_scopes: [],
            directory_scopes: [],
            package_lockfiles: ["Cargo.lock"],
            allowed_tools: ["antigravity", "workspace"],
            suggested_scope: "accessibility",
            bounded_file_scopes: ["docs/ACCESSIBILITY.md"],
            authority_boundary_type: "PROVENANCE_BOUNDED",
            is_bounded: true,
          },
          expected_outcome: "Accessibility audit passes",
          required_fresh_evidence: ["ACCESSIBILITY_RESULT"],
        },
      ],
      scope_hash: "test-scope-hash-49",
      authorized: false,
    };

    const rendered = render(
      <MissionCockpit
        status={finishedExecution()}
        verification={rejectedVer}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
        onAuthorizeCorrection={onAuthorize}
      />,
    );

    // 1. Structured rejection findings rendered
    expect(screen.getByTestId("correction-units-section")).toBeTruthy();
    expect(screen.getByText("Documentation falsely claims approval.")).toBeTruthy();
    expect(screen.getByText("Accessibility blocked.")).toBeTruthy();
    expect(screen.getAllByText(/docs\/PLAN\.md/).length).toBeGreaterThan(0);

    // 2. Semantic deduplication notice rendered
    expect(screen.getByTestId("correction-deduplication-notice")).toBeTruthy();
    expect(screen.getByText(/Semantic Deduplication Applied/)).toBeTruthy();
    expect(screen.getByText(/Duplicate correction work: 0/)).toBeTruthy();

    // 3. Bounded authority scope rendered
    expect(screen.getAllByText("PROVENANCE_BOUNDED").length).toBe(2);
    expect(screen.getByText(/crates\/routing\/src\/lib\.rs, docs\/PLAN\.md/)).toBeTruthy();

    // 4. Proposed tasks reflect deduplication: exactly 2 tasks, task-2 excluded
    expect(screen.queryByTestId("correction-task-card-task-2")).toBeNull();
    expect(screen.getByTestId("correction-task-card-task-1")).toBeTruthy();
    expect(screen.getByTestId("correction-task-card-task-3")).toBeTruthy();

    // 5. Authorize button enabled when bounded
    const authBtn = screen.getByRole("button", { name: "Authorize scoped correction" });
    expect(authBtn).toBeTruthy();
    expect((authBtn as HTMLButtonElement).disabled).toBe(false);

    // 6. When human_refinement_required is true: alert rendered and button disabled
    const refinementVer = {
      ...rejectedVer,
      correction_scope: {
        ...rejectedVer.correction_scope,
        human_refinement_required: true,
      },
    };
    rendered.rerender(
      <MissionCockpit
        status={finishedExecution()}
        verification={refinementVer}
        antigravity={readyAntigravity}
        busy={false}
        revalidating={false}
        verificationNotice={null}
        onCommand={vi.fn()}
        onVerify={vi.fn()}
        onDecision={vi.fn()}
        onRefresh={vi.fn()}
        onAuthorizeCorrection={onAuthorize}
      />,
    );
    expect(screen.getByTestId("human-refinement-required-alert")).toBeTruthy();
    const disabledBtn = screen.getByRole("button", { name: "Human scope refinement required" });
    expect((disabledBtn as HTMLButtonElement).disabled).toBe(true);
  });
});
