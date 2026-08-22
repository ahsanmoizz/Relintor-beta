import type { ExecutionStatus, VerificationStatus } from "./backend";

export type SemanticTone = "neutral" | "info" | "success" | "warning" | "danger";
export type MissionPrimaryAction = "none" | "setup" | "run" | "continue" | "recover" | "verify" | "view_verification";

export type MissionPresentation = {
  headline: string;
  supporting: string;
  badge: string;
  tone: SemanticTone;
  primaryAction: MissionPrimaryAction;
  recoveryRequired: boolean;
  verifiedComplete: boolean;
};

function token(value: string | null | undefined): string {
  return (value || "").replace(/[^a-z0-9]/gi, "").toUpperCase();
}

export function humanStatus(value: string | null | undefined, fallback = "Waiting"): string {
  const labels: Record<string, string> = {
    READY: "Ready",
    HEALTHY: "Healthy",
    RUNNING: "Running",
    ACTIVE: "Active",
    COMPLETED: "Complete",
    VERIFIEDCOMPLETE: "Verified Complete",
    VERIFICATIONFINISHED: "Verification finished",
    REVALIDATIONREQUIRED: "Safety check required",
    BLOCKEDEXTERNAL: "Blocked by an external dependency",
    STOPPEDINCOMPLETE: "Stopped before completion",
    TURNENDEDINCOMPLETE: "Ready to continue",
    EXECUTIONTASKSFINISHEDAWAITINGVERIFICATION: "Execution finished",
    NORECOVERYREQUIRED: "No recovery needed",
    PREEXECUTIONRETRYAUTHORIZED: "Safe to retry",
    PREEXECUTIONPREVENTED: "Stopped before external work",
    SAFETORESUME: "Safe to resume",
    SAFETORESUMEAFTERPROCESSRECONCILIATION: "Safe to resume",
    RECOVERYNOTALLOWED: "Manual review required",
    INTERRUPTEDATSAFECHECKPOINT: "Safe checkpoint available",
    INTERRUPTEDWITHOUTSAFECHECKPOINT: "Manual review required",
    P9CHECKPOINTNOTYETCREATED: "No recovery record yet",
    AUTHORITYREVALIDATIONREQUIRED: "Recovery review required",
    CONTINUATIONSTARTED: "Continuation started",
    CONTINUATIONAUTHORIZED: "Continuation authorized",
    RUNSTARTED: "Mission created",
    TASKATTEMPTSTARTED: "Task started",
    LEASEISSUED: "Execution authorized",
    BUDGETWARNING: "Execution time limit reached",
    SAFESTOP: "Stopped safely",
  };
  const normalized = token(value);
  if (labels[normalized]) return labels[normalized];
  if (!value) return fallback;
  return value
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replaceAll("_", " ")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase()
    .replace(/^./, (letter) => letter.toUpperCase());
}

export function displayWindowsPath(value: string): string {
  return value.replace(/^\\\\\?\\/, "");
}

export function displayProjectName(value: string): string {
  const clean = displayWindowsPath(value).replace(/[\\/]+$/, "");
  const segments = clean.split(/[\\/]/).filter(Boolean);
  return segments.at(-1) || "Project";
}

export function verificationPresentation(status: VerificationStatus | null): {
  label: string;
  supporting: string;
  tone: SemanticTone;
  verifiedComplete: boolean;
} {
  if (!status || status.execution_run_id === "browser-preview") {
    return {
      label: "Waiting for evidence",
      supporting: "Relintor will verify the work after execution evidence is captured.",
      tone: "info",
      verifiedComplete: false,
    };
  }

  const hasMaterialGap =
    status.missing_evidence.length > 0 ||
    status.failed_checks.length > 0 ||
    status.stale_evidence.length > 0 ||
    status.blocked_external.length > 0 ||
    status.requirements_total === 0 ||
    status.requirements_verified !== status.requirements_total;
  const authoritySaysVerified = token(status.completion_state) === "VERIFIEDCOMPLETE";
  if (authoritySaysVerified && !hasMaterialGap && status.evidence_count > 0) {
    return {
      label: "Verified Complete",
      supporting: `${status.requirements_verified} of ${status.requirements_total} requirements passed with ${status.evidence_count} evidence ${status.evidence_count === 1 ? "artifact" : "artifacts"}.`,
      tone: "success",
      verifiedComplete: true,
    };
  }
  if (status.failed_checks.length || status.stale_evidence.length || status.blocked_external.length || authoritySaysVerified) {
    return {
      label: "Verification needs attention",
      supporting: "Relintor found evidence or dependency issues that must be resolved before completion can be claimed.",
      tone: "warning",
      verifiedComplete: false,
    };
  }
  if (status.evidence_count === 0) {
    return {
      label: "Waiting for evidence",
      supporting: "Relintor will verify the work after execution evidence is captured.",
      tone: "info",
      verifiedComplete: false,
    };
  }
  return {
    label: "Ready to verify",
    supporting: `${status.evidence_count} evidence ${status.evidence_count === 1 ? "artifact is" : "artifacts are"} ready for review.`,
    tone: "info",
    verifiedComplete: false,
  };
}

export function missionPresentation(
  status: ExecutionStatus,
  verification: VerificationStatus | null,
  executorReady: boolean,
): MissionPresentation {
  const state = token(status.state);
  const phase = token(status.execution_phase);
  const recovery = token(status.recovery_state);
  const running = phase === "RUNNING";
  const dispatching = status.dispatch_active && !running;
  const recoveryRequired =
    !running &&
    !dispatching &&
    (["BLOCKEDEXTERNAL", "REVALIDATIONREQUIRED", "STOPPEDINCOMPLETE"].includes(state) ||
      ["P9RECOVERYUNAVAILABLE", "REVALIDATIONREQUIRED", "RECOVERYNOTALLOWED", "BLOCKEDEXTERNAL"].includes(recovery));
  const verificationView = verificationPresentation(verification);

  if (running) {
    return {
      headline: "Antigravity is working",
      supporting: "Relintor is supervising the authorized task and recording durable activity.",
      badge: "Running",
      tone: "info",
      primaryAction: "none",
      recoveryRequired: false,
      verifiedComplete: false,
    };
  }
  if (dispatching) {
    return {
      headline: "Starting Antigravity",
      supporting: "Relintor is waiting for the owned executor process before it reports the task as running.",
      badge: "Starting",
      tone: "info",
      primaryAction: "none",
      recoveryRequired: false,
      verifiedComplete: false,
    };
  }
  if (recoveryRequired) {
    return {
      headline: "Recovery review required",
      supporting: "Relintor must confirm the previous attempt's safety before another task can run.",
      badge: "Needs attention",
      tone: "warning",
      primaryAction: "recover",
      recoveryRequired: true,
      verifiedComplete: false,
    };
  }
  if (verificationView.verifiedComplete) {
    return {
      headline: "Verified Complete",
      supporting: verificationView.supporting,
      badge: "Verified",
      tone: "success",
      primaryAction: "view_verification",
      recoveryRequired: false,
      verifiedComplete: true,
    };
  }
  if (state === "EXECUTIONTASKSFINISHEDAWAITINGVERIFICATION") {
    if (!verification || verification.execution_run_id === "browser-preview") {
      return {
        headline: "Capture evidence and check requirements",
        supporting: "Run verification to capture evidence from this exact execution attempt, then check every sealed requirement.",
        badge: "Evidence pending",
        tone: "info",
        primaryAction: "verify",
        recoveryRequired: false,
        verifiedComplete: false,
      };
    }
    return {
      headline: verificationView.label === "Verification needs attention" ? verificationView.label : "Work finished — verify the evidence",
      supporting: verificationView.supporting,
      badge: verificationView.label === "Verification needs attention" ? "Needs attention" : "Evidence ready",
      tone: verificationView.tone,
      primaryAction: "verify",
      recoveryRequired: false,
      verifiedComplete: false,
    };
  }
  if (!executorReady) {
    return {
      headline: "Antigravity setup required",
      supporting: "Complete the guided executor setup before running this mission.",
      badge: "Setup required",
      tone: "warning",
      primaryAction: "setup",
      recoveryRequired: false,
      verifiedComplete: false,
    };
  }
  if (state === "TURNENDEDINCOMPLETE") {
    return {
      headline: "Ready to continue",
      supporting: "The previous turn ended at a durable boundary. Continue with the same sealed mission.",
      badge: "Ready",
      tone: "neutral",
      primaryAction: "continue",
      recoveryRequired: false,
      verifiedComplete: false,
    };
  }
  return {
    headline: "Ready to run",
    supporting: "Relintor is ready to authorize the next task in this sealed mission.",
    badge: "Ready",
    tone: "neutral",
    primaryAction: "run",
    recoveryRequired: false,
    verifiedComplete: false,
  };
}

export function eventPresentation(kind: string, detail: string): { title: string; detail: string } {
  const title = humanStatus(kind, "Mission activity");
  const normalized = `${kind} ${detail}`.toLowerCase();
  if (/p6 handoff validated|runstarted/.test(normalized)) {
    return { title: "Mission created", detail: "The sealed mission was accepted by the execution authority." };
  }
  if (/leaseissued/.test(normalized)) {
    return { title: "Execution authorized", detail: "Relintor issued a bounded authorization for this task attempt." };
  }
  if (/attemptstarted|taskattemptstarted/.test(normalized)) {
    return { title: "Task started", detail: "Antigravity began work inside the authorized mission boundary." };
  }
  if (/budgetwarning|wall-clock|time limit/.test(normalized)) {
    return { title: "Execution time limit reached", detail: "Relintor stopped accepting work beyond the authorized time window." };
  }
  if (/revalidationrequired|recovery decision/.test(normalized)) {
    return { title: "Recovery review required", detail: "Relintor recorded that another attempt needs a fresh safety decision." };
  }
  return { title, detail };
}
