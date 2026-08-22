import { useState, type FormEvent, type ReactNode } from "react";
import {
  createCloudApiClient,
  type CloudApiClient,
  type P10Account,
  type P10AuditEvent,
  type P10BillingState,
  type P10Entitlement,
  type P10MfaChallenge,
  type P10OrganizationMember,
  type P10TeamPolicy,
} from "@relintor/contracts/cloudApi";

type LoadState = {
  entitlement: P10Entitlement | null;
  billing: P10BillingState | null;
  members: P10OrganizationMember[] | null;
  policy: P10TeamPolicy | null;
  audit: P10AuditEvent[] | null;
  errors: Partial<Record<"entitlement" | "billing" | "members" | "policy" | "audit", string>>;
};

const emptyLoadState: LoadState = {
  entitlement: null,
  billing: null,
  members: null,
  policy: null,
  audit: null,
  errors: {},
};

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : "The cloud API returned an unknown error.";
}

export function App() {
  const [baseUrl, setBaseUrl] = useState("");
  const [accessToken, setAccessToken] = useState("");
  const [client, setClient] = useState<CloudApiClient | null>(null);
  const [account, setAccount] = useState<P10Account | null>(null);
  const [loadState, setLoadState] = useState<LoadState>(emptyLoadState);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const loadPortal = async (nextClient: CloudApiClient) => {
    setLoading(true);
    const results = await Promise.allSettled([
      nextClient.entitlement(),
      nextClient.billing(),
      nextClient.members(),
      nextClient.policy(),
      nextClient.audit(),
    ]);
    const [entitlement, billing, members, policy, audit] = results;
    setLoadState({
      entitlement: entitlement.status === "fulfilled" ? entitlement.value : null,
      billing: billing.status === "fulfilled" ? billing.value : null,
      members: members.status === "fulfilled" ? members.value : null,
      policy: policy.status === "fulfilled" ? policy.value : null,
      audit: audit.status === "fulfilled" ? audit.value : null,
      errors: {
        ...(entitlement.status === "rejected" ? { entitlement: errorMessage(entitlement.reason) } : {}),
        ...(billing.status === "rejected" ? { billing: errorMessage(billing.reason) } : {}),
        ...(members.status === "rejected" ? { members: errorMessage(members.reason) } : {}),
        ...(policy.status === "rejected" ? { policy: errorMessage(policy.reason) } : {}),
        ...(audit.status === "rejected" ? { audit: errorMessage(audit.reason) } : {}),
      },
    });
    setLoading(false);
  };

  const connect = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setConnectionError(null);
    try {
      const nextClient = createCloudApiClient(baseUrl, accessToken);
      const nextAccount = await nextClient.account();
      setClient(nextClient);
      setAccount(nextAccount);
      await loadPortal(nextClient);
    } catch (reason) {
      setClient(null);
      setAccount(null);
      setConnectionError(errorMessage(reason));
    }
  };

  const signOut = () => {
    setClient(null);
    setAccount(null);
    setAccessToken("");
    setLoadState(emptyLoadState);
    setConnectionError(null);
  };

  if (!client || !account) {
    return (
      <main className="admin-shell signed-out-shell">
        <header className="masthead">
          <div className="masthead-brand">
            <img className="brand-logo" src="/relintor-logo.png" alt="Relintor" />
            <div>
              <p className="eyebrow">P10 / FOUNDER ADMIN</p>
              <h1>Founder &amp; admin portal</h1>
            </div>
          </div>
          <span className="status-pill">SIGNED OUT</span>
        </header>
        <section className="paper-card connect-card" aria-labelledby="connect-title">
          <p className="eyebrow">SERVER-AUTHENTICATED ACCESS</p>
          <h2 id="connect-title">Connect an existing cloud session</h2>
          <p className="lede">
            This portal does not mint identity, founder status, roles, MFA success, grants, or
            entitlements. Enter a real authenticated access token; it is held in memory only.
          </p>
          <form onSubmit={connect} className="stack-form">
            <label htmlFor="cloud-url">Cloud API URL</label>
            <input
              id="cloud-url"
              type="url"
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
              placeholder="https://cloud.example.com"
              required
            />
            <label htmlFor="access-token">Authenticated access token</label>
            <input
              id="access-token"
              type="password"
              value={accessToken}
              onChange={(event) => setAccessToken(event.target.value)}
              autoComplete="off"
              required
            />
            <button className="primary-button" type="submit">
              Connect securely
            </button>
          </form>
          {connectionError && <ErrorNotice message={connectionError} />}
          <p className="form-hint">
            The account endpoint must accept the session. Admin and founder scope are resolved by
            the cloud service independently for every protected operation.
          </p>
        </section>
      </main>
    );
  }

  return (
    <main className="admin-shell">
      <header className="masthead">
        <div className="masthead-brand">
          <img className="brand-logo" src="/relintor-logo.png" alt="Relintor" />
          <div>
            <p className="eyebrow">P10 / FOUNDER ADMIN</p>
            <h1>Founder &amp; admin portal</h1>
          </div>
        </div>
        <div className="masthead-actions">
          <span className="status-pill">AUTHENTICATED SESSION</span>
          <button className="quiet-button" onClick={signOut}>Sign out</button>
        </div>
      </header>

      <section className="identity-banner paper-card" aria-labelledby="identity-title">
        <div>
          <p className="eyebrow">SERVER-RESOLVED ACCOUNT</p>
          <h2 id="identity-title">{account.organization.name}</h2>
          <p>{account.user.email} · organization role: <strong>{account.role}</strong></p>
        </div>
        <div className="authority-note">
          <strong>Authority remains on the server.</strong>
          <span>Founder/admin scope and MFA are not inferred by this renderer.</span>
        </div>
      </section>

      {loadState.errors.audit ? (
        <ErrorNotice message={`Admin API access denied or unavailable: ${loadState.errors.audit}`} />
      ) : (
        <div className="success-notice" role="status">Server returned the admin audit scope.</div>
      )}

      <div className="toolbar">
        <span>{loading ? "Refreshing server state…" : "Live server state"}</span>
        <button className="quiet-button" onClick={() => void loadPortal(client)} disabled={loading}>Refresh</button>
      </div>

      <section className="dashboard-grid">
        <ResourceCard title="Entitlement & trial state" error={loadState.errors.entitlement}>
          {loadState.entitlement ? (
            <dl className="facts">
              <div><dt>Plan</dt><dd>{loadState.entitlement.claims.plan}</dd></div>
              <div><dt>Source</dt><dd>{loadState.entitlement.claims.source}</dd></div>
              <div><dt>Expires</dt><dd>{formatTimestamp(loadState.entitlement.claims.expires_at)}</dd></div>
              <div><dt>Offline grace</dt><dd>{formatTimestamp(loadState.entitlement.claims.grace_until)}</dd></div>
            </dl>
          ) : <UnavailableState />}
        </ResourceCard>

        <ResourceCard title="Subscription & billing" error={loadState.errors.billing}>
          {loadState.billing ? (
            <>
              <p className={loadState.billing.billing.available ? "state-good" : "state-pending"}>
                {loadState.billing.billing.available ? "Provider available" : "PENDING_EXTERNAL_ENVIRONMENT"}
              </p>
              <p className="muted">{loadState.billing.billing.detail}</p>
              {loadState.billing.subscriptions.length === 0 ? <UnavailableState /> : loadState.billing.subscriptions.map((subscription) => (
                <div className="list-row" key={subscription.id}>
                  <strong>{subscription.plan} · {subscription.billing_interval}</strong>
                  <span>{subscription.state} · seats {subscription.seat_limit ?? "unlimited"}</span>
                </div>
              ))}
            </>
          ) : <UnavailableState />}
        </ResourceCard>

        <ResourceCard title="Organization members" error={loadState.errors.members}>
          {loadState.members ? (loadState.members.length === 0 ? <UnavailableState /> : loadState.members.map((member) => (
            <div className="list-row" key={member.user.id}>
              <strong>{member.user.email}</strong><span>{member.role}</span>
            </div>
          ))) : <UnavailableState />}
        </ResourceCard>

        <ResourceCard title="Shared team policy" error={loadState.errors.policy}>
          {loadState.policy ? (
            <>
              <p className="muted">Version {loadState.policy.version} · updated by {loadState.policy.updated_by}</p>
              <pre className="json-block">{JSON.stringify(loadState.policy.policy, null, 2)}</pre>
            </>
          ) : <UnavailableState />}
        </ResourceCard>
      </section>

      <MfaAndAdminOperations client={client} proofRequired={loadState.errors.audit !== undefined} onRefresh={() => void loadPortal(client)} />

      <section className="paper-card audit-card" aria-labelledby="audit-title">
        <div className="section-heading"><div><p className="eyebrow">DURABLE RECORD</p><h2 id="audit-title">Admin audit history</h2></div><button className="quiet-button" onClick={() => void loadPortal(client)} disabled={loading}>Reload audit</button></div>
        {loadState.audit ? (loadState.audit.length === 0 ? <UnavailableState /> : <div className="audit-list">{loadState.audit.map((event) => <div className="audit-row" key={event.id}><strong>{event.operation} · {event.entity_type}</strong><span>{event.outcome} · {event.reason}</span><small>{event.created_at} · request {event.request_id}</small></div>)}</div>) : <UnavailableState />}
      </section>
    </main>
  );
}

function MfaAndAdminOperations({ client, onRefresh }: { client: CloudApiClient; proofRequired: boolean; onRefresh: () => void }) {
  const [challenge, setChallenge] = useState<P10MfaChallenge | null>(null);
  const [code, setCode] = useState("");
  const [proof, setProof] = useState("");
  const [mfaMessage, setMfaMessage] = useState<string | null>(null);
  const [mfaBusy, setMfaBusy] = useState(false);
  const [grantKind, setGrantKind] = useState<"user" | "company">("user");
  const [target, setTarget] = useState("");
  const [plan, setPlan] = useState<"INDIVIDUAL" | "PRO" | "TEAM" | "ENTERPRISE">("TEAM");
  const [grantReason, setGrantReason] = useState("");
  const [expiresIn, setExpiresIn] = useState("");
  const [seatLimit, setSeatLimit] = useState("");
  const [grantMessage, setGrantMessage] = useState<string | null>(null);
  const [grantBusy, setGrantBusy] = useState(false);
  const [grantId, setGrantId] = useState("");
  const [modifyReason, setModifyReason] = useState("");
  const [modifyMessage, setModifyMessage] = useState<string | null>(null);
  const [revokeReason, setRevokeReason] = useState("");
  const [memberId, setMemberId] = useState("");
  const [memberRole, setMemberRole] = useState<"owner" | "admin" | "member">("member");
  const [memberReason, setMemberReason] = useState("");
  const [memberMessage, setMemberMessage] = useState<string | null>(null);

  const beginMfa = async () => {
    setMfaBusy(true); setMfaMessage(null);
    try { setChallenge(await client.beginMfa()); }
    catch (reason) { setMfaMessage(errorMessage(reason)); }
    finally { setMfaBusy(false); }
  };

  const verifyMfa = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setMfaBusy(true); setMfaMessage(null);
    try { const result = await client.verifyMfa(challenge?.challenge_id ?? "", code); setProof(result.proof); setMfaMessage("MFA proof accepted by the server and held in memory only."); }
    catch (reason) { setMfaMessage(errorMessage(reason)); }
    finally { setMfaBusy(false); }
  };

  const submitGrant = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setGrantMessage(null);
    if (!proof) { setGrantMessage("A fresh server-issued MFA proof is required before a grant mutation."); return; }
    if (!grantReason.trim() || !target.trim()) { setGrantMessage("A target and reason are required; the server will still validate scope."); return; }
    setGrantBusy(true);
    try {
      const input = { plan, reason: grantReason, ...(grantKind === "user" ? { user_id: target } : { organization_id: target }), ...(expiresIn ? { expires_in_seconds: Number(expiresIn) } : {}), ...(seatLimit ? { seat_limit: Number(seatLimit) } : {}) };
      const result = grantKind === "user" ? await client.createUserGrant(proof, input) : await client.createCompanyGrant(proof, input);
      const returnedId = typeof result === "object" && result !== null && "id" in result ? String((result as { id: unknown }).id) : "";
      if (returnedId) setGrantId(returnedId);
      setGrantMessage(returnedId ? `Grant mutation accepted by the server. Grant ID: ${returnedId}` : "Grant mutation accepted by the server.");
      onRefresh();
    } catch (reason) { setGrantMessage(errorMessage(reason)); }
    finally { setGrantBusy(false); }
  };

  const modifyGrant = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setModifyMessage(null);
    if (!proof) { setModifyMessage("A fresh server-issued MFA proof is required before a grant mutation."); return; }
    if (!grantId.trim() || !modifyReason.trim()) { setModifyMessage("A grant ID and reason are required."); return; }
    try { await client.modifyGrant(grantId, proof, { plan, reason: modifyReason }); setModifyMessage("Grant modification accepted by the server."); onRefresh(); }
    catch (reason) { setModifyMessage(errorMessage(reason)); }
  };

  const revokeGrant = async () => {
    setModifyMessage(null);
    if (!proof) { setModifyMessage("A fresh server-issued MFA proof is required before a grant mutation."); return; }
    if (!grantId.trim() || !revokeReason.trim()) { setModifyMessage("A grant ID and reason are required."); return; }
    try { await client.revokeGrant(grantId, proof, revokeReason); setModifyMessage("Grant revocation accepted by the server."); onRefresh(); }
    catch (reason) { setModifyMessage(errorMessage(reason)); }
  };

  const changeMember = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setMemberMessage(null);
    if (!proof) { setMemberMessage("A fresh server-issued MFA proof is required before membership changes."); return; }
    if (!memberId.trim() || !memberReason.trim()) { setMemberMessage("A member ID and reason are required."); return; }
    try { await client.changeMemberRole(memberId, proof, memberRole, memberReason); setMemberMessage("Membership role change accepted by the server."); onRefresh(); }
    catch (reason) { setMemberMessage(errorMessage(reason)); }
  };

  const removeMember = async () => {
    setMemberMessage(null);
    if (!proof) { setMemberMessage("A fresh server-issued MFA proof is required before membership changes."); return; }
    if (!memberId.trim() || !memberReason.trim()) { setMemberMessage("A member ID and reason are required."); return; }
    try { await client.removeMember(memberId, proof, memberReason); setMemberMessage("Membership removal accepted by the server."); onRefresh(); }
    catch (reason) { setMemberMessage(errorMessage(reason)); }
  };

  return <section className="paper-card operations-card" aria-labelledby="operations-title">
    <div className="section-heading"><div><p className="eyebrow">PRIVILEGED WORKFLOWS</p><h2 id="operations-title">MFA, grants &amp; founder administration</h2></div><span className={proof ? "state-good" : "state-pending"}>{proof ? "MFA PROOF ACTIVE IN MEMORY" : "MFA REQUIRED"}</span></div>
    <div className="operation-grid">
      <div className="operation-panel"><h3>Step 1 · server MFA</h3><p className="muted">The service issues and verifies the challenge. This UI never asserts success itself.</p><button className="primary-button" onClick={() => void beginMfa()} disabled={mfaBusy}>{mfaBusy ? "Requesting…" : "Request MFA challenge"}</button>{challenge && <form className="stack-form compact-form" onSubmit={verifyMfa}><span className="form-hint">Challenge expires {formatTimestamp(challenge.expires_at)}</span><label htmlFor="mfa-code">Authenticator code</label><input id="mfa-code" inputMode="numeric" value={code} onChange={(event) => setCode(event.target.value)} required /><button className="secondary-button" type="submit" disabled={mfaBusy}>Verify with server</button></form>}{mfaMessage && <p className="inline-message" role="status">{mfaMessage}</p>}</div>
      <form className="operation-panel stack-form" onSubmit={submitGrant}><h3>Step 2 · complimentary grant</h3><label htmlFor="grant-kind">Grant scope</label><select id="grant-kind" value={grantKind} onChange={(event) => setGrantKind(event.target.value as "user" | "company")}><option value="user">User grant</option><option value="company">Company grant</option></select><label htmlFor="grant-target">Target user or organization ID</label><input id="grant-target" value={target} onChange={(event) => setTarget(event.target.value)} required /><label htmlFor="grant-plan">Plan</label><select id="grant-plan" value={plan} onChange={(event) => setPlan(event.target.value as typeof plan)}><option>INDIVIDUAL</option><option>PRO</option><option>TEAM</option><option>ENTERPRISE</option></select><label htmlFor="grant-expires">Expiry seconds (optional)</label><input id="grant-expires" type="number" min="1" value={expiresIn} onChange={(event) => setExpiresIn(event.target.value)} /><label htmlFor="grant-seats">Seat limit (optional)</label><input id="grant-seats" type="number" min="0" value={seatLimit} onChange={(event) => setSeatLimit(event.target.value)} /><label htmlFor="grant-reason">Reason</label><input id="grant-reason" value={grantReason} onChange={(event) => setGrantReason(event.target.value)} required /><button className="primary-button" type="submit" disabled={grantBusy}>{grantBusy ? "Submitting…" : "Create grant"}</button>{grantMessage && <p className="inline-message" role="status">{grantMessage}</p>}</form>
      <div className="operation-panel"><h3>Step 3 · modify or revoke</h3><p className="muted">Use an ID returned by the server. No client-generated grant is treated as valid.</p><form className="stack-form compact-form" onSubmit={modifyGrant}><label htmlFor="grant-id">Grant ID</label><input id="grant-id" value={grantId} onChange={(event) => setGrantId(event.target.value)} required /><label htmlFor="modify-reason">Modification reason</label><input id="modify-reason" value={modifyReason} onChange={(event) => setModifyReason(event.target.value)} required /><button className="secondary-button" type="submit">Modify grant plan</button></form><label htmlFor="revoke-reason">Revocation reason</label><input id="revoke-reason" value={revokeReason} onChange={(event) => setRevokeReason(event.target.value)} /><button className="danger-button" onClick={() => void revokeGrant()}>Revoke grant</button>{modifyMessage && <p className="inline-message" role="status">{modifyMessage}</p>}</div>
      <div className="operation-panel"><h3>Step 4 · membership administration</h3><p className="muted">The server rechecks tenant scope and role authority for every change.</p><form className="stack-form compact-form" onSubmit={changeMember}><label htmlFor="member-id">Member user ID</label><input id="member-id" value={memberId} onChange={(event) => setMemberId(event.target.value)} required /><label htmlFor="member-role">New role</label><select id="member-role" value={memberRole} onChange={(event) => setMemberRole(event.target.value as typeof memberRole)}><option value="member">member</option><option value="admin">admin</option><option value="owner">owner</option></select><label htmlFor="member-reason">Membership reason</label><input id="member-reason" value={memberReason} onChange={(event) => setMemberReason(event.target.value)} required /><button className="secondary-button" type="submit">Change role</button></form><button className="danger-button" onClick={() => void removeMember()}>Remove member</button>{memberMessage && <p className="inline-message" role="status">{memberMessage}</p>}</div>
    </div>
  </section>;
}

function ResourceCard({ title, error, children }: { title: string; error?: string; children: ReactNode }) {
  return <article className="paper-card resource-card"><h2>{title}</h2>{error ? <ErrorNotice message={error} /> : children}</article>;
}

function ErrorNotice({ message }: { message: string }) { return <p className="error-notice" role="alert">{message}</p>; }
function UnavailableState() { return <p className="muted">No server result is available. The portal will not invent state.</p>; }
function formatTimestamp(value: number) { return new Date(value * 1000).toLocaleString(); }
