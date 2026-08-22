import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

const account = {
  user: { id: "user-1", email: "founder@example.test" },
  organization: { id: "org-1", name: "Relintor" },
  role: "owner",
};

function response(body: unknown, status = 200) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } }));
}

function installFetch(overrides: Record<string, unknown> = {}) {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(String(input)).pathname;
    if (path === "/v1/account") return response(account);
    if (path === "/v1/entitlements") return response({ claims: { plan: "ENTERPRISE", source: "FOUNDER", expires_at: 2000000000, grace_until: 2000000000, subject: "user-1", organization: "org-1", capabilities: [], limits: {}, issued_at: 100, schema_version: 1, issuer: "relintor", id: "ent-1", key_id: "key-1" }, signature: "signed" });
    if (path === "/v1/organizations/members") return response([]);
    if (path === "/v1/organizations/policy") return response({ organization_id: "org-1", version: 1, policy: {}, updated_by: "user-1" });
    if (path === "/v1/billing/status") return response({ billing: { available: false, provider: null, detail: "No provider configured" }, subscriptions: [] });
    if (path === "/v1/admin/audit") return overrides.auditError
      ? response({ error: { message: "admin scope required" } }, 403)
      : response(overrides.audit ?? [{ id: "audit-1", operation: "grant", entity_type: "complimentary_grant", outcome: "success", reason: "test", request_id: "req-1", created_at: "2026-08-16T00:00:00Z" }]);
    if (path === "/v1/admin/mfa/challenge") return response({ challenge_id: "challenge-1", expires_at: 2000000000 });
    if (path === "/v1/admin/mfa/verify") return response({ proof: "server-proof", expires_at: 2000000000 });
    if (path === "/v1/admin/grants/user" && init?.method === "POST") return response({ id: "grant-1" });
    return response(overrides[path] ?? {}, 404);
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function connect() {
  fireEvent.change(screen.getByLabelText("Cloud API URL"), { target: { value: "https://cloud.example.test" } });
  fireEvent.change(screen.getByLabelText("Authenticated access token"), { target: { value: "session-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Connect securely" }));
}

describe("P10 founder/admin portal", () => {
  beforeEach(() => { vi.restoreAllMocks(); });

  it("starts signed out and does not invent account state", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Founder & admin portal" })).toBeTruthy();
    expect(screen.getByText("SIGNED OUT")).toBeTruthy();
    expect(screen.getByText(/held in memory only/)).toBeTruthy();
  });

  it("shows a truthful API denial", async () => {
    const fetchMock = vi.fn(() => response({ error: { message: "session denied" } }, 403));
    vi.stubGlobal("fetch", fetchMock);
    render(<App />); connect();
    expect((await screen.findByRole("alert")).textContent).toContain("session denied");
  });

  it("loads authorized account, billing status, and audit history from HTTP", async () => {
    installFetch(); render(<App />); connect();
    expect(await screen.findByText("Relintor")).toBeTruthy();
    expect(screen.getByText("PENDING_EXTERNAL_ENVIRONMENT")).toBeTruthy();
    expect(screen.getByText("grant · complimentary_grant")).toBeTruthy();
    expect(screen.getByText("Server returned the admin audit scope.")).toBeTruthy();
  });

  it("requires a server MFA challenge before showing an in-memory proof", async () => {
    installFetch(); render(<App />); connect();
    await screen.findByText("Relintor");
    fireEvent.click(screen.getByRole("button", { name: "Request MFA challenge" }));
    expect(await screen.findByLabelText("Authenticator code")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Authenticator code"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify with server" }));
    expect(await screen.findByText(/held in memory only/)).toBeTruthy();
  });

  it("wires a grant mutation through the server-issued MFA proof", async () => {
    const fetchMock = installFetch(); render(<App />); connect();
    await screen.findByText("Relintor");
    fireEvent.click(screen.getByRole("button", { name: "Request MFA challenge" }));
    fireEvent.change(await screen.findByLabelText("Authenticator code"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify with server" }));
    await screen.findByText(/MFA proof accepted/);
    fireEvent.change(screen.getByLabelText("Target user or organization ID"), { target: { value: "target-user" } });
    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "support grant" } });
    fireEvent.click(screen.getByRole("button", { name: "Create grant" }));
    expect(await screen.findByText(/Grant ID: grant-1/)).toBeTruthy();
    const grantCall = fetchMock.mock.calls.find(([input]) => String(input).endsWith("/v1/admin/grants/user"));
    expect(grantCall?.[1]?.headers).toEqual(expect.objectContaining({ "X-Admin-Mfa-Proof": "server-proof" }));
  });

  it("surfaces an admin audit denial independently of account authentication", async () => {
    installFetch({ auditError: true });
    render(<App />); connect();
    expect(await screen.findByText(/Admin API access denied or unavailable/)).toBeTruthy();
    await waitFor(() => expect(screen.queryByText("Server returned the admin audit scope.")).toBeNull());
  });
});
