/**
 * Shared authenticated P10 HTTP contract.
 *
 * This module is deliberately only a transport client. It never creates
 * roles, entitlements, MFA success, grants, or billing state. Those decisions
 * remain server-side in services/cloud-api.
 */

export type P10Plan = "INDIVIDUAL" | "PRO" | "TEAM" | "ENTERPRISE";

export type P10Account = {
  user: { id: string; email: string };
  organization: { id: string; name: string };
  role: string;
};

export type P10OrganizationMember = {
  user: { id: string; email: string };
  role: "owner" | "admin" | "member";
};

export type P10TeamPolicy = {
  organization_id: string;
  version: number;
  policy: Record<string, unknown>;
  updated_by: string;
};

export type P10Subscription = {
  id: string;
  organization_id: string | null;
  user_id: string | null;
  plan: P10Plan;
  billing_interval: "monthly" | "annual";
  state: string;
  seat_limit: number | null;
  provider: string | null;
};

export type P10BillingState = {
  billing: {
    available: boolean;
    provider: string | null;
    detail: string;
  };
  subscriptions: P10Subscription[];
};

export type P10Entitlement = {
  claims: {
    subject: string;
    organization: string | null;
    plan: P10Plan;
    source: "PLAN" | "FOUNDER" | "COMPLIMENTARY" | "TRIAL" | "ADMIN";
    capabilities: string[];
    limits: Record<string, number>;
    issued_at: number;
    expires_at: number;
    grace_until: number;
    schema_version: number;
    issuer: string;
    id: string;
    key_id: string;
  };
  signature: string;
};

export type P10AuditEvent = {
  id: string;
  actor_user_id: string | null;
  actor_role: string;
  organization_id: string | null;
  target_user_id: string | null;
  entity_type: string;
  entity_id: string;
  operation: string;
  reason: string;
  outcome: string;
  request_id: string;
  created_at: string;
};

export type P10GrantInput = {
  user_id?: string;
  organization_id?: string;
  plan: P10Plan;
  expires_in_seconds?: number;
  seat_limit?: number;
  ai_budget_override?: number;
  reason: string;
};

export type P10GrantModifyRequest = {
  plan?: P10Plan;
  expires_in_seconds?: number | null;
  seat_limit?: number | null;
  ai_budget_override?: number | null;
  reason: string;
};

export type P10MfaChallenge = { challenge_id: string; expires_at: number };
export type P10MfaProof = { proof: string; expires_at: number };

export class CloudApiError extends Error {
  public readonly status: number | null;

  public constructor(message: string, status: number | null = null) {
    super(message);
    this.name = "CloudApiError";
    this.status = status;
  }
}

export function createCloudApiClient(baseUrl: string, accessToken: string): CloudApiClient {
  let url: URL;
  try {
    url = new URL(baseUrl.trim());
  } catch {
    throw new CloudApiError("Cloud API URL must be a valid http or https URL");
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new CloudApiError("Cloud API URL must use http or https");
  }
  if (!accessToken.trim()) {
    throw new CloudApiError("An in-memory authenticated access token is required");
  }
  return new CloudApiClient(url.toString().replace(/\/$/u, ""), accessToken);
}

export class CloudApiClient {
  private readonly baseUrl: string;
  private readonly accessToken: string;

  public constructor(baseUrl: string, accessToken: string) {
    this.baseUrl = baseUrl;
    this.accessToken = accessToken;
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    let response: Response;
    try {
      response = await fetch(`${this.baseUrl}${path}`, {
        ...init,
        headers: {
          Accept: "application/json",
          Authorization: `Bearer ${this.accessToken}`,
          ...(init.body ? { "Content-Type": "application/json" } : {}),
          ...init.headers,
        },
      });
    } catch (error) {
      throw new CloudApiError(
        `Cloud API unavailable: ${error instanceof Error ? error.message : "network request failed"}`,
      );
    }

    const body: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      const message =
        typeof body === "object" && body !== null && "error" in body
          ? String(
              (body as { error?: { message?: unknown } }).error?.message ||
                "Cloud API request denied",
            )
          : "Cloud API request denied";
      throw new CloudApiError(message, response.status);
    }
    return body as T;
  }

  public account(): Promise<P10Account> {
    return this.request<P10Account>("/v1/account");
  }

  public entitlement(): Promise<P10Entitlement> {
    return this.request<P10Entitlement>("/v1/entitlements");
  }

  public members(): Promise<P10OrganizationMember[]> {
    return this.request<P10OrganizationMember[]>("/v1/organizations/members");
  }

  public policy(): Promise<P10TeamPolicy> {
    return this.request<P10TeamPolicy>("/v1/organizations/policy");
  }

  public billing(): Promise<P10BillingState> {
    return this.request<P10BillingState>("/v1/billing/status");
  }

  public audit(): Promise<P10AuditEvent[]> {
    return this.request<P10AuditEvent[]>("/v1/admin/audit");
  }

  public beginMfa(): Promise<P10MfaChallenge> {
    return this.request<P10MfaChallenge>("/v1/admin/mfa/challenge", { method: "POST" });
  }

  public verifyMfa(challengeId: string, code: string): Promise<P10MfaProof> {
    return this.request<P10MfaProof>("/v1/admin/mfa/verify", {
      method: "POST",
      body: JSON.stringify({ challenge_id: challengeId, code }),
    });
  }

  public createCompanyGrant(proof: string, input: P10GrantInput): Promise<unknown> {
    return this.request("/v1/admin/grants/company", this.proofRequest(proof, input));
  }

  public createUserGrant(proof: string, input: P10GrantInput): Promise<unknown> {
    return this.request("/v1/admin/grants/user", this.proofRequest(proof, input));
  }

  public modifyGrant(
    grantId: string,
    proof: string,
    input: P10GrantModifyRequest,
  ): Promise<unknown> {
    return this.request(`/v1/admin/grants/${encodeURIComponent(grantId)}`, {
      ...this.proofRequest(proof, input),
      method: "PATCH",
    });
  }

  public revokeGrant(grantId: string, proof: string, reason: string): Promise<unknown> {
    return this.request(`/v1/admin/grants/${encodeURIComponent(grantId)}/revoke`, {
      ...this.proofRequest(proof, { reason }),
      method: "POST",
    });
  }

  public updatePolicy(
    proof: string,
    expectedVersion: number,
    policy: Record<string, unknown>,
    reason: string,
  ): Promise<P10TeamPolicy> {
    return this.request<P10TeamPolicy>("/v1/organizations/policy", {
      ...this.proofRequest(proof, {
        expected_version: expectedVersion,
        policy,
        reason,
      }),
      method: "PUT",
    });
  }

  public changeMemberRole(
    memberId: string,
    proof: string,
    role: "owner" | "admin" | "member",
    reason: string,
  ): Promise<unknown> {
    return this.request(`/v1/organizations/members/${encodeURIComponent(memberId)}`, {
      ...this.proofRequest(proof, { role, reason }),
      method: "PATCH",
    });
  }

  public removeMember(memberId: string, proof: string, reason: string): Promise<unknown> {
    return this.request(`/v1/organizations/members/${encodeURIComponent(memberId)}`, {
      ...this.proofRequest(proof, { reason }),
      method: "DELETE",
    });
  }

  private proofRequest(proof: string, body: unknown): RequestInit {
    return {
      method: "POST",
      headers: { "X-Admin-Mfa-Proof": proof },
      body: JSON.stringify(body),
    };
  }
}
