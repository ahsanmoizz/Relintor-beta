import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  isHealthy,
  createNewProject,
  answerInvestigation,
  readAccountState,
  signInWithGoogle,
  signOut,
  readDesktopHealth,
  readAntigravityHealth,
  readAntigravitySetupStatus,
  startAntigravitySetup,
  cancelAntigravitySetup,
  chooseAntigravityInstallLocation,
  chooseExistingAntigravityCli,
  useExistingAntigravityCli,
  signInToAntigravity,
  sealProjectMission,
  evaluateAuthority,
  type AccountState,
  type AuthorityPreview,
  type AuthorityFactDecision,
  type DesktopHealth,
  type InvestigationView,
  type InvestigatorAnswer,
  type ExecutionHandoff,
  type ExecutionStatus,
  readExecutionStatus,
  stepExecution,
  pauseExecution,
  revalidateExecution,
  retryRecoveredTask,
  stopExecution,
  continueExecution,
  verificationStatus,
  verificationStart,
  rerunVerification,
  verificationEvidence,
  completionCertificate,
  exportVerificationManifest,
  takeoverScan,
  chooseWorkspace,
  setProjectWorkspace,
  investigateTakeoverProject,
  listProjects,
  openProject,
  type TakeoverView,
  type ProjectOpenView,
  type ProjectSummary,
  type VerificationStatus,
  type VerificationEvidence,
  type AntigravityHealth,
  type AntigravitySetupView,
} from "./backend";
import { createReadinessGate } from "./readiness";
import {
  displayWindowsPath,
  displayProjectName,
  eventPresentation,
  humanStatus,
  missionPresentation,
  verificationPresentation,
} from "./activityPresentation";

export type Route = "home" | "projects" | "activity" | "account";
type ProjectEntryMode = "chooser" | "new" | "takeover" | "takeover-goal";

const routes: Array<{ id: Route; label: string; icon: string }> = [
  { id: "home", label: "Home", icon: "⌂" },
  { id: "projects", label: "Projects", icon: "▣" },
  { id: "activity", label: "Activity", icon: "◷" },
  { id: "account", label: "Account", icon: "◎" },
];

function routeFromHash(): Route {
  const value = window.location.hash.replace("#", "") as Route;
  return routes.some((route) => route.id === value) ? value : "home";
}

function initialTheme(): "light" | "dark" {
  return localStorage.getItem("relintor-theme") === "dark" ? "dark" : "light";
}

export function App() {
  const [route, setRoute] = useState<Route>(routeFromHash);
  const [theme, setTheme] = useState<"light" | "dark">(initialTheme);
  const [health, setHealth] = useState<DesktopHealth | null>(null);
  const [account, setAccount] = useState<AccountState | null>(null);
  const [investigation, setInvestigation] = useState<InvestigationView | null>(null);
  const [handoff, setHandoff] = useState<ExecutionHandoff | null>(null);
  const [projectEntry, setProjectEntry] = useState<ProjectEntryMode>("chooser");
  const [activeProject, setActiveProject] = useState<ProjectOpenView | null>(null);
  const [showOnboarding, setShowOnboarding] = useState(() => localStorage.getItem("relintor-onboarding-seen") !== "true");

  useEffect(() => {
    const onHashChange = () => setRoute(routeFromHash());
    window.addEventListener("hashchange", onHashChange);
    void readDesktopHealth().then(setHealth);
    void readAccountState().then(setAccount);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("relintor-theme", theme);
  }, [theme]);

  const navigate = (next: Route) => {
    window.location.hash = next;
    setRoute(next);
  };

  const handleHandoff = (value: ExecutionHandoff | null) => {
    setHandoff(value);
    if (value) navigate("activity");
  };

  const beginProjectEntry = (entry: Exclude<ProjectEntryMode, "chooser">) => {
    setProjectEntry(entry);
    setInvestigation(null);
    setHandoff(null);
    setActiveProject(null);
    navigate("projects");
  };

  const handleProjectOpened = (value: ProjectOpenView) => {
    setActiveProject(value);
    setInvestigation(value.investigation);
    setHandoff(value.handoff);
    if (value.handoff) navigate("activity");
  };

  const authorityText = useMemo(() => {
    if (!health) return "Authority checking";
    if (health.application.status === "unavailable") return "Browser preview";

    const coreReady =
      isHealthy(health.application.status) &&
      isHealthy(health.database.status) &&
      isHealthy(health.specification.status);

    return coreReady ? "Authority baseline healthy" : "Authority needs attention";
  }, [health]);

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>

      <header className="topbar">
        <div className="brand-lockup">
          <a className="wordmark" href="#home" aria-label="Relintor home">
            <img className="brand-logo" src="/relintor-logo.png" alt="Relintor" />
          </a>
          <span className="beta-badge">Beta</span>
        </div>
        <div className="guardian-status" aria-label={`System status: ${authorityText}`}>
          <span className="status-mark" aria-hidden="true">
            ●
          </span>
          {authorityText}
        </div>
      </header>

      <div className="shell-body">
        <aside className="sidebar" aria-label="Primary navigation">
          <nav aria-label="Primary navigation">
            {routes.map((item) => (
              <button
                key={item.id}
                className={`nav-item ${route === item.id ? "active" : ""}`}
                aria-current={route === item.id ? "page" : undefined}
                onClick={() => {
                  if (item.id === "projects") setProjectEntry("chooser");
                  navigate(item.id);
                }}
              >
                <span className="nav-icon" aria-hidden="true">
                  {item.icon}
                </span>
                {item.label}
              </button>
            ))}
          </nav>

          <div className="sidebar-footer">
            <span>v0.1.0</span>
            <span>Authority baseline</span>
          </div>
        </aside>

        <main id="main-content" className="main-content" tabIndex={-1}>
          {route === "home" && <Home onNewProject={() => beginProjectEntry("new")} onTakeover={() => beginProjectEntry("takeover")} showOnboarding={showOnboarding} onDismissOnboarding={() => { localStorage.setItem("relintor-onboarding-seen", "true"); setShowOnboarding(false); }} />}
          {route === "projects" && <Projects initialMode={projectEntry} activeProject={activeProject} investigation={investigation} handoff={handoff} onInvestigation={setInvestigation} onHandoff={handleHandoff} onProjectOpened={handleProjectOpened} />}
          {route === "activity" && <Activity handoff={handoff} />}
          {route === "account" && (
            <Account
              health={health}
              account={account}
              theme={theme}
              onThemeChange={setTheme}
            />
          )}

        </main>
      </div>
    </div>
  );
}

function Home({ onNewProject, onTakeover, showOnboarding, onDismissOnboarding }: { onNewProject: () => void; onTakeover: () => void; showOnboarding: boolean; onDismissOnboarding: () => void }) {
  return (
    <section className="view" aria-labelledby="home-title">
      <div className="eyebrow">EXECUTION AUTHORITY FOR AI-BUILT SOFTWARE</div>
      <h1 id="home-title">What are you building?</h1>
      <p className="lede">
        Relintor turns intent into a sealed, traceable mission and keeps completion accountable
        to evidence.
      </p>
      <div className="hero-proof" aria-label="Relintor promise">
        <div className="hero-object" aria-hidden="true"><span>FIELD NOTE 01</span><strong>PROOF<br />BEFORE<br />DONE</strong></div>
        <div><p className="hero-tagline">Don’t trust done. Prove it.</p><p className="muted-copy">Describe the outcome, inspect reality, seal the scope, and supervise the work until evidence matches.</p></div>
      </div>

      {showOnboarding && <FirstRunGuide onDismiss={onDismissOnboarding} />}

      <div className="action-grid">
        <button
          className="action-card primary-card"
          onClick={onNewProject}
        >
          <span className="action-symbol" aria-hidden="true">
            ＋
          </span>
          <span>
            <strong>New project</strong>
            <small>Start from an idea, brief, or documents.</small>
          </span>
          <span aria-hidden="true">→</span>
        </button>

        <button
          className="action-card"
          onClick={onTakeover}
        >
          <span className="action-symbol" aria-hidden="true">
            ↗
          </span>
          <span>
            <strong>Take over existing project</strong>
            <small>Reconstruct what a codebase actually does.</small>
          </span>
          <span aria-hidden="true">→</span>
        </button>
      </div>

      <section className="recent-section" aria-labelledby="recent-title">
        <div className="section-heading">
          <h2 id="recent-title">Recent missions</h2>
          <span className="mono-label">LOCAL ONLY</span>
        </div>
        <div className="empty-state">
          <span className="empty-mark" aria-hidden="true">
            □
          </span>
          <p>No missions yet.</p>
          <small>Your local project evidence will appear here once a mission is created.</small>
        </div>
      </section>

      <div className="principles" aria-label="Relintor workflow">
        <div>
          <strong>Describe</strong>
          <span>Make intent legible.</span>
        </div>
        <div>
          <strong>Seal</strong>
          <span>Keep scope accountable.</span>
        </div>
        <div>
          <strong>Watch</strong>
          <span>See real mission state.</span>
        </div>
        <div>
          <strong>Verify</strong>
          <span>Let evidence decide.</span>
        </div>
      </div>
      <div className="proof-strip" aria-label="Evidence promises">
        <span>Requirements accounted</span><span>Evidence attached</span><span>Safe recovery</span><span>No API keys</span>
      </div>
    </section>
  );
}

function FirstRunGuide({ onDismiss }: { onDismiss: () => void }) {
  return <section className="panel onboarding-card" aria-labelledby="onboarding-title">
    <div><span className="panel-kicker">FIRST RUN</span><h2 id="onboarding-title">A calm path from idea to proof.</h2></div>
    <p>Relintor is the authority layer for AI-built software. You can start with a new idea or take over an existing project; the read-only scan and investigator explain decisions before anything is sealed.</p>
    <div className="onboarding-steps"><span><strong>1</strong> Describe or scan</span><span><strong>2</strong> Review the blueprint</span><span><strong>3</strong> Seal with Rust authority</span><span><strong>4</strong> Watch, stop, resume, verify</span></div>
    <p className="form-hint">Antigravity is an execution adapter, not the source of truth. If an entitlement, dependency, network, or verification gate is unavailable, Relintor says so. No terminal or documentation reading is required.</p>
    <button className="secondary-button" type="button" onClick={onDismiss}>Continue to workspace</button>
  </section>;
}

function userFacingAuthorityError(reason: unknown, fallback: string): string {
  const message = String(reason);
  if (/Antigravity could not start this continuation|EXECUTOR_WORKER_SPAWN_FAILED/i.test(message)) {
    return "Antigravity could not start this continuation. Re-check Antigravity readiness, then choose Resume next turn to retry the authorized continuation.";
  }
  if (/ANTIGRAVITY_SETUP_REQUIRED|adapter execution failed closed|TASKDRIFTDENIED|production bridge|Antigravity CLI|antigravity.*(unavailable|missing|not configured)/i.test(message)) {
    return "Antigravity is not ready to run this task. Choose Set up Antigravity, complete the official installation and sign-in steps, then choose Re-check readiness.";
  }
  if (/RevalidationRequired|revalidation required|checkpoint records durable state/i.test(message)) {
    return "This mission needs a fresh safety check before it can continue. Choose Check recovery safety, then continue only if Relintor authorizes it.";
  }
  if (/AUTHORITY_READ_LEGACY_EXECUTION_STATE/i.test(message)) {
    return "Relintor found this sealed mission, but could not validate its older execution record after the update. Restart Relintor, then retry the authority read; the sealed mission and failed attempt are preserved.";
  }
  if (/AUTHORITY_READ_SEALED_MISSION_UNAVAILABLE/i.test(message)) {
    return "Relintor found the project, but could not read sealed revision 1 from the local authority. Restart Relintor and retry the authority read; do not reseal or create another revision.";
  }
  if (/AUTHORITY_READ_FAILED/i.test(message)) {
    return "Relintor could not read this sealed mission from the local authority. Restart Relintor and retry the authority read; the existing mission is preserved.";
  }
  if (/query returned no rows|load project execution scope|mission state unavailable/i.test(message)) {
    return fallback;
  }
  return /authority|execution state|ledger|sqlite|database|query/i.test(message) ? fallback : message;
}

function friendlyExecutionLabel(value: string | null | undefined, fallback: string): string {
  const normalized = (value || "").toUpperCase();
  const labels: Record<string, string> = {
    READY: "Ready to run",
    NO_RECOVERY_REQUIRED: "No recovery required",
    RUNNING: "Running",
    BLOCKED_EXTERNAL: "Waiting for required setup",
    BLOCKEDEXTERNAL: "Waiting for required setup",
    REVALIDATION_REQUIRED: "Safety re-check required",
    REVALIDATIONREQUIRED: "Safety re-check required",
    STOPPED_INCOMPLETE: "Stopped before completion",
    STOPPEDINCOMPLETE: "Stopped before completion",
    TURN_ENDED_INCOMPLETE: "Ready for next turn",
    TURNENDEDINCOMPLETE: "Ready for next turn",
    EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION: "Work finished · evidence ready",
    EXECUTIONTASKSFINISHEDAWAITINGVERIFICATION: "Work finished · evidence ready",
    COMPLETED: "Complete",
    HEALTHY: "Healthy",
    IDLE: "Idle",
    ACTIVE: "Active",
    SAFE_TO_RESUME: "Safe to resume",
    SAFE_TO_RESUME_AFTER_PROCESS_RECONCILIATION: "Safe to resume after checks",
    PRE_EXECUTION_RETRY_AUTHORIZED: "Retry authorized after a prevented attempt",
    PREEXECUTIONRETRYAUTHORIZED: "Retry authorized after a prevented attempt",
    PRE_EXECUTION_PREVENTED: "Execution was prevented before external work",
    PREEXECUTIONPREVENTED: "Execution was prevented before external work",
    INTERRUPTED_AT_SAFE_CHECKPOINT: "Safe checkpoint available",
    INTERRUPTEDWITHOUTSAFECHECKPOINT: "No safe checkpoint available",
    RECOVERY_NOT_ALLOWED: "Safe retry cannot be proven",
    RECOVERYNOTALLOWED: "Safe retry cannot be proven",
    P9_CHECKPOINT_NOT_YET_CREATED: "Checkpoint not created yet",
    TASKDRIFTDENIED: "Execution prevented",
    ADAPTEREXECUTIONFAILEDCLOSED: "Execution prevented",
    CONTINUATIONSTARTED: "Continuation started",
    CONTINUATIONAUTHORIZED: "Continuation authorized",
    SAFESTOP: "Stopped at a safe boundary",
    AUTHORITYREVALIDATIONREQUIRED: "Recovery decision recorded",
  };
  return labels[normalized] || (value ? value.replaceAll("_", " ").toLowerCase() : fallback);
}

function friendlyExecutionMessage(value: string | null | undefined, fallback: string): string {
  if (!value) return fallback;
  const normalized = value.toLowerCase();
  if (/could not prove.*owned process|executor.*after restart|marked interrupted/i.test(value)) {
    return "Relintor could not prove that the previous executor was still the same owned process after restart. The attempt was marked interrupted; choose Check recovery safety before continuing.";
  }
  if (/adapter execution failed closed|taskdriftd|production bridge|antigravity.*(unavailable|missing|not configured)/i.test(value)) {
    return "Antigravity is not ready to run this task. Complete setup, then re-check readiness before retrying.";
  }
  if (/external process started|external side effect|cannot prove.*side effect|manual recovery review|recovery decision (revalidation_required|blocked_external)/i.test(value)) {
    return "The previous Antigravity run stopped during restart recovery. Relintor found workspace changes but no trusted completion record; review them below before retrying this task.";
  }
  if (/checkpoint records durable state|revalidation|required.*safety|p6, registry, mission/i.test(normalized)) {
    return "A fresh safety check is required before this mission can continue.";
  }
  if (/pre-execution|before external process|no external work/i.test(normalized)) {
    return "The previous attempt was stopped before external work began. Relintor can authorize this same task to be retried without resealing the mission.";
  }
  if (/no trusted recovery checkpoint|safe retry cannot be proven|recovery not allowed/i.test(normalized)) {
    return "Relintor cannot prove that the previous attempt was safe to retry. The mission remains blocked until a trusted recovery boundary is available.";
  }
  return userFacingAuthorityError(value, fallback);
}

function Projects({
  initialMode,
  activeProject,
  investigation,
  handoff,
  onInvestigation,
  onHandoff,
  onProjectOpened,
}: {
  initialMode: ProjectEntryMode;
  activeProject: ProjectOpenView | null;
  investigation: InvestigationView | null;
  handoff: ExecutionHandoff | null;
  onInvestigation: (view: InvestigationView | null) => void;
  onHandoff: (handoff: ExecutionHandoff | null) => void;
  onProjectOpened: (value: ProjectOpenView) => void;
}) {
  const [mode, setMode] = useState<ProjectEntryMode>(initialMode);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [projectsBusy, setProjectsBusy] = useState(false);
  const [projectsError, setProjectsError] = useState<string | null>(null);
  const [projectsRefresh, setProjectsRefresh] = useState(0);
  const [takeover, setTakeover] = useState<TakeoverView | null>(null);
  const [takeoverPath, setTakeoverPath] = useState("");
  const [takeoverBusy, setTakeoverBusy] = useState(false);
  const [takeoverError, setTakeoverError] = useState<string | null>(null);
  const [takeoverGoal, setTakeoverGoal] = useState("");
  const [idea, setIdea] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [authorityFacts, setAuthorityFacts] = useState<AuthorityFactDecision[]>(initialAuthorityFacts);
  const [authority, setAuthority] = useState<AuthorityPreview | null>(null);
  const [authorityBusy, setAuthorityBusy] = useState(false);
  const [authorityError, setAuthorityError] = useState<string | null>(null);

  useEffect(() => setMode(initialMode), [initialMode]);

  useEffect(() => {
    if (mode !== "chooser") return;
    setProjectsBusy(true);
    setProjectsError(null);
    void listProjects()
      .then(setProjects)
      .catch((reason) => setProjectsError(String(reason)))
      .finally(() => setProjectsBusy(false));
  }, [mode, projectsRefresh]);

  useEffect(() => {
    setAuthorityFacts(activeProject?.authority_facts.length ? activeProject.authority_facts : initialAuthorityFacts());
    setAuthority(null);
    setAuthorityError(null);
  }, [activeProject?.project.project_id]);

  const refreshCanonical = async (projectId: string) => {
    try {
      onProjectOpened(await openProject(projectId));
    } catch (reason) {
      if (!String(reason).includes("Persisted project reopening is available")) throw reason;
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const view = await createNewProject(idea);
      onInvestigation(view);
      await refreshCanonical(view.investigation.project_id);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const submitTakeover = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setTakeoverError(null);
    setTakeoverBusy(true);
    try {
      setTakeover(await takeoverScan(takeoverPath));
    } catch (reason) {
      setTakeoverError(String(reason));
    } finally {
      setTakeoverBusy(false);
    }
  };

  const submitTakeoverGoal = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const projectId = takeover?.project_id ?? activeProject?.project.project_id;
    if (!projectId) return;
    setError(null);
    setBusy(true);
    try {
      const view = await investigateTakeoverProject(projectId, takeoverGoal);
      onInvestigation(view);
      await refreshCanonical(projectId);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const answerQuestions = async (answers: InvestigatorAnswer[]) => {
    if (!investigation) return;
    try {
      const view = await answerInvestigation(investigation.blueprint.product_definition, answers);
      onInvestigation(view);
      await refreshCanonical(view.investigation.project_id);
    } catch (reason) {
      setError(String(reason));
    }
  };

  const reviewAuthority = async () => {
    setAuthorityError(null);
    setAuthorityBusy(true);
    try {
      const projectId = investigation?.investigation.project_id ?? activeProject?.project.project_id ?? takeover?.project_id ?? null;
      setAuthority(await evaluateAuthority(projectId, authorityFacts));
    } catch (reason) {
      setAuthorityError(userFacingAuthorityError(reason, "The project authority could not read the persisted project. Review the project again."));
    } finally {
      setAuthorityBusy(false);
    }
  };

  const setAuthorityFact = (fact: string, decision: AuthorityFactDecision["decision"]) => {
    setAuthorityFacts((current) => current.map((value) => (value.id === fact ? { ...value, decision } : value)));
    setAuthority(null);
  };

  const openSavedProject = async (project: ProjectSummary) => {
    setProjectsError(null);
    try {
      const opened = await openProject(project.project_id);
      onProjectOpened(opened);
      setTakeover(null);
      if (opened.project.takeover_fingerprint && !opened.investigation) {
        setMode("takeover-goal");
        setTakeoverGoal("");
      } else {
        setMode("new");
      }
    } catch (reason) {
      setProjectsError(String(reason));
    }
  };

  const projectId = investigation?.investigation.project_id ?? activeProject?.project.project_id ?? takeover?.project_id ?? null;
  const workspaceRoot = takeover?.takeover.root ?? activeProject?.project.root_path ?? null;

  return (
    <section className="view" aria-labelledby="projects-title">
      <div className="eyebrow">PROJECT INTAKE</div>
      <h1 id="projects-title">
        {mode === "chooser" ? "Continue a project." : mode === "takeover" ? "Understand an existing project." : mode === "new" ? "Make the idea legible." : "Set the next project goal."}
      </h1>
      <p className="lede">
        {mode === "chooser" ? "Open a saved project at its persisted phase, or choose how you want to begin." : mode === "takeover" ? "Relintor scans the selected repository read-only, separates source facts from runtime proof, and builds a reality report." : mode === "takeover-goal" ? "The reality report is preserved. Tell Relintor what you want to change before investigation begins." : "Describe what you want to build. Relintor surfaces only decisions that can change architecture, scope, security, cost, or verification."}
      </p>

      {mode === "chooser" && <ProjectChooser projects={projects} busy={projectsBusy} error={projectsError} onRefresh={() => setProjectsRefresh((value) => value + 1)} onOpen={(project) => void openSavedProject(project)} onNew={() => setMode("new")} onTakeover={() => setMode("takeover")} />}

      {!investigation && mode === "new" && (
        <form className="panel investigator-form" onSubmit={submit}>
          <label htmlFor="project-idea">What are you building?</label>
          <textarea id="project-idea" value={idea} onChange={(event) => setIdea(event.target.value)} placeholder="A clear outcome is enough to begin. Add the users, constraints, or deployment you already know." rows={7} required />
          <div className="form-footer"><span className="form-hint">Sources remain local and are classified before any gateway context is prepared.</span><button className="primary-button" type="submit" disabled={busy || !idea.trim()}>{busy ? "Investigating…" : "Investigate idea"}</button></div>
          {error && <p className="error-text" role="alert">{error}</p>}
        </form>
      )}
      {!investigation && mode === "new" && <button className="secondary-button" type="button" onClick={() => setMode("takeover")}>Existing Project / Take Over Project</button>}

      {mode === "takeover" && !takeover && !activeProject?.project.takeover_fingerprint && <form className="panel investigator-form" onSubmit={submitTakeover}>
        <label htmlFor="takeover-path">Project folder</label>
        <input id="takeover-path" value={takeoverPath} onChange={(event) => setTakeoverPath(event.target.value)} placeholder="C:\\Projects\\existing-app" required />
        <p className="form-hint">Initial discovery is read-only. It does not install dependencies, run migrations, or execute repository scripts.</p>
        <div className="form-footer"><span className="form-hint">{takeoverBusy ? "Scanning repository · finding routes · comparing promises…" : "The scan records a workspace fingerprint and bounded evidence."}</span><button className="primary-button" type="submit" disabled={takeoverBusy || !takeoverPath.trim()}>{takeoverBusy ? "Scanning…" : "Scan project"}</button></div>
        {takeoverError && <p className="error-text" role="alert">{takeoverError}</p>}
      </form>}
      {mode === "takeover" && takeover && <TakeoverReview report={takeover} onContinue={() => setMode("takeover-goal")} onStartOver={() => { setTakeover(null); setMode("takeover"); }} />}
      {mode === "takeover-goal" && !investigation && <TakeoverGoal projectId={takeover?.project_id ?? activeProject?.project.project_id ?? ""} rootPath={workspaceRoot} value={takeoverGoal} busy={busy} error={error} onChange={setTakeoverGoal} onBack={() => setMode(takeover ? "takeover" : "chooser")} onSubmit={(event) => void submitTakeoverGoal(event)} />}
      {investigation && <BlueprintReview view={investigation} onStartOver={() => { onInvestigation(null); setMode("chooser"); }} onAnswer={answerQuestions} />}
      {investigation && !workspaceRoot && projectId && <WorkspaceSelector projectId={projectId} onUpdated={onProjectOpened} />}
      {investigation && workspaceRoot && <AuthorityReview facts={authorityFacts} preview={authority} projectId={projectId} busy={authorityBusy} error={authorityError} onSetDecision={setAuthorityFact} onReview={() => void reviewAuthority()} onHandoff={onHandoff} />}
      {handoff && projectId && <VerificationSection projectId={projectId} />}
    </section>
  );
}

function ProjectChooser({ projects, busy, error, onRefresh, onOpen, onNew, onTakeover }: { projects: ProjectSummary[]; busy: boolean; error: string | null; onRefresh: () => void; onOpen: (project: ProjectSummary) => void; onNew: () => void; onTakeover: () => void }) {
  return <section className="panel project-chooser" aria-labelledby="project-chooser-title">
    <span className="panel-kicker">YOUR PROJECTS</span>
    <h2 id="project-chooser-title">Continue where you left off</h2>
    <p className="muted-copy">Open a saved project, or start with a new idea or an existing codebase.</p>
    {busy && <p className="loading-state" role="status">Reading saved projects…</p>}
    {!busy && !projects.length && <div className="empty-inline"><strong>No saved projects yet</strong><span>Start a new project or scan an existing codebase.</span></div>}
    {projects.length > 0 && <div className="project-list">{projects.map((project) => <div className="project-list-row" key={project.project_id}><div><strong>{project.name}</strong><small>{project.root_path ? displayWindowsPath(project.root_path) : "Workspace required"} · {humanStatus(project.state, "Saved")}</small></div><button className="secondary-button" type="button" onClick={() => onOpen(project)}>Open</button></div>)}</div>}
    {error && <p className="error-text" role="alert">{error}</p>}
    <div className="result-actions"><div className="button-row"><button className="primary-button" type="button" onClick={onNew}>New project</button><button className="secondary-button" type="button" onClick={onTakeover}>Take over a project</button></div><button className="tertiary-button" type="button" onClick={onRefresh} disabled={busy}>Refresh projects</button></div>
  </section>;
}

function TakeoverGoal({ projectId, rootPath, value, busy, error, onChange, onBack, onSubmit }: { projectId: string; rootPath: string | null; value: string; busy: boolean; error: string | null; onChange: (value: string) => void; onBack: () => void; onSubmit: (event: FormEvent<HTMLFormElement>) => void }) {
  return <form className="panel investigator-form" onSubmit={onSubmit}>
    <span className="panel-kicker">NEXT GOAL</span>
    <p className="workspace-context">Workspace: <strong>{rootPath ? displayWindowsPath(rootPath) : "Workspace required"}</strong></p>
    <label htmlFor="takeover-goal">What do you want Relintor to change?</label>
    <textarea id="takeover-goal" value={value} onChange={(event) => onChange(event.target.value)} placeholder="Describe the outcome you want to change in this existing project." rows={7} required />
    <div className="form-footer"><button className="secondary-button" type="button" onClick={onBack}>Back to reality report</button><button className="primary-button" type="submit" disabled={busy || !value.trim()}>{busy ? "Investigating…" : "Investigate project"}</button></div>
    <details className="technical-details"><summary>Technical details</summary><dl><div><dt>Project ID</dt><dd><code>{projectId}</code></dd></div></dl></details>
    {error && <p className="error-text" role="alert">{error}</p>}
  </form>;
}

function WorkspaceSelector({ projectId, onUpdated }: { projectId: string; onUpdated: (value: ProjectOpenView) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choose = async () => {
    setError(null);
    setBusy(true);
    try {
      const root = await chooseWorkspace();
      if (!root) return;
      onUpdated(await setProjectWorkspace(projectId, root));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };
  return <section className="panel workspace-required" aria-labelledby="workspace-required-title">
    <span className="panel-kicker">WORKSPACE REQUIRED</span>
    <h2 id="workspace-required-title">Choose a real project workspace before standards review.</h2>
    <p className="muted-copy">A new idea does not have a filesystem root yet. Select the actual folder Relintor is allowed to inspect and execute against; the app installation folder is never used as a substitute.</p>
    <button className="primary-button" type="button" onClick={() => void choose()} disabled={busy}>{busy ? "Choosing workspace…" : "Choose workspace"}</button>
    {error && <p className="error-text" role="alert">{error}</p>}
  </section>;
}

function VerificationSection({ projectId }: { projectId: string }) {
  const [status, setStatus] = useState<VerificationStatus | null>(null);
  const [evidence, setEvidence] = useState<VerificationEvidence[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await verificationStatus(projectId));
      setEvidence(await verificationEvidence(projectId));
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "Verification is unavailable until a persisted P7 execution ledger exists."));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, [projectId]);

  const runVerification = async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await verificationStart(projectId));
      setEvidence(await verificationEvidence(projectId));
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "Verification could not read the persisted P7 execution ledger."));
    } finally {
      setBusy(false);
    }
  };

  const rerun = async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await rerunVerification(projectId));
      setEvidence(await verificationEvidence(projectId));
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "Verification could not re-read the persisted P7 execution ledger."));
    } finally {
      setBusy(false);
    }
  };

  const issueCertificate = async () => {
    setBusy(true);
    setError(null);
    try {
      const certificate = await completionCertificate(projectId);
      setStatus((current) => current ? { ...current, certificate } : current);
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "The completion certificate is unavailable until verification has a real P7 execution ledger."));
    } finally {
      setBusy(false);
    }
  };

  const exportManifest = async () => {
    try {
      const manifest = await exportVerificationManifest(projectId);
      const blob = new Blob([manifest], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${projectId}-verification-manifest.json`;
      link.click();
      URL.revokeObjectURL(url);
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "The verification manifest could not be read from the Rust authority."));
    }
  };

  const verificationView = verificationPresentation(status);
  const canStart = Boolean(status && status.execution_run_id !== "browser-preview" && status.evidence_count > 0);
  const attentionItems = [
    ...(status?.failed_checks || []),
    ...(status?.stale_evidence || []),
    ...(status?.blocked_external || []),
    ...(status?.missing_evidence || []),
  ];

  return (
    <section className="panel verification-section" aria-labelledby="verification-title">
      <span className="panel-kicker">VERIFICATION</span>
      <h2 id="verification-title">Verification</h2>
      <p className="muted-copy">Execution can finish without proving the result. Relintor calls work complete only when the required evidence passes.</p>
      {!status && !error && <p className="loading-state" role="status" aria-live="polite">Checking verification evidence…</p>}
      {status && <div className={`verification-overview tone-${verificationView.tone}`} role="status" aria-live="polite">
        <div><span className="panel-kicker">CURRENT RESULT</span><strong>{verificationView.label}</strong><p>{verificationView.supporting}</p></div>
        <div className="verification-stages" aria-label="Completion stages"><span><small>Execution</small><strong>{status.execution_run_id !== "browser-preview" ? "Recorded" : "Waiting"}</strong></span><span><small>Evidence</small><strong>{status.evidence_count > 0 ? "Captured" : "Waiting"}</strong></span><span><small>Verification</small><strong>{verificationView.verifiedComplete ? "Passed" : attentionItems.length ? "Needs attention" : "Waiting"}</strong></span></div>
      </div>}
      {attentionItems.length > 0 && <section className="attention-list" aria-labelledby="verification-attention-title"><h3 id="verification-attention-title">What needs attention</h3><ul>{attentionItems.slice(0, 5).map((item) => <li key={item}>{item}</li>)}</ul>{attentionItems.length > 5 && <p>{attentionItems.length - 5} more items are available in verification details.</p>}</section>}
      {!canStart && !verificationView.verifiedComplete && <p className="info-callout">Waiting for execution evidence.</p>}
      <div className="result-actions">
        {!verificationView.verifiedComplete && <button className="primary-button" type="button" disabled={busy || !canStart} onClick={() => void runVerification()}>{busy ? "Verifying…" : "Verify work"}</button>}
        {verificationView.verifiedComplete && !status?.certificate && <button className="primary-button" type="button" disabled={busy} onClick={() => void issueCertificate()}>{busy ? "Issuing certificate…" : "Issue completion certificate"}</button>}
        {verificationView.verifiedComplete && status?.certificate && <span className="success-text">Completion certificate issued.</span>}
        <details className="advanced-controls"><summary>Verification details</summary><div className="verification-details"><ResultPanel title="Missing evidence" items={status?.missing_evidence || []} empty="No missing evidence reported." /><ResultPanel title="Failed checks" items={status?.failed_checks || []} empty="No failed checks reported." /><ResultPanel title="Skipped checks" items={status?.skipped_checks || []} empty="No skipped checks reported." /><ResultPanel title="Stale evidence" items={status?.stale_evidence || []} empty="No stale evidence reported." /><ResultPanel title="External blockers" items={status?.blocked_external || []} empty="No external blocker reported." /><ResultPanel title="Accepted risks" items={status?.accepted_risks || []} empty="No accepted risks recorded." />{evidence.length ? <div className="drilldown-list">{evidence.map((item) => <details key={item.evidence_id}><summary>{item.evidence_id} · {humanStatus(item.result)}</summary><dl><div><dt>Class</dt><dd>{item.class}</dd></div><div><dt>Confidence</dt><dd>{item.confidence}</dd></div><div><dt>Digest</dt><dd>{item.digest}</dd></div></dl></details>)}</div> : <p className="muted-copy">No persisted evidence artifacts are available.</p>}<div className="advanced-control-row"><button className="secondary-button" type="button" disabled={busy || !canStart} onClick={() => void rerun()}>Run verification again</button><button className="secondary-button" type="button" disabled={busy} onClick={() => void refresh()}>Refresh status</button><button className="secondary-button" type="button" disabled={busy} onClick={() => void exportManifest()}>Export evidence manifest</button></div>{status?.detail && <p className="form-hint">{status.detail}</p>}{status?.certificate && <dl className="technical-list"><div><dt>Certificate</dt><dd>{status.certificate.certificate_id}</dd></div><div><dt>Digest</dt><dd>{status.certificate.digest}</dd></div></dl>}</div></details>
      </div>
      {error && <p className="error-text" role="alert">{error}</p>}
    </section>
  );
}

const authorityFactOptions = [
  ["web", "Web frontend"],
  ["backend", "Backend or API"],
  ["database", "Database or migrations"],
  ["authentication", "Authentication or permissions"],
  ["ui_surface", "User-facing interface"],
  ["seo_relevance", "Public search/discoverability"],
  ["performance", "Measured performance target"],
  ["deployment", "Deployment or release path"],
  ["observability", "Observability or operations"],
  ["privacy", "Privacy or sensitive data"],
  ["payments", "Payments or financial workflows"],
  ["ai", "AI or machine learning"],
  ["blockchain", "Blockchain or Web3"],
  ["mobile", "Mobile application"],
  ["desktop", "Desktop or local-first app"],
  ["data_engineering", "Data engineering or pipelines"],
  ["integrations", "Third-party integrations"],
] as const;

function initialAuthorityFacts(): AuthorityFactDecision[] {
  return authorityFactOptions.map(([id]) => ({ id, decision: "not_sure" }));
}

function AuthorityReview({
  facts,
  preview,
  projectId,
  busy,
  error,
  onSetDecision,
  onReview,
  onHandoff,
}: {
  facts: AuthorityFactDecision[];
  preview: AuthorityPreview | null;
  projectId: string | null;
  busy: boolean;
  error: string | null;
  onSetDecision: (fact: string, decision: AuthorityFactDecision["decision"]) => void;
  onReview: () => void;
  onHandoff: (handoff: ExecutionHandoff | null) => void;
}) {
  const [sealBusy, setSealBusy] = useState(false);
  const [sealError, setSealError] = useState<string | null>(null);
  const seal = async () => {
    if (!projectId) return;
    setSealBusy(true);
    setSealError(null);
    try {
      if (!preview) return;
      const sealedHandoff = await sealProjectMission(projectId, preview.review_digest);
      onHandoff(sealedHandoff);
    } catch (reason) {
      setSealError(userFacingAuthorityError(reason, "Relintor could not seal this mission because its execution workspace is not available. Review the project scope before trying again."));
    } finally {
      setSealBusy(false);
    }
  };
  return (
    <section className="panel authority-review" aria-labelledby="authority-review-title">
      <span className="panel-kicker">PROFESSIONAL STANDARDS</span>
      <h2 id="authority-review-title">Review what this project requires.</h2>
      <p className="muted-copy">
        Choose the project surfaces that are genuinely present. Relintor will explain the
        standards that apply and keep the others visible as not applicable.
      </p>
      <div className="authority-facts" aria-label="Project surfaces">
        {authorityFactOptions.map(([id, label]) => (
          <label className="authority-fact" key={id}>
            <span>{label}</span>
            <select
              aria-label={`${label} applicability`}
              value={facts.find((fact) => fact.id === id)?.decision || "not_sure"}
              onChange={(event) => onSetDecision(id, event.target.value as AuthorityFactDecision["decision"])}
            >
              <option value="yes">Yes</option>
              <option value="no">No</option>
              <option value="not_sure">Not sure</option>
            </select>
          </label>
        ))}
      </div>
      <div className="form-footer">
        <span className="form-hint">The authority process evaluates structured facts, not renderer-written rules.</span>
        <button className="primary-button" type="button" onClick={onReview} disabled={busy || !facts.length}>
          {busy ? "Reviewing standards…" : "Review professional standards"}
        </button>
      </div>
      {error && <p className="error-text" role="alert">{error}</p>}
      {preview && <AuthorityReviewResult preview={preview} />}
      {sealError && <p className="error-text" role="alert">{sealError}</p>}
      {preview && <div className="result-actions">
        <span>Sealing creates the immutable execution contract for this revision. Later changes require a new revision.</span>
        <button
          className="primary-button"
          type="button"
          onClick={() => void seal()}
          disabled={sealBusy || !projectId || preview.blockers.length > 0}
        >
          {sealBusy ? "Sealing mission…" : "Seal mission"}
        </button>
      </div>}
    </section>
  );
}

function AuthorityReviewResult({ preview }: { preview: AuthorityPreview }) {
  const applicable = preview.packs.filter((pack) => pack.applicable_rules > 0);
  const notApplicable = preview.packs.filter((pack) => pack.applicable_rules === 0);
  return (
    <div className="authority-results" aria-live="polite">
      <div className="authority-summary">
        <span><strong>{preview.applicable_rules}</strong> standards selected</span>
        <span><strong>{preview.requirements.length}</strong> requirements to prove</span>
        <span><strong>{preview.task_count}</strong> execution tasks</span>
      </div>
      <p className="muted-copy">Relintor selected the standards that match this project and translated them into requirements and tasks. Review the applicable items before sealing.</p>
      <div className="result-grid standards-primary">
        <ResultPanel
          title="Applicable standards"
          items={applicable.map((pack) => `${pack.title}: ${pack.explanation}`)}
          empty="No applicable standards were selected."
        />
        <ResultPanel
          title="Requirements to prove"
          items={preview.requirements.map((requirement) => `${requirement.title} — ${requirement.why_required}`)}
          empty="No implementation requirements were generated."
        />
        <ResultPanel title="Before sealing" items={preview.blockers} empty="Ready to seal. No blockers were found." />
      </div>
      <details className="technical-details"><summary>Not applicable and technical evidence</summary><div className="result-grid"><ResultPanel title={`${preview.not_applicable_rules} not applicable`} items={notApplicable.map((pack) => `${pack.title}: ${pack.explanation}`)} empty="Every reviewed pack has an applicable rule." /><ResultPanel title="Evidence obligations" items={preview.requirements.map((requirement) => `${requirement.title}: ${requirement.evidence.join(", ")}`)} empty="No evidence obligations were generated." /></div></details>
    </div>
  );
}

function TakeoverReview({ report, onContinue, onStartOver }: { report: TakeoverView; onContinue: () => void; onStartOver: () => void }) {
  const counts = report.capabilities.reduce<Record<string, number>>((result, capability) => {
    result[capability.classification] = (result[capability.classification] || 0) + 1;
    return result;
  }, {});
  return <div className="investigator-results">
    <div className="result-header"><div><span className="panel-kicker">REALITY REPORT</span><h2>{displayProjectName(report.takeover.root)}</h2><p>Relintor inspected {report.snapshot.file_count} files without running project code. Review the findings, then continue with your goal.</p><p className="workspace-context">Workspace: <strong>{displayWindowsPath(report.takeover.root)}</strong></p><details className="technical-details"><summary>Scan details</summary><dl><div><dt>Fingerprint</dt><dd><code>{report.fingerprint}</code></dd></div><div><dt>Included size</dt><dd>{report.snapshot.total_bytes.toLocaleString()} bytes</dd></div></dl></details></div><div className="result-actions"><button className="primary-button" type="button" onClick={onContinue}>Continue</button><button className="secondary-button" type="button" onClick={onStartOver}>Scan another project</button></div></div>
    <div className="result-grid">
      <ResultPanel title="Capability reality" items={Object.entries(counts).map(([state, count]) => `${state}: ${count}`)} empty="No capabilities reconciled yet." />
      <ResultPanel title="Build systems" items={report.build_systems.map((system) => `${system.name}: ${system.status}`)} empty="No build system detected." />
      <ResultPanel title="Routes & APIs" items={report.routes.map((route) => `${route.method} ${route.path}: ${route.status}`)} empty="No route evidence detected." />
      <ResultPanel title="Findings" items={report.findings.map((finding) => `${finding.severity} · ${finding.finding_type}: ${finding.summary}`)} empty="No findings yet." />
      <ResultPanel title="Recommendations" items={report.recommendations.map((recommendation) => `${recommendation.priority} · ${recommendation.kind} ${recommendation.target}: ${recommendation.reason}`)} empty="No recommendations yet." />
      <ResultPanel title="Evidence boundaries" items={[`${report.documentation_promises.length} documentation promises`, `${report.test_suites.length} test suites/scripts`, `${report.ci_workflows.length} CI workflows`, `${report.ui_surfaces.length} UI surfaces`, `${report.database_systems.length} database systems`, "Static presence is not runtime proof."]} empty="No evidence categories detected." />
    </div>
  </div>;
}

function BlueprintReview({ view, onStartOver, onAnswer }: { view: InvestigationView; onStartOver: () => void; onAnswer: (answers: InvestigatorAnswer[]) => Promise<void> }) {
  const blueprint = view.blueprint;
  const [answers, setAnswers] = useState<InvestigatorAnswer[]>([]);
  const [answering, setAnswering] = useState(false);

  const submitAnswer = async (answer: InvestigatorAnswer) => {
    const next = [...answers.filter((existing) => existing.question_id !== answer.question_id), answer];
    setAnswers(next);
    setAnswering(true);
    try {
      await onAnswer(next);
    } finally {
      setAnswering(false);
    }
  };

  return (
    <div className="investigator-results">
      <div className="result-header">
        <div>
          <span className="panel-kicker">BLUEPRINT {blueprint.status.replaceAll("_", " ")}</span>
          <h2>{blueprint.product_definition}</h2>
          <p>{blueprint.problem_outcome}</p>
        </div>
        <button className="secondary-button" onClick={onStartOver}>Start another</button>
      </div>
      <div className="result-grid">
        <QuestionPanel questions={blueprint.questions.filter((question) => question.disposition !== "Suppressed")} answers={answers} onAnswer={submitAnswer} disabled={answering} />
        <ResultPanel title="Conflicts" items={blueprint.conflicts.map((conflict) => `${conflict.claim_a} / ${conflict.claim_b}: ${conflict.recommended_resolution}`)} empty="No conflicting claims detected." />
        <ResultPanel title="Assumptions" items={blueprint.assumptions.map((assumption) => `${assumption.statement} ${assumption.consequence_if_wrong}`)} empty="No defaults were required yet." />
        <ResultPanel title="Architecture decisions" items={blueprint.architecture_decisions.map((decision) => `${decision.decision_question}: ${decision.selected_option || "unresolved"} — ${decision.reason}`)} empty="No architecture decision records yet." />
        <ResultPanel title="Personas & journeys" items={[...blueprint.personas.map((persona) => `${persona.name}: ${persona.functional_role}`), ...blueprint.journeys.map((journey) => `Journey: ${journey.goal} → ${journey.success_state}`)]} empty="No functional roles inferred yet." />
        <ResultPanel title="Non-functional needs" items={blueprint.non_functional_requirements.map((requirement) => `${requirement.domain}: ${requirement.statement}`)} empty="No additional non-functional needs inferred." />
        <ResultPanel title="Risks" items={blueprint.risks.map((risk) => `${risk.severity} · ${risk.title}: ${risk.mitigation}`)} empty="No risks have been recorded by the investigator." />
        <ResultPanel title="Deferred / not applicable" items={blueprint.questions.filter((question) => question.disposition === "Suppressed" || question.can_defer).map((question) => `${question.question}: can be deferred=${question.can_defer}`)} empty="No deferred decisions are recorded." />
        <section className="panel result-panel sealing-boundary" aria-labelledby="sealing-locks-title">
          <span className="panel-kicker">BEFORE SEALING</span>
          <h3 id="sealing-locks-title">What the authority will lock</h3>
          <ul><li>Product intent, accepted decisions, assumptions, risks, and applicable standards.</li><li>Requirement scope, task order, and the review digest.</li><li>The revision used by execution and later verification.</li></ul>
          <p className="form-hint">The renderer cannot approve or seal this blueprint. Continue to the standards review below; the Rust authority must accept the facts.</p>
        </section>
      </div>
      <div className="result-actions">
        <span>Review the questions, conflicts, assumptions, risks, and decisions before the authority review.</span>
        <button className="secondary-button" type="button" onClick={() => document.getElementById("authority-review-title")?.scrollIntoView({ behavior: "smooth", block: "start" })}>Continue to standards review</button>
      </div>
    </div>
  );
}

function QuestionPanel({ questions, answers, onAnswer, disabled }: { questions: InvestigationView["blueprint"]["questions"]; answers: InvestigatorAnswer[]; onAnswer: (answer: InvestigatorAnswer) => Promise<void>; disabled: boolean }) {
  return <section className="panel result-panel question-panel"><span className="panel-kicker">Important questions</span>{questions.length ? questions.map((question) => {
    const answer = answers.find((candidate) => candidate.question_id === question.id);
    return <div className="question-card" key={question.id}><strong>{question.question}</strong><p>{question.why_it_matters}</p><small>Recommended default: {question.recommended_default}</small><div className="question-options">{question.options.map((option) => <button key={option.id} className={`secondary-button ${answer?.option_id === option.id ? "selected" : ""}`} aria-pressed={answer?.option_id === option.id} disabled={disabled} onClick={() => void onAnswer({ question_id: question.id, option_id: option.id, not_sure: false })}>{option.label}</button>)}<button className={`secondary-button ${answer?.not_sure ? "selected" : ""}`} aria-pressed={answer?.not_sure || false} disabled={disabled} onClick={() => void onAnswer({ question_id: question.id, option_id: null, not_sure: true })}>I’m not sure</button></div></div>;
  }) : <p className="muted-copy">No material ambiguity was surfaced.</p>}</section>;
}

function ResultPanel({ title, items, empty }: { title: string; items: string[]; empty: string }) {
  return <section className="panel result-panel"><span className="panel-kicker">{title}</span>{items.length ? <ul>{items.map((item) => <li key={item}>{item}</li>)}</ul> : <p className="muted-copy">{empty}</p>}</section>;
}

export function Activity({ handoff }: { handoff: ExecutionHandoff | null }) {
  const [status, setStatus] = useState<ExecutionStatus | null>(null);
  const [verification, setVerification] = useState<VerificationStatus | null>(null);
  const [antigravity, setAntigravity] = useState<AntigravityHealth | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(Boolean(handoff));
  const [error, setError] = useState<string | null>(null);
  const [authorityReadFailure, setAuthorityReadFailure] = useState(false);
  const [authorityReadAttempt, setAuthorityReadAttempt] = useState(0);
  const [authorityReadStatus, setAuthorityReadStatus] = useState<string | null>(null);
  const [revalidating, setRevalidating] = useState(false);
  const pollInFlight = useRef(false);
  const readinessGate = useRef(createReadinessGate<AntigravityHealth | null>(3_000)).current;
  const projectId = handoff?.mission_id.replace(/^mission-/, "") || null;

  const refreshAntigravity = async (force = false): Promise<AntigravityHealth | null> => {
    try {
      const value = await readinessGate.request(readAntigravityHealth, force);
      setAntigravity(value);
      return value;
    } catch {
      setAntigravity(null);
      return null;
    }
  };

  const load = async () => {
    if (!projectId) return;
    const attempt = authorityReadAttempt + 1;
    setAuthorityReadAttempt(attempt);
    setLoading(true);
    setError(null);
    setAuthorityReadFailure(false);
    setAuthorityReadStatus(`Authority read attempt ${attempt} is in progress…`);
    try {
      const executionState = await readExecutionStatus(projectId);
      setStatus(executionState);
      setAuthorityReadStatus(`Authority read attempt ${attempt} succeeded.`);
      try {
        setVerification(await verificationStatus(projectId));
      } catch {
        setVerification(null);
      }
    } catch (reason) {
      setAuthorityReadFailure(true);
      setError(userFacingAuthorityError(reason, "Relintor couldn't load this mission's execution state. The sealed mission is preserved; retry the authority read."));
      setAuthorityReadStatus(`Authority read attempt ${attempt} completed without a usable mission snapshot.`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    setStatus(null);
    setVerification(null);
    if (projectId) void load();
  }, [projectId]);

  useEffect(() => {
    let active = true;
    const refresh = () => {
      if (active) void refreshAntigravity();
    };
    refresh();
    const timer = window.setInterval(refresh, 15_000);
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      active = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  useEffect(() => {
    if (!projectId || !status) return;
    let active = true;
    const poll = async () => {
      if (document.visibilityState === "hidden" || pollInFlight.current) return;
      pollInFlight.current = true;
      try {
        const current = await readExecutionStatus(projectId);
        if (active) {
          setStatus(current);
          // Verification becomes meaningful only after the authenticated P7
          // run reaches its terminal evidence boundary. Poll it from the same
          // single-flight loop so the user never has to refresh by hand.
          if (current.state === "ExecutionTasksFinishedAwaitingVerification") {
            try {
              const value = await verificationStatus(projectId);
              if (active) setVerification(value);
            } catch {
              if (active) setVerification(null);
            }
          }
        }
      } catch {
        // The durable worker will publish a terminal or recovery state. A
        // transient read failure must not invent completion or clear RUNNING.
      } finally {
        pollInFlight.current = false;
      }
    };
    const interval = status.state.toUpperCase() === "RUNNING" || status.dispatch_active ? 2_000 : 12_000;
    const timer = window.setInterval(() => void poll(), interval);
    const onVisible = () => {
      if (document.visibilityState === "visible") void poll();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      active = false;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [projectId, status?.dispatch_active, status?.state]);

  const runCommand = async (command: (id: string) => Promise<ExecutionStatus>) => {
    if (!projectId) return;
    const isRevalidation = command === revalidateExecution;
    if (isRevalidation) setRevalidating(true);
    setBusy(true);
    setError(null);
    setAuthorityReadFailure(false);
    try {
      setStatus(await command(projectId));
      try {
        setVerification(await verificationStatus(projectId));
      } catch {
        setVerification(null);
      }
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "Relintor couldn't apply that action. No completion state was changed; refresh the status and try again."));
    } finally {
      setBusy(false);
      if (isRevalidation) setRevalidating(false);
    }
  };

  const runVerificationCommand = async () => {
    if (!projectId) return;
    setBusy(true);
    setError(null);
    try {
      setVerification(await verificationStart(projectId));
    } catch (reason) {
      setError(userFacingAuthorityError(reason, "Relintor couldn't verify this work. The execution record is preserved; review the verification details before retrying."));
    } finally {
      setBusy(false);
    }
  };

  return <section className="view" aria-labelledby="activity-title">
    <div className="eyebrow">MISSION ACTIVITY</div>
    <h1 id="activity-title">{status?.project_name || "Activity"}</h1>
    <p className="lede">Follow the current task, respond when Relintor needs you, and verify the evidence before calling the work complete.</p>
    {handoff && <AntigravitySetupCard health={antigravity} onRefresh={refreshAntigravity} />}
    {!handoff && <div className="panel empty-panel" data-testid="activity-empty"><span className="panel-kicker">NO SEALED MISSION</span><h2>Nothing is ready to execute.</h2><p>Review a blueprint and let the Rust authority seal a mission first. There is no frontend-only activity to display.</p></div>}
    {handoff && loading && <div className="panel loading-panel" role="status" aria-live="polite"><span className="panel-kicker">MISSION STATE</span><h2>Reading persisted state…</h2><p>Waiting for the Rust execution and verification authorities.</p></div>}
    {handoff && !loading && status && <MissionCockpit status={status} verification={verification} antigravity={antigravity} busy={busy} revalidating={revalidating} onCommand={runCommand} onVerify={() => void runVerificationCommand()} onRefresh={() => void load()} />}
    {handoff && !loading && !status && !error && <div className="panel empty-panel"><span className="panel-kicker">STATE UNAVAILABLE</span><h2>No mission state returned.</h2><p>The authority did not provide an execution record, so the UI will not infer one.</p></div>}
    {error && <div className="panel error-panel" role="alert"><span className="panel-kicker">ACTION REQUIRED</span><h2>{/Antigravity is not ready/i.test(error) ? "Antigravity setup required" : authorityReadFailure ? "Mission state unavailable" : "Relintor needs your attention"}</h2><p>{error}</p>{authorityReadFailure && <button className="secondary-button" type="button" onClick={() => void load()}>Retry authority read</button>}{authorityReadFailure && authorityReadStatus && <p className="form-hint" role="status">{authorityReadStatus}</p>}</div>}
  </section>;
}

function formatBytes(value: number | null): string {
  if (value === null) return "Unavailable";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let amount = value;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount.toFixed(unit === 0 || Number.isInteger(amount) ? 0 : 1)} ${units[unit]}`;
}

function setupStageLabel(stage: string): string {
  return {
    CHECKING_SYSTEM: "Checking this Windows system",
    CHECKING_EXISTING_INSTALL: "Checking existing Antigravity installation",
    CHECKING_STORAGE: "Checking available storage",
    PREPARING_STORAGE: "Preparing the selected storage location",
    DOWNLOADING_INSTALLER: "Downloading the official installer",
    INSTALLING: "Installing the official Antigravity CLI",
    VERIFYING_EXECUTABLE: "Verifying the CLI executable",
    CHECKING_VERSION: "Checking supported CLI version",
    AUTH_REQUIRED: "Sign-in required",
    AUTH_STARTING: "Starting official Antigravity sign-in",
    AUTH_WAITING_FOR_BROWSER: "Waiting for the official sign-in page",
    AUTH_WAITING_FOR_USER: "Complete sign-in in the official Antigravity window",
    AUTH_VERIFYING: "Verifying Antigravity authentication",
    AUTH_READY: "Antigravity authentication ready",
    AUTH_FAILED: "Antigravity sign-in needs attention",
    AUTH_CANCELLED: "Antigravity sign-in cancelled",
    AUTH_TIMED_OUT: "Antigravity sign-in timed out",
    PREPARING_RELINTOR_BRIDGE: "Preparing the verified Relintor bridge",
    VERIFYING_BRIDGE_SIGNATURE: "Verifying the bridge signature and files",
    INSTALLING_RELINTOR_BRIDGE: "Installing the verified Relintor bridge",
    VERIFYING_INSTALLED_BRIDGE: "Verifying the installed bridge",
    TESTING_ADAPTER: "Testing Antigravity plugin discovery",
    VERIFYING_ADAPTER: "Verifying the Relintor adapter",
    BRIDGE_FAILED: "Verified bridge setup needs attention",
    READY: "Ready for execution",
    FAILED: "Setup needs attention",
    CANCELLED: "Setup cancelled",
  }[stage] || "Checking Antigravity readiness";
}

function AntigravitySetupCard({ health, onRefresh }: { health: AntigravityHealth | null; onRefresh: (force?: boolean) => Promise<AntigravityHealth | null> | AntigravityHealth | null }) {
  const [current, setCurrent] = useState<AntigravityHealth | null>(health);
  const [setup, setSetup] = useState<AntigravitySetupView | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const setupStatusGate = useRef(createReadinessGate<AntigravitySetupView>(500)).current;

  useEffect(() => setCurrent(health), [health]);

  const refreshSetup = async (force = false) => {
    try {
      const value = await setupStatusGate.request(readAntigravitySetupStatus, force);
      setSetup(value);
      if (value.storage_path) setSelectedPath((existing) => existing || value.storage_path);
      return value;
    } catch {
      setError("Relintor could not read Antigravity setup status. Try again from the installed desktop app.");
      return null;
    }
  };

  const waitForSetupCompletion = async () => {
    for (let attempt = 0; attempt < 660; attempt += 1) {
      const value = await refreshSetup(true);
      if (!value?.active) return value;
      await new Promise((resolve) => window.setTimeout(resolve, 1_000));
    }
    return await refreshSetup(true);
  };

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    const refresh = async () => {
      const value = await refreshSetup();
      if (!active) return;
      timer = window.setTimeout(refresh, value?.active ? 1_000 : 5_000);
    };
    void refresh();
    return () => {
      active = false;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, []);

  const startSetup = async () => {
    setBusy(true);
    setError(null);
    try {
      setSetup(await startAntigravitySetup(selectedPath || setup?.storage_path || null));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const cancelSetup = async () => {
    setBusy(true);
    try {
      setSetup(await cancelAntigravitySetup());
    } finally {
      setBusy(false);
    }
  };

  const chooseLocation = async () => {
    const path = await chooseAntigravityInstallLocation();
    if (path) setSelectedPath(path);
  };

  const chooseExisting = async () => {
    const path = await chooseExistingAntigravityCli();
    if (!path) return;
    setBusy(true);
    setError(null);
    try {
      setSetup(await useExistingAntigravityCli(path));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const signIn = async () => {
    setBusy(true);
    setError(null);
    try {
      setSetup(await signInToAntigravity());
      await waitForSetupCompletion();
      await onRefresh(true);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const recheck = async () => {
    setBusy(true);
    setError(null);
    try {
      const value = await onRefresh(true);
      if (value) setCurrent(value);
      await refreshSetup(true);
    } finally {
      setBusy(false);
    }
  };

  if (!current) {
    return <section className="panel executor-panel executor-checking" role="status" aria-live="polite"><span className="panel-kicker">ANTIGRAVITY</span><h2>Checking executor readiness…</h2><p>Relintor is checking the CLI, sign-in, and signed connection before enabling execution.</p></section>;
  }

  if (current.adapter_ready) {
    return <section className="executor-ready-compact" role="status" aria-label="Antigravity ready"><span className="status-dot tone-success" aria-hidden="true">●</span><strong>Antigravity</strong><span>Ready</span><button className="tertiary-button" type="button" onClick={() => void recheck()} disabled={busy}>{busy ? "Checking…" : "Check again"}</button></section>;
  }

  const setupView = setup;
  const setupReady = current.adapter_ready || setupView?.adapter_ready === true;
  const setupActive = setupView?.active === true;
  const setupNeedsAuth = ["AUTH_REQUIRED", "AUTH_FAILED", "AUTH_CANCELLED", "AUTH_TIMED_OUT"].includes(setupView?.stage || "") || current.authentication_status === "sign_in_required";
  const bridgeStatus = current.adapter_ready ? "Ready" : setupView?.stage === "BRIDGE_FAILED" ? "Failed" : "Not verified";
  const setupActionLabel = current.authentication_status === "ready" && !current.adapter_ready ? "Retry bridge setup" : "Set up Antigravity";

  return <section className="panel executor-panel" role="status" aria-live="polite" aria-labelledby="antigravity-setup-title">
    <div className="executor-heading"><div><span className="panel-kicker">EXECUTOR READINESS</span><h2 id="antigravity-setup-title">Antigravity setup required</h2></div><span className="readiness-badge readiness-required">SETUP REQUIRED</span></div>
    <p>{current.detail} Relintor will keep task execution disabled until every required check passes.</p>
    <div className="readiness-grid"><span>IDE installed</span><strong>{friendlyExecutionLabel(current.ide_status, "Not verified")}</strong><span>CLI installed</span><strong>{friendlyExecutionLabel(current.cli_status, "Not verified")}</strong><span>CLI authentication</span><strong>{friendlyExecutionLabel(current.authentication_status, "Not verified")}</strong><span>Executable detectable</span><strong>{current.executable_detectable ? "Yes" : "No"}</strong><span>Signed bridge</span><strong>{bridgeStatus}</strong><span>Relintor adapter</span><strong>Not ready</strong></div>
    <div className="executor-actions"><button className="primary-button" type="button" onClick={() => void startSetup()} disabled={busy || setupActive || !setupView?.automatic_install_available}>{busy || setupActive ? "Setting up…" : setupActionLabel}</button><button className="secondary-button" type="button" onClick={() => void chooseLocation()} disabled={busy || setupActive}>Choose location</button><button className="secondary-button" type="button" onClick={() => void chooseExisting()} disabled={busy || setupActive}>Use existing CLI</button><button className="secondary-button" type="button" onClick={() => void recheck()} disabled={busy}>{busy ? "Checking…" : "Re-check readiness"}</button></div>
    {setupView && <div className="setup-plan" aria-live="polite"><span className="panel-kicker">ANTIGRAVITY SETUP</span><h3>{setupStageLabel(setupView.stage)}</h3><p>{setupView.detail}</p><div className="setup-progress" aria-label={`Antigravity setup: ${setupStageLabel(setupView.stage)}`}><span>{setupStageLabel(setupView.stage)}</span>{setupView.progress_indeterminate ? <span className="progress-indeterminate" /> : <span className="progress-track"><span style={{ width: `${setupView.progress_percent ?? 0}%` }} /></span>}</div><dl><div><dt>What will be installed</dt><dd>{setupView.automatic_install_available ? "The official Antigravity CLI from antigravity.google over HTTPS." : "No installation is available in the browser preview."}</dd></div><div><dt>Selected location</dt><dd>{selectedPath || setupView.storage_path || "Choose a location"}</dd></div><div><dt>Storage policy</dt><dd>{setupView.storage_detail}</dd></div><div><dt>Safety reserve</dt><dd>At least {formatBytes(setupView.c.safe_minimum_bytes)}</dd></div><div><dt>C drive available</dt><dd>{formatBytes(setupView.c.available_bytes)}</dd></div><div><dt>D drive available</dt><dd>{formatBytes(setupView.d.available_bytes)}</dd></div><div><dt>Local app data</dt><dd>{setupView.local_appdata_state.replaceAll("_", " ")}</dd></div></dl>{setupActive && <button className="secondary-button" type="button" onClick={() => void cancelSetup()} disabled={busy}>Cancel setup</button>}{setupNeedsAuth && <button className="primary-button" type="button" onClick={() => void signIn()} disabled={busy || setupActive}>{busy || setupActive ? "Waiting…" : "Sign in to Antigravity"}</button>}{setupReady && <p className="success-text">Antigravity is ready. Return to the recovery review to continue safely.</p>}<details><summary>Advanced details</summary><p className="mono-wrap">{setupView.advanced_details}</p></details></div>}
    {error && <p className="error-text" role="alert">{error}</p>}
  </section>;
}

function MissionCockpit({ status, verification, antigravity, busy, revalidating, onCommand, onVerify, onRefresh }: { status: ExecutionStatus; verification: VerificationStatus | null; antigravity: AntigravityHealth | null; busy: boolean; revalidating: boolean; onCommand: (command: (id: string) => Promise<ExecutionStatus>) => Promise<void>; onVerify: () => void; onRefresh: () => void }) {
  const executorReady = antigravity?.adapter_ready === true;
  const presentation = missionPresentation(status, verification, executorReady);
  const verificationView = verificationPresentation(verification);
  const running = status.execution_phase === "RUNNING";
  const dispatching = status.dispatch_active && !running;
  const currentTaskId = status.active_task || status.recovery_task_id || status.runnable_tasks[0] || null;
  const currentTask = status.active_task
    ? status.current_task_objective || "Current authorized task"
    : status.recovery_task_objective || status.current_task_objective || (currentTaskId ? "Next authorized task" : "No runnable task is available");
  const taskNumber = Math.min(status.total_tasks, status.finished_tasks + 1);
  const remainingTasks = Math.max(0, status.total_tasks - status.finished_tasks - (running ? 1 : 0));
  const recoveryMessage = revalidating
    ? "Checking recovery safety…"
    : status.resume_blocker
      ? friendlyExecutionMessage(status.resume_blocker, "Relintor needs a fresh safety decision before this mission can continue.")
      : status.recovery_state.toUpperCase() === "NO_RECOVERY_REQUIRED"
        ? "No previous attempt is recorded for this mission."
        : status.last_safe_checkpoint
          ? "A trusted checkpoint is available for review."
          : "Relintor has not authorized another attempt yet.";
  const verificationBlocked = verification?.blocked_external || [];
  const recentEvents = status.events.slice(-5).reverse();
  const verifying = busy && status.state === "ExecutionTasksFinishedAwaitingVerification";
  const completedTaskMessage =
    !running && status.finished_tasks > 0 && status.finished_tasks < status.total_tasks
      ? `Task ${status.finished_tasks} complete. ${status.total_tasks - status.finished_tasks} ${status.total_tasks - status.finished_tasks === 1 ? "task remains" : "tasks remain"}${dispatching ? ". Relintor is starting the next authorized task automatically." : "."}`
      : null;

  const primaryControl = (() => {
    if (presentation.primaryAction === "run") return <button className="primary-button" type="button" disabled={busy} onClick={() => void onCommand(stepExecution)}>Run next task</button>;
    if (presentation.primaryAction === "continue") return <button className="primary-button" type="button" disabled={busy} onClick={() => void onCommand(continueExecution)}>Continue mission</button>;
    if (presentation.primaryAction === "recover") {
      const manualRetry = status.recovery_action === "MANUAL_REVIEW_RETRY";
      return <button className="primary-button" type="button" disabled={busy || dispatching} onClick={() => void onCommand(manualRetry ? retryRecoveredTask : revalidateExecution)}>{manualRetry ? (busy ? "Authorizing reviewed retry…" : "Review changes and retry this task") : (revalidating ? "Checking recovery safety…" : "Check recovery safety")}</button>;
    }
    if (presentation.primaryAction === "verify") return <button className="primary-button" type="button" disabled={busy} onClick={onVerify}>{verifying ? "Capturing evidence and checking requirements…" : "Verify work"}</button>;
    if (presentation.primaryAction === "view_verification") return <button className="primary-button" type="button" onClick={() => document.getElementById("mission-verification-details")?.scrollIntoView({ behavior: "smooth", block: "start" })}>View verification</button>;
    return null;
  })();

  return <div className="cockpit" aria-label="Mission activity">
    <section className={`mission-status-hero tone-${presentation.tone}`} role="status" aria-live="polite" aria-atomic="true">
      <div><span className="panel-kicker">CURRENT STATUS</span><h2>{verifying ? "Capturing evidence and checking requirements" : presentation.headline}</h2><p>{verifying ? "Relintor is authenticating evidence from this exact execution attempt before evaluating the sealed requirements." : presentation.supporting}</p></div>
      <span className={`status-badge tone-${presentation.tone}`}>{presentation.badge}</span>
    </section>

    <section className="panel current-task-panel" aria-labelledby="current-task-title">
      <div className="task-heading"><div><span className="panel-kicker">{running ? "CURRENT TASK" : "NEXT TASK"}</span><h3 id="current-task-title">{currentTask}</h3></div>{status.total_tasks > 0 && <span className="task-position">Task {taskNumber} of {status.total_tasks}</span>}</div>
      <p>{status.finished_tasks} completed · {running ? "1 running · " : ""}{remainingTasks} remaining</p>
      {completedTaskMessage && <p className="success-text">{completedTaskMessage}</p>}
      {(running || currentTaskId) && <p className="form-hint">Relintor continues while the executor shows healthy progress and ownership. Watchdog, no-progress, lease, and adapter safety boundaries still stop an unsafe or stalled attempt.</p>}
      <div className="primary-action-row">{primaryControl}{presentation.primaryAction === "setup" && <span className="action-guidance">Use Set up Antigravity above. Relintor will enable execution after readiness is verified.</span>}{running && <button className="danger-button" type="button" disabled={busy} onClick={() => void onCommand(stopExecution)}>Stop safely</button>}</div>
    </section>

    {presentation.recoveryRequired && <section className="panel attention-panel" aria-labelledby="recovery-title"><span className="panel-kicker">RECOVERY</span><h3 id="recovery-title">Review the interrupted task before continuing</h3><p>{recoveryMessage}</p>{status.external_changes.length > 0 && <ul>{status.external_changes.map((path) => <li key={path}>Workspace change to review: {displayWindowsPath(path)}</li>)}</ul>}<p className="form-hint">Relintor will not retry automatically or claim the workspace is unchanged. A reviewed retry creates a new exact attempt and preserves this history.</p></section>}

    {(verificationBlocked.length > 0 || (verification && verificationView.tone === "warning")) && <section className="panel attention-panel" aria-labelledby="verification-blocker-title"><span className="panel-kicker">VERIFICATION</span><h3 id="verification-blocker-title">Verification needs attention</h3><p>{verificationView.supporting}</p>{verificationBlocked.length > 0 && <ul>{verificationBlocked.slice(0, 4).map((item) => <li key={item}>{item}</li>)}</ul>}</section>}

    <div className="mission-summary-grid" aria-label="Mission summary">
      <section><span>Progress</span><strong>{status.finished_tasks} of {status.total_tasks} complete</strong><small>{status.runnable_tasks.length} ready to run</small></section>
      <section><span>Executor</span><strong>{executorReady ? "Antigravity ready" : "Setup required"}</strong><small>{running ? "Working now" : humanStatus(status.watchdog_state, "Waiting")}</small></section>
      <section><span>Verification</span><strong>{verificationView.label}</strong><small>{verificationView.supporting}</small></section>
    </div>

    <section className="panel activity-log" aria-labelledby="activity-log-title">
      <div className="section-heading"><div><span className="panel-kicker">RECENT ACTIVITY</span><h2 id="activity-log-title">What happened</h2></div><span className="auto-updated">Updates automatically</span></div>
      {recentEvents.length ? <ol className="event-timeline">{recentEvents.map((event) => { const copy = eventPresentation(event.kind, event.detail); return <li key={`${event.sequence}-${event.kind}`}><div><strong>{copy.title}</strong><span>{new Date(event.occurred_at_ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span></div><p>{copy.detail}</p></li>; })}</ol> : <div className="empty-inline"><strong>No activity yet</strong><span>Run the first task when you are ready.</span></div>}
      {status.events.length > 5 && <details className="technical-details"><summary>View full authority timeline</summary><ol className="event-timeline full-timeline">{status.events.slice().reverse().map((event) => { const copy = eventPresentation(event.kind, event.detail); return <li key={`full-${event.sequence}-${event.kind}`}><div><strong>{copy.title}</strong><span>Sequence {event.sequence}</span></div><p>{copy.detail}</p><small>{event.kind}{event.task_id ? ` · ${event.task_id}` : ""}</small></li>; })}</ol></details>}
    </section>

    <details id="mission-verification-details" className="panel technical-details mission-details"><summary>Mission details</summary><div className="mission-detail-grid"><div><span>Revision</span><strong>{status.revision}</strong></div><div><span>Execution state</span><strong>{humanStatus(status.state)}</strong></div><div><span>Safe boundary</span><strong>{status.safe_boundary_reached ? "Reached" : "Not reached"}</strong></div><div><span>Verification</span><strong>{verificationView.label}</strong></div></div><dl className="technical-list"><div><dt>Mission ID</dt><dd><code>{status.mission_id}</code></dd></div>{currentTaskId && <div><dt>Task ID</dt><dd><code>{currentTaskId}</code></dd></div>}<div><dt>Project ID</dt><dd><code>{status.project_id}</code></dd></div><div><dt>Ledger path</dt><dd><code>{displayWindowsPath(status.ledger_path)}</code></dd></div><div><dt>Recovery state</dt><dd>{status.recovery_state}</dd></div><div><dt>Turn / steps / tool calls</dt><dd>{status.current_turn} / {status.execution_steps} / {status.tool_calls}</dd></div></dl></details>

    <details className="advanced-controls"><summary>Advanced controls</summary><div className="advanced-control-row"><button className="secondary-button" type="button" disabled={busy} onClick={onRefresh}>Refresh status</button>{running && <button className="secondary-button" type="button" disabled={busy || dispatching} onClick={() => void onCommand(pauseExecution)}>Pause at the next safe boundary</button>}</div></details>
  </div>;
}

function Account({
  health,
  account,
  theme,
  onThemeChange,
}: {
  health: DesktopHealth | null;
  account: AccountState | null;
  theme: "light" | "dark";
  onThemeChange: (theme: "light" | "dark") => void;
}) {
  const [privacyLocal, setPrivacyLocal] = useState(() => localStorage.getItem("relintor-privacy-local") !== "false");
  const [privacyCloud, setPrivacyCloud] = useState(() => localStorage.getItem("relintor-privacy-cloud") === "true");
  const [privacyAi, setPrivacyAi] = useState(() => localStorage.getItem("relintor-privacy-ai") === "true");
  const [diagnosticsMessage, setDiagnosticsMessage] = useState<string | null>(null);
  const [authBusy, setAuthBusy] = useState(false);
  const [authMessage, setAuthMessage] = useState<string | null>(null);
  const databaseDetail = health?.database
    ? `${health.database.detail}${
        health.database.schema_version !== null
          ? ` Schema ${health.database.schema_version}.`
          : ""
      }`
    : "Reading local database health…";

  return (
    <section className="view account-view" aria-labelledby="account-title">
      <div className="eyebrow">ACCOUNT &amp; SETTINGS</div>
      <h1 id="account-title">Account</h1>
      <p className="lede">
        Manage your sign-in, Antigravity connection, privacy choices, and appearance.
      </p>

      <details className="system-details">
        <summary>System health and performance</summary>
        <div className="health-grid">
        <HealthCard
          title="Application"
          status={health?.application.status || "checking"}
          detail={health?.application.detail || "Reading local authority health…"}
        />
        <HealthCard
          title="Local database"
          status={health?.database.status || "checking"}
          detail={databaseDetail}
        />
        <HealthCard
          title="Locked specification"
          status={health?.specification.status || "checking"}
          detail={health?.specification.detail || "Reading specification integrity…"}
        />
        <HealthCard
          title="OS keychain"
          status={health?.keychain.status || "checking"}
          detail={health?.keychain.detail || "Reading secure-store health…"}
        />
        <HealthCard
          title="Antigravity"
          status={health?.antigravity.status || "checking"}
          detail={health?.antigravity.detail || "Checking for the official CLI executable."}
        />
        </div>
        <PerformanceSummary />
      </details>
      <AntigravitySetupCard health={health?.antigravity || null} onRefresh={() => readAntigravityHealth().catch(() => null)} />

      <section className="panel account-state account-session" aria-labelledby="account-state-title">
        <span className="panel-kicker">GOOGLE ACCOUNT</span>
        <h2 id="account-state-title">
          {account?.status === "signed_in" ? `Signed in${account.email ? ` as ${account.email}` : ""}` : "Signed out"}
        </h2>
        <p>{account?.status === "signed_in" ? "Your cloud session is active." : "Sign in to use account-backed Beta services. Local project work remains available."}</p>
        {account?.status === "signed_in" ? (
          <button className="secondary-button" type="button" onClick={() => { setAuthBusy(true); setAuthMessage(null); void signOut().then(() => window.location.reload()).catch((reason) => setAuthMessage(String(reason))).finally(() => setAuthBusy(false)); }} disabled={authBusy}>
            {authBusy ? "Signing out…" : "Sign out"}
          </button>
        ) : (
          <button className="primary-button" type="button" onClick={() => { setAuthBusy(true); setAuthMessage("Opening secure browser sign-in…"); void signInWithGoogle().then(() => window.location.reload()).catch((reason) => setAuthMessage(String(reason))).finally(() => setAuthBusy(false)); }} disabled={authBusy}>
            {authBusy ? "Waiting for Google…" : "Continue with Google"}
          </button>
        )}
        {authMessage && <p className="form-hint" role="status">{authMessage}</p>}
        <div className="account-facts">
          <span>Beta access</span>
          <strong>{account?.entitlement === "VALID" ? "Active" : account?.entitlement === "GRACE" ? "Temporarily available offline" : account?.entitlement === "EXPIRED" ? "Expired" : account?.entitlement === "INVALID_SIGNATURE" ? "Needs attention" : "Not confirmed"}</strong>
        </div>
        {(!account || account.entitlement === "UNAVAILABLE") && <p className="form-hint">Beta access has not been confirmed by the cloud service. This does not change the truth of local execution or verification records.</p>}
        {account?.plan && (
          <div className="account-facts">
            <span>Plan</span>
            <strong>{account.plan}</strong>
          </div>
        )}
      </section>

      <section className="panel account-state" aria-labelledby="team-billing-title">
        <span className="panel-kicker">TEAMS &amp; BILLING</span>
        <h2 id="team-billing-title">Not available in this closed Beta</h2>
        <p>Team workspaces and billing will appear here when the production service is available.</p>
        <div className="account-facts">
          <span>Availability</span>
          <strong>Coming later</strong>
        </div>
      <small>
          Relintor does not show sample memberships, plans, or billing data.
      </small>
      </section>

      <section className="panel account-state privacy-panel" aria-labelledby="privacy-title">
        <span className="panel-kicker">PRIVACY CONTROLS</span>
        <h2 id="privacy-title">Choose what leaves this device.</h2>
        <p>These controls explain product behavior; they do not create an entitlement or override the Rust authority. Local project evidence stays local unless an authorized workflow requires a bounded handoff.</p>
        <label className="privacy-toggle"><input type="checkbox" checked={privacyLocal} onChange={(event) => { setPrivacyLocal(event.target.checked); localStorage.setItem("relintor-privacy-local", String(event.target.checked)); }} /> Keep project evidence local by default</label>
        <label className="privacy-toggle"><input type="checkbox" checked={privacyCloud} onChange={(event) => { setPrivacyCloud(event.target.checked); localStorage.setItem("relintor-privacy-cloud", String(event.target.checked)); }} /> Allow cloud account/team state when signed in</label>
        <label className="privacy-toggle"><input type="checkbox" checked={privacyAi} onChange={(event) => { setPrivacyAi(event.target.checked); localStorage.setItem("relintor-privacy-ai", String(event.target.checked)); }} /> Allow minimal-context AI gateway requests when externally available</label>
        <p className="form-hint">Provider keys, access tokens, database credentials, MFA secrets, private signing keys, and raw project content are not placed in diagnostics.</p>
      </section>

      <section className="panel account-state diagnostics-panel" aria-labelledby="diagnostics-title">
        <span className="panel-kicker">SUPPORT DIAGNOSTICS</span>
        <h2 id="diagnostics-title">Export a safe support snapshot.</h2>
        <p>The snapshot contains app version, platform, health words, adapter availability, entitlement state, and no secrets or sensitive project content.</p>
        <div className="result-actions"><button className="secondary-button" type="button" onClick={() => { const payload = JSON.stringify(buildSafeDiagnostics(health, account), null, 2); const blob = new Blob([payload], { type: "application/json" }); const url = URL.createObjectURL(blob); const link = document.createElement("a"); link.href = url; link.download = "relintor-support-diagnostics.json"; link.click(); URL.revokeObjectURL(url); setDiagnosticsMessage("Safe diagnostics exported locally."); }}>Export safe diagnostics</button><button className="secondary-button" type="button" onClick={() => { const payload = JSON.stringify(buildSafeDiagnostics(health, account), null, 2); void navigator.clipboard?.writeText(payload); setDiagnosticsMessage("Safe diagnostics copied when clipboard access is available."); }}>Copy safe diagnostics</button></div>
        {diagnosticsMessage && <p className="form-hint" role="status">{diagnosticsMessage}</p>}
      </section>

      <div className="settings-row">
        <div>
          <span className="panel-kicker">APPEARANCE</span>
          <h2>Theme</h2>
          <p>Light and dark themes preserve the paper-and-ink hierarchy.</p>
        </div>
        <div className="segmented" role="group" aria-label="Theme">
          <button
            className={theme === "light" ? "selected" : ""}
            aria-pressed={theme === "light"}
            onClick={() => onThemeChange("light")}
          >
            Light
          </button>
          <button
            className={theme === "dark" ? "selected" : ""}
            aria-pressed={theme === "dark"}
            onClick={() => onThemeChange("dark")}
          >
            Dark
          </button>
        </div>
      </div>
    </section>
  );
}

function buildSafeDiagnostics(health: DesktopHealth | null, account: AccountState | null) {
  return {
    app_version: "0.1.0",
    generated_at: new Date().toISOString(),
    platform: typeof navigator === "undefined" ? "unknown" : navigator.platform,
    user_agent: typeof navigator === "undefined" ? "unknown" : navigator.userAgent,
    health: health ? {
      application: health.application.status,
      database: health.database.status,
      specification: health.specification.status,
      keychain: health.keychain.status,
      antigravity: { status: health.antigravity.status, compatibility: health.antigravity.compatibility, cli_invocation_capability: health.antigravity.cli_invocation_capability, plugin_hook_capability: health.antigravity.plugin_hook_capability },
    } : "loading",
    account: account ? { status: account.status, plan: account.plan, entitlement: account.entitlement } : "loading",
    external_status: "P10_REAL_BILLING_PROVIDER=PENDING_EXTERNAL_ENVIRONMENT",
    redaction: ["access tokens", "provider keys", "database credentials", "MFA secrets", "private signing keys", "raw project content"],
  };
}

function PerformanceSummary() {
  const [measurement, setMeasurement] = useState<{ startupMs: number | null; resources: number; transferBytes: number | null } | null>(null);

  useEffect(() => {
    const navigation = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined;
    const resources = performance.getEntriesByType("resource") as PerformanceResourceTiming[];
    const transferBytes = resources.reduce((total, entry) => total + (entry.transferSize || 0), 0);
    setMeasurement({ startupMs: navigation?.domContentLoadedEventEnd ? Math.round(navigation.domContentLoadedEventEnd) : null, resources: resources.length, transferBytes: transferBytes || null });
  }, []);

  return <section className="panel performance-panel" aria-labelledby="performance-title"><span className="panel-kicker">MEASURED PERFORMANCE</span><h2 id="performance-title">Runtime timing</h2>{measurement ? <div className="performance-facts"><div><span>Startup</span><strong>{measurement.startupMs === null ? "UNAVAILABLE" : `${measurement.startupMs} ms`}</strong></div><div><span>Observed resources</span><strong>{measurement.resources}</strong></div><div><span>Transfer bytes</span><strong>{measurement.transferBytes === null ? "UNAVAILABLE" : measurement.transferBytes.toLocaleString()}</strong></div></div> : <p className="loading-state">Reading browser timing entries…</p>}<p className="form-hint">These are observed runtime entries, not a fabricated quality verdict. Bundle and list performance must be measured in the release environment.</p></section>;
}

function HealthCard({
  title,
  status,
  detail,
}: {
  title: string;
  status: string;
  detail: string;
}) {
  const statusClass = status.replace(/[^a-z0-9_-]/gi, "-").toLowerCase();

  return (
    <article className="health-card">
      <div className="health-title">
        <span className={`health-dot ${statusClass}`} aria-hidden="true">
          ●
        </span>
        <h2>{title}</h2>
        <span className="status-word">{status.replaceAll("_", " ")}</span>
      </div>
      <p>{detail}</p>
    </article>
  );
}
