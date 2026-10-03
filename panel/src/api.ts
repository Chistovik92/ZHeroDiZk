import type { AclRule, AuditPage, Capability, Device, Entry, GrantItem, Group, LoginResult, Me, Member, Org } from "./types";

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

let apiBase = "";
let token: string | null = null;
let onUnauthorized: () => void = () => {};

export function configure(options: { base?: string; onUnauthorized?: () => void }): void {
  if (options.base !== undefined) apiBase = options.base.replace(/\/+$/, "");
  if (options.onUnauthorized) onUnauthorized = options.onUnauthorized;
}

export function setToken(value: string | null): void {
  token = value;
}

/** Turns the body of an error response into a message; the server sends `{"error": "..."}`. */
export function errorMessage(body: unknown, status: number): string {
  if (body && typeof body === "object" && "error" in body && typeof (body as { error: unknown }).error === "string") {
    return (body as { error: string }).error;
  }
  return `HTTP ${status}`;
}

async function request<T>(method: string, path: string, body?: unknown, auth = true): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (auth && token) headers["authorization"] = `Bearer ${token}`;
  let response: Response;
  try {
    response = await fetch(apiBase + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch {
    throw new ApiError(0, "network");
  }
  const text = await response.text();
  let parsed: unknown = null;
  if (text) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
  }
  if (!response.ok) {
    if (response.status === 401 && auth && token) onUnauthorized();
    throw new ApiError(response.status, errorMessage(parsed, response.status));
  }
  return parsed as T;
}

const org = (id: string) => `/v1/orgs/${encodeURIComponent(id)}`;

export const api = {
  health: () => request<{ status: string; database: string }>("GET", "/healthz", undefined, false),
  register: (email: string, password: string) => request<Me>("POST", "/v1/auth/register", { email, password }, false),
  login: (email: string, password: string) => request<LoginResult>("POST", "/v1/auth/login", { email, password }, false),
  loginMfa: (mfaToken: string, code: string) =>
    request<{ token: string; expires_at: string }>("POST", "/v1/auth/login/mfa", { mfa_token: mfaToken, code }, false),
  logout: () => request<void>("POST", "/v1/auth/logout"),
  me: () => request<Me>("GET", "/v1/auth/me"),
  changePassword: (current_password: string, new_password: string) =>
    request<void>("POST", "/v1/auth/password", { current_password, new_password }),
  mfaEnroll: () => request<{ secret: string; otpauth_uri: string }>("POST", "/v1/auth/mfa/enroll"),
  mfaConfirm: (code: string) => request<{ recovery_codes: string[] }>("POST", "/v1/auth/mfa/confirm", { code }),
  mfaDisable: (password: string, code: string) => request<void>("POST", "/v1/auth/mfa/disable", { password, code }),

  orgs: () => request<Org[]>("GET", "/v1/orgs"),
  createOrg: (name: string) => request<Org>("POST", "/v1/orgs", { name }),
  members: (id: string) => request<Member[]>("GET", `${org(id)}/members`),
  addMember: (id: string, email: string, role: "member" | "admin") =>
    request<void>("POST", `${org(id)}/members`, { email, role }),
  enrollmentToken: (id: string) => request<{ token: string; expires_at: string }>("POST", `${org(id)}/enrollment-tokens`),

  devices: (id: string) => request<Device[]>("GET", `${org(id)}/devices`),
  revokeDevice: (id: string, device: string) => request<void>("POST", `${org(id)}/devices/${device}/revoke`),
  access: (id: string, device: string) =>
    request<{ device_id: string; capabilities: Capability[] }>("GET", `${org(id)}/devices/${device}/access`),

  groups: (id: string) => request<Group[]>("GET", `${org(id)}/groups`),
  createGroup: (id: string, name: string) => request<Group>("POST", `${org(id)}/groups`, { name }),
  addToGroup: (id: string, group: string, device_id: string) =>
    request<void>("POST", `${org(id)}/groups/${group}/devices`, { device_id }),
  removeFromGroup: (id: string, group: string, device: string) =>
    request<void>("DELETE", `${org(id)}/groups/${group}/devices/${device}`),

  acl: (id: string) => request<AclRule[]>("GET", `${org(id)}/acl`),
  putAcl: (id: string, rule: AclRule) => request<AclRule>("PUT", `${org(id)}/acl`, rule),
  deleteAcl: (id: string, user: string, group: string) => request<void>("DELETE", `${org(id)}/acl/${user}/${group}`),

  addressBook: (id: string) => request<Entry[]>("GET", `${org(id)}/address-book`),
  saveEntry: (id: string, device: string, alias: string) =>
    request<void>("PUT", `${org(id)}/address-book/${device}`, { alias }),
  deleteEntry: (id: string, device: string) => request<void>("DELETE", `${org(id)}/address-book/${device}`),

  grants: (id: string) => request<GrantItem[]>("GET", `${org(id)}/grants`),
  revokeGrant: (id: string, grant: string) => request<void>("POST", `${org(id)}/grants/${grant}/revoke`),

  audit: (id: string, before?: number, limit = 100) =>
    request<AuditPage>("GET", `${org(id)}/audit?limit=${limit}${before ? `&before=${before}` : ""}`),
};
