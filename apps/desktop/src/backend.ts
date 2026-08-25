import { invoke } from "@tauri-apps/api/core";

export type HealthStatus = {
  status: string;
  detail: string;
};

export type DatabaseHealth = {
  status: string;
  detail: string;
  schema_version: number | null;
  database_path: string | null;
};

export type KeychainHealth = {
  status: string;
  provider: string;
  detail: string;
};

export type AntigravityHealth = {
  status: string;
  version: string | null;
  executable: string | null;
  compatibility: string;
  cli_invocation_capability: boolean;
  plugin_hook_capability: boolean;
  ide_status: string;
  cli_status: string;
  authentication_status: string;
  executable_detectable: boolean;
  adapter_ready: boolean;
  setup_required: boolean;
  environment: string;
  platform: string;
  detected_at_ms: number;
  detail: string;
};

export type DiskSpaceView = {
  available_bytes: number | null;
  safe_minimum_bytes: number;
};

export type AntigravitySetupView = {
  stage: string;
  active: boolean;
  progress_percent: number | null;
  progress_indeterminate: boolean;
  storage_path: string | null;
  recommended_storage_path: string | null;
  storage_detail: string;
  c: DiskSpaceView;
  d: DiskSpaceView;
  local_appdata_state: string;
  cli_path: string | null;
  cli_version: string | null;
  authentication_status: string;
  adapter_ready: boolean;
  consent_required: boolean;
  automatic_install_available: boolean;
  requires_location: boolean;
  can_cancel: boolean;
  can_retry: boolean;
  detail: string;
  error: string | null;
  advanced_details: string;
};

export type DesktopHealth = {
  application: HealthStatus;
  database: DatabaseHealth;
  specification: HealthStatus;
  keychain: KeychainHealth;
  antigravity: AntigravityHealth;
};

export type AccountState = {
  status: "signed_out" | "signed_in" | "offline" | "invalid";
  email: string | null;
  plan: string | null;
  entitlement: "VALID" | "GRACE" | "EXPIRED" | "UNAVAILABLE" | "INVALID_SIGNATURE";
  detail: string;
};

export type InvestigationView = {
  investigation: {
    id: string;
    project_id: string;
    status: string;
    provider: { live: boolean; availability: string };
  };
  blueprint: {
    status: string;
    product_definition: string;
    problem_outcome: string;
    capabilities: string[];
    questions: Array<{
      id: string;
      question: string;
      why_it_matters: string;
      recommended_default: string;
      options: Array<{ id: string; label: string; consequence: string }>;
      can_defer: boolean;
      affects: string[];
      disposition: string;
    }>;
    conflicts: Array<{
      id: string;
      claim_a: string;
      claim_b: string;
      severity: string;
      recommended_resolution: string;
      resolution_state: string;
    }>;
    assumptions: Array<{ id: string; statement: string; consequence_if_wrong: string }>;
    risks: Array<{ id: string; title: string; severity: string; mitigation: string }>;
    architecture_decisions: Array<{
      id: string;
      decision_question: string;
      selected_option: string | null;
      reason: string;
      status: string;
    }>;
    personas: Array<{ id: string; name: string; functional_role: string; needs: string[] }>;
    journeys: Array<{ id: string; goal: string; major_steps: string[]; success_state: string }>;
    non_functional_requirements: Array<{ id: string; domain: string; statement: string; classification: string }>;
    fingerprint: string;
  };
};

export type InvestigatorAnswer = {
  question_id: string;
  option_id: string | null;
  not_sure: boolean;
};

export type TakeoverView = {
  project_id: string;
  takeover: { id: string; root: string; scanner_version: string };
  snapshot: { is_git_repository: boolean; file_count: number; total_bytes: number; fingerprint: { value: string } };
  fingerprint: string;
  inventory: Array<{ relative_path: string; classification: string; included: boolean; size_bytes: number; excluded_reason?: string | null }>;
  build_systems: Array<{ name: string; confidence: string; status: string }>;
  dependency_graph: { dependencies: Array<{ name: string; kind: string; missing_reference: boolean }>; cycles: string[][]; manifest_lock_inconsistencies: string[] };
  routes: Array<{ method: string; path: string; handler: string | null; status: string; source: { locator: string } }>;
  database_systems: Array<{ technology: string; confidence: string }>;
  migrations: Array<{ identifier: string; path: string; duplicate_identifier: boolean; ordering_gap: boolean }>;
  auth_systems: Array<{ technology: string; states: string[]; confidence: string }>;
  ui_surfaces: Array<{ name: string; path: string; state: string }>;
  test_suites: Array<{ name: string; suite_type: string; skipped_or_disabled: boolean }>;
  ci_workflows: Array<{ name: string; disabled: boolean; commands: string[] }>;
  deployment_targets: Array<{ target: string; state: string }>;
  documentation_promises: Array<{ text: string; location: string; verification_state: string }>;
  capabilities: Array<{ name: string; classification: string; confidence: string }>;
  findings: Array<{ finding_type: string; summary: string; severity: string; classification: string | null }>;
  conflicts: Array<{ claim_a: string; claim_b: string; severity: string }>;
  recommendations: Array<{ target: string; kind: string; current_reality: string; priority: string; reason: string }>;
};

export type ProjectSummary = {
  project_id: string;
  name: string;
  root_path: string | null;
  state: string;
  investigation_status: string | null;
  takeover_fingerprint: string | null;
  sealed_revision: number | null;
};

export type ProjectOpenView = {
  project: ProjectSummary;
  investigation: InvestigationView | null;
  authority_facts: AuthorityFactDecision[];
  handoff: ExecutionHandoff | null;
};

export type AuthorityPreview = {
  registry_version: number;
  total_rules: number;
  applicable_rules: number;
  not_applicable_rules: number;
  packs: Array<{
    pack_id: string;
    title: string;
    applicable_rules: number;
    not_applicable_rules: number;
    explanation: string;
  }>;
  requirements: Array<{
    requirement_id: string;
    title: string;
    source: string;
    priority: string;
    state: string;
    why_required: string;
    acceptance: string[];
    evidence: string[];
    dependencies: string[];
  }>;
  task_count: number;
  sealing_state: string;
  blockers: string[];
  review_digest: string;
};

export type AuthorityFactDecision = {
  id: string;
  decision: "yes" | "no" | "not_sure";
};

export type ExecutionHandoff = {
  mission_id: string;
  revision: number;
  contract_hash: string;
  state: string;
  task_order: string[];
  scheduler_owner: string;
};

export type ExecutionStatus = {
  project_id: string;
  project_name: string;
  mission_id: string;
  revision: number;
  state: string;
  watchdog_state: string;
  current_turn: number;
  active_task: string | null;
  current_task_objective: string | null;
  recovery_task_id: string | null;
  recovery_task_objective: string | null;
  runnable_tasks: string[];
  total_tasks: number;
  finished_tasks: number;
  tool_calls: number;
  execution_steps: number;
  estimated_cost_micros: number | null;
  safe_boundary_reached: boolean;
  last_event: string | null;
  ledger_path: string;
  recovery_state: string;
  last_safe_checkpoint: string | null;
  resume_disposition: string | null;
  resume_blocker: string | null;
  external_changes: string[];
  recovery_detected: boolean;
  recovery_action: "NONE" | "CHECK_SAFETY" | "MANUAL_REVIEW_RETRY" | string;
  dispatch_active: boolean;
  execution_phase: string;
  execution_time_limit_ms: number;
  events: Array<{ sequence: number; occurred_at_ms: number; task_id: string | null; kind: string; detail: string }>;
};

export type VerificationStatus = {
  project_id: string;
  mission_id: string;
  revision: number;
  execution_run_id: string;
  state: string;
  completion_state: string;
  requirements_verified: number;
  requirements_total: number;
  missing_evidence: string[];
  failed_checks: string[];
  skipped_checks: string[];
  stale_evidence: string[];
  blocked_external: string[];
  accepted_risks: string[];
  evidence_count: number;
  certificate: { certificate_id: string; final_state: string; digest: string } | null;
  workflow_stage: string;
  summary: string;
  human_decisions: Array<{
    requirement_id: string;
    title: string;
    question: string;
    summary: string;
    criterion_ids: string[];
  }>;
  collector_activity: string[];
  collection_failures: string[];
  detail: string;
};

export type VerificationEvidence = {
  evidence_id: string;
  class: string;
  result: string;
  confidence: string;
  digest: string;
};

const browserPreviewHealth: DesktopHealth = {
  application: {
    status: "unavailable",
    detail: "Browser preview has no Rust authority process.",
  },
  database: {
    status: "unavailable",
    detail: "Browser preview cannot inspect the Rust-owned local database.",
    schema_version: null,
    database_path: null,
  },
  specification: {
    status: "unavailable",
    detail: "Browser preview cannot verify bundled specification resources.",
  },
  keychain: {
    status: "unavailable",
    provider: "OS keychain",
    detail: "Browser preview cannot inspect the operating-system secure store.",
  },
  antigravity: {
    status: "unknown",
    version: null,
    executable: null,
    compatibility: "unknown",
    cli_invocation_capability: false,
    plugin_hook_capability: false,
    ide_status: "not_verified",
    cli_status: "not_available_in_browser_preview",
    authentication_status: "not_available_in_browser_preview",
    executable_detectable: false,
    adapter_ready: false,
    setup_required: true,
    environment: "browser-preview",
    platform: "browser",
    detected_at_ms: 0,
    detail: "Antigravity detection runs only from the desktop authority process.",
  },
};

const signedOutAccount: AccountState = {
  status: "signed_out",
  email: null,
  plan: null,
  entitlement: "UNAVAILABLE",
  detail: "No account session is active. Provider credentials are never requested here.",
};

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function readDesktopHealth(): Promise<DesktopHealth> {
  if (!isTauriRuntime()) {
    return browserPreviewHealth;
  }

  try {
    return await invoke<DesktopHealth>("health_all");
  } catch (error) {
    return {
      ...browserPreviewHealth,
      application: {
        status: "failed",
        detail: `Desktop authority health read failed: ${String(error)}`,
      },
    };
  }
}

export async function readAntigravityHealth(): Promise<AntigravityHealth> {
  if (!isTauriRuntime()) return browserPreviewHealth.antigravity;
  return invoke<AntigravityHealth>("health_antigravity");
}

const browserPreviewSetup: AntigravitySetupView = {
  stage: "FAILED",
  active: false,
  progress_percent: null,
  progress_indeterminate: false,
  storage_path: null,
  recommended_storage_path: null,
  storage_detail: "Use the installed desktop app to inspect Windows storage.",
  c: { available_bytes: null, safe_minimum_bytes: 5 * 1024 * 1024 * 1024 },
  d: { available_bytes: null, safe_minimum_bytes: 5 * 1024 * 1024 * 1024 },
  local_appdata_state: "not_available_in_browser_preview",
  cli_path: null,
  cli_version: null,
  authentication_status: "not_available",
  adapter_ready: false,
  consent_required: true,
  automatic_install_available: false,
  requires_location: true,
  can_cancel: false,
  can_retry: false,
  detail: "Antigravity setup runs only from the installed desktop app.",
  error: null,
  advanced_details: "browser-preview; no installer or filesystem access",
};

export async function readAntigravitySetupStatus(): Promise<AntigravitySetupView> {
  if (!isTauriRuntime()) return browserPreviewSetup;
  return invoke<AntigravitySetupView>("antigravity_setup_status");
}

export async function startAntigravitySetup(requestedPath: string | null): Promise<AntigravitySetupView> {
  if (!isTauriRuntime()) {
    return browserPreviewSetup;
  }
  return invoke<AntigravitySetupView>("antigravity_setup_start", { requestedPath });
}

export async function cancelAntigravitySetup(): Promise<AntigravitySetupView> {
  if (!isTauriRuntime()) return browserPreviewSetup;
  return invoke<AntigravitySetupView>("antigravity_setup_cancel");
}

export async function chooseAntigravityInstallLocation(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  return invoke<string | null>("antigravity_choose_install_location");
}

export async function chooseExistingAntigravityCli(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  return invoke<string | null>("antigravity_choose_existing_cli");
}

export async function useExistingAntigravityCli(path: string): Promise<AntigravitySetupView> {
  if (!isTauriRuntime()) return browserPreviewSetup;
  return invoke<AntigravitySetupView>("antigravity_use_existing_cli", { path });
}

export async function signInToAntigravity(): Promise<AntigravitySetupView> {
  if (!isTauriRuntime()) return browserPreviewSetup;
  return invoke<AntigravitySetupView>("antigravity_sign_in");
}

export async function readAccountState(): Promise<AccountState> {
  if (!isTauriRuntime()) return signedOutAccount;
  try {
    return await invoke<AccountState>("account_state");
  } catch {
    return {
      ...signedOutAccount,
      status: "offline",
      detail: "Cloud authentication is unavailable. Try again when connected.",
    };
  }
}

export async function signInWithGoogle(): Promise<AccountState> {
  if (!isTauriRuntime()) {
    throw new Error("Google sign-in is available from the installed desktop app.");
  }
  return invoke<AccountState>("sign_in_with_google");
}

export async function signOut(): Promise<AccountState> {
  if (!isTauriRuntime()) return signedOutAccount;
  return invoke<AccountState>("sign_out");
}

export async function createNewProject(idea: string): Promise<InvestigationView> {
  if (isTauriRuntime()) {
    return invoke<InvestigationView>("investigator_new_project", { idea });
  }
  return browserPreviewInvestigation(idea);
}

export async function answerInvestigation(idea: string, answers: InvestigatorAnswer[]): Promise<InvestigationView> {
  if (isTauriRuntime()) {
    return invoke<InvestigationView>("investigator_answer_questions", { idea, answers });
  }
  const preview = browserPreviewInvestigation(idea);
  return {
    ...preview,
    blueprint: {
      ...preview.blueprint,
      status: answers.length ? "ReadyForReview" : preview.blueprint.status,
      questions: preview.blueprint.questions.map((question) => {
        const answer = answers.find((candidate) => candidate.question_id === question.id);
        return answer ? { ...question, disposition: answer.not_sure ? "AutoDefaulted" : "Answered" } : question;
      }),
      assumptions: answers.filter((answer) => answer.not_sure).map((answer) => ({
        id: `browser-assumption-${answer.question_id}`,
        statement: "Recommended default applied because you selected I’m not sure.",
        consequence_if_wrong: "Revisit this decision before implementation.",
      })),
    },
  };
}

export async function takeoverScan(root: string): Promise<TakeoverView> {
  if (isTauriRuntime()) {
    return invoke<TakeoverView>("takeover_scan", { root });
  }
  return browserPreviewTakeover(root);
}

export async function chooseWorkspace(): Promise<string | null> {
  if (!isTauriRuntime()) {
    throw new Error("Workspace selection is available from the installed desktop app.");
  }
  return invoke<string | null>("choose_workspace");
}

export async function setProjectWorkspace(projectId: string, rootPath: string): Promise<ProjectOpenView> {
  if (!isTauriRuntime()) {
    throw new Error("Workspace selection is available from the installed desktop app.");
  }
  return invoke<ProjectOpenView>("set_project_workspace", { projectId, rootPath });
}

export async function investigateTakeoverProject(projectId: string, goal: string): Promise<InvestigationView> {
  if (isTauriRuntime()) {
    return invoke<InvestigationView>("investigator_takeover_project", { projectId, goal });
  }
  const preview = browserPreviewInvestigation(goal);
  return {
    ...preview,
    investigation: { ...preview.investigation, project_id: projectId },
  };
}

export async function listProjects(): Promise<ProjectSummary[]> {
  if (!isTauriRuntime()) return [];
  return invoke<ProjectSummary[]>("projects_list");
}

export async function openProject(projectId: string): Promise<ProjectOpenView> {
  if (!isTauriRuntime()) {
    throw new Error("Persisted project reopening is available from the installed desktop app.");
  }
  return invoke<ProjectOpenView>("project_open", { projectId });
}

export async function evaluateAuthority(projectId: string | null, facts: AuthorityFactDecision[]): Promise<AuthorityPreview> {
  if (isTauriRuntime()) {
    if (!projectId) throw new Error("A persisted Relintor project is required before authority review.");
    return invoke<AuthorityPreview>("authority_preview", { projectId, facts });
  }
  const applicable = new Set(facts.filter((fact) => fact.decision === "yes").map((fact) => fact.id));
  const packNames: Record<string, string> = {
    web: "Web frontend",
    backend: "Backend/API",
    database: "Databases",
    authentication: "Authentication/authorization",
    ui_surface: "Accessibility",
    seo_relevance: "SEO/discoverability",
    performance: "Performance",
    deployment: "DevOps/release engineering",
    observability: "Observability/operations",
    privacy: "Data/privacy",
    payments: "Payments/financial workflows",
    ai: "AI/ML applications",
    blockchain: "Blockchain/Web3",
    mobile: "Mobile",
    desktop: "Desktop",
    data_engineering: "Data engineering",
    integrations: "Third-party integrations",
  };
  const packs = Object.entries(packNames).map(([id, title]) => ({
    pack_id: id,
    title,
    applicable_rules: applicable.has(id) ? 2 : 0,
    not_applicable_rules: applicable.has(id) ? 0 : 2,
    explanation: applicable.has(id)
      ? "This project surface was selected for review."
      : "This project surface was not selected, so its rules are not applicable in this preview.",
  }));
  return {
    registry_version: 1,
    total_rules: 16,
    applicable_rules: applicable.size * 2,
    not_applicable_rules: Math.max(0, Object.keys(packNames).length * 2 - applicable.size * 2),
    packs,
    requirements: facts.filter((fact) => fact.decision === "yes").map((fact, index) => ({
      requirement_id: `browser-preview-requirement-${index + 1}`,
      title: `${packNames[fact.id] || fact.id} review obligation`,
      source: "Selected project surface (browser preview)",
      priority: "P1",
      state: "UNSTARTED",
      why_required: "The selected project surface needs a reviewable implementation obligation.",
      acceptance: ["A deterministic check demonstrates the obligation."],
      evidence: ["TEST_OUTPUT"],
      dependencies: [],
    })),
    task_count: applicable.size,
    sealing_state: "REVIEW_REQUIRED",
    blockers: [
      "Browser preview cannot seal a mission; open the desktop authority process for Rust-backed evaluation.",
      "P7 execution is not started; this screen does not claim execution or completion.",
    ],
    review_digest: "browser-preview",
  };
}

export async function sealProjectMission(projectId: string, expectedReviewDigest: string): Promise<ExecutionHandoff> {
  if (!isTauriRuntime()) {
    throw new Error("Browser preview cannot seal a mission; use the Rust desktop authority process.");
  }
  return invoke<ExecutionHandoff>("seal_project_mission", { projectId, expectedReviewDigest });
}

function browserPreviewExecution(projectId: string): ExecutionStatus {
  return {
    project_id: projectId,
    project_name: "Browser preview mission",
    mission_id: `mission-${projectId}`,
    revision: 0,
    state: "BROWSER_PREVIEW_UNAVAILABLE",
    watchdog_state: "UNAVAILABLE",
    current_turn: 0,
    active_task: null,
    current_task_objective: null,
    recovery_task_id: null,
    recovery_task_objective: null,
    runnable_tasks: [],
    total_tasks: 0,
    finished_tasks: 0,
    tool_calls: 0,
    execution_steps: 0,
    estimated_cost_micros: null,
    safe_boundary_reached: false,
    last_event: "Execution status is available only from the Rust authority process.",
    events: [],
    ledger_path: "browser-preview",
    recovery_state: "P9_RECOVERY_UNAVAILABLE",
    last_safe_checkpoint: null,
    resume_disposition: null,
    resume_blocker: null,
    external_changes: [],
    recovery_detected: false,
    recovery_action: "NONE",
    dispatch_active: false,
    execution_phase: "READY",
    execution_time_limit_ms: 600000,
  };
}

export async function readExecutionStatus(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_status", { projectId });
}

export async function startExecution(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_start", { projectId });
}

export async function stepExecution(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_step", { projectId });
}

export async function pauseExecution(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_pause", { projectId });
}

export async function stopExecution(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_stop", { projectId });
}

export async function continueExecution(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_continue", { projectId });
}

export async function revalidateExecution(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_revalidate", { projectId });
}

export async function retryRecoveredTask(projectId: string): Promise<ExecutionStatus> {
  if (!isTauriRuntime()) return browserPreviewExecution(projectId);
  return invoke<ExecutionStatus>("execution_retry_recovered_task", { projectId });
}

function browserPreviewVerification(projectId: string): VerificationStatus {
  return {
    project_id: projectId,
    mission_id: `mission-${projectId}`,
    revision: 0,
    execution_run_id: "browser-preview",
    state: "VERIFICATION_PENDING",
    completion_state: "REVALIDATION_REQUIRED",
    requirements_verified: 0,
    requirements_total: 0,
    missing_evidence: ["Rust desktop authority is required before verification can run."],
    failed_checks: [],
    skipped_checks: [],
    stale_evidence: [],
    blocked_external: ["P7 authenticated execution ledger unavailable in browser preview."],
    accepted_risks: [],
    evidence_count: 0,
    certificate: null,
    workflow_stage: "DESKTOP_AUTHORITY_REQUIRED",
    summary: "Verification is available only in the installed Relintor desktop application.",
    human_decisions: [],
    collector_activity: [],
    collection_failures: [],
    detail: "Browser preview never fabricates evidence, verification state, or certificates.",
  };
}

export async function verificationStatus(projectId: string): Promise<VerificationStatus> {
  if (!isTauriRuntime()) return browserPreviewVerification(projectId);
  return invoke<VerificationStatus>("verification_status", { projectId });
}

export async function verificationStart(projectId: string): Promise<VerificationStatus> {
  if (!isTauriRuntime()) return browserPreviewVerification(projectId);
  return invoke<VerificationStatus>("verification_start", { projectId });
}

export async function rerunVerification(projectId: string): Promise<VerificationStatus> {
  if (!isTauriRuntime()) return browserPreviewVerification(projectId);
  return invoke<VerificationStatus>("verification_rerun", { projectId });
}

export async function submitHumanDecision(
  projectId: string,
  requirementId: string,
  approved: boolean,
  notes: string,
): Promise<VerificationStatus> {
  if (!isTauriRuntime()) {
    throw new Error("The installed Relintor desktop authority is required to record a decision.");
  }
  return invoke<VerificationStatus>("verification_submit_human_decision", {
    projectId,
    requirementId,
    approved,
    notes,
  });
}

export async function verificationEvidence(projectId: string): Promise<VerificationEvidence[]> {
  if (!isTauriRuntime()) return [];
  return invoke<VerificationEvidence[]>("verification_evidence", { projectId });
}

export async function completionCertificate(projectId: string): Promise<VerificationStatus["certificate"]> {
  if (!isTauriRuntime()) return null;
  return invoke<NonNullable<VerificationStatus["certificate"]>>("verification_certificate", { projectId });
}

export async function exportVerificationManifest(projectId: string): Promise<string> {
  if (!isTauriRuntime()) return JSON.stringify(browserPreviewVerification(projectId), null, 2);
  return invoke<string>("verification_export_manifest", { projectId });
}

function browserPreviewTakeover(root: string): TakeoverView {
  return {
    project_id: "browser-preview-project",
    takeover: { id: "browser-preview-takeover", root, scanner_version: "browser-preview" },
    snapshot: { is_git_repository: false, file_count: 0, total_bytes: 0, fingerprint: { value: "browser-preview" } },
    fingerprint: "browser-preview",
    inventory: [],
    build_systems: [],
    dependency_graph: { dependencies: [], cycles: [], manifest_lock_inconsistencies: [] },
    routes: [],
    database_systems: [],
    migrations: [],
    auth_systems: [],
    ui_surfaces: [],
    test_suites: [],
    ci_workflows: [],
    deployment_targets: [],
    documentation_promises: [],
    capabilities: [],
    findings: [{ finding_type: "runtime", summary: "Browser preview cannot scan a repository; run the desktop authority scan.", severity: "info", classification: "Unproven" }],
    conflicts: [],
    recommendations: [{ target: "repository scan", kind: "InvestigateFurther", current_reality: "Unproven", priority: "P1", reason: "Browser preview has no read-only filesystem authority." }],
  };
}

function browserPreviewInvestigation(idea: string): InvestigationView {
  const cleanIdea = idea.trim();
  return {
    investigation: {
      id: "browser-preview-investigation",
      project_id: "browser-preview-project",
      status: "NeedsAnswers",
      provider: { live: false, availability: "browser_preview" },
    },
    blueprint: {
      status: "NeedsAnswers",
      product_definition: cleanIdea,
      problem_outcome: `Relintor needs more evidence to turn “${cleanIdea}” into an agreed blueprint.`,
      capabilities: ["capture project intent", "review important decisions", "approve or revise a blueprint"],
      questions: [{
        id: "browser-preview-question",
        question: "Who is the first functional user or role?",
        why_it_matters: "The first actor changes permissions, journeys, and data ownership.",
        recommended_default: "Start with one clearly named functional role.",
        options: [{ id: "single-role", label: "Single role", consequence: "Keeps the first scope narrow." }],
        can_defer: true,
        affects: ["personas", "journeys"],
        disposition: "Asked",
      }],
      conflicts: [],
      assumptions: [],
      risks: [],
      architecture_decisions: [],
      personas: [],
      journeys: [],
      non_functional_requirements: [],
      fingerprint: "browser-preview",
    },
  };
}

export function isHealthy(status: string): boolean {
  return status === "healthy" || status === "available";
}
