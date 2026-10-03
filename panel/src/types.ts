export type Role = "member" | "admin" | "owner";
export type Capability = "view" | "input" | "file_transfer" | "clipboard";
export const CAPABILITIES: Capability[] = ["view", "input", "file_transfer", "clipboard"];

export interface Me { id: string; email: string }
export interface Org { id: string; name: string; role: Role }
export interface Device { id: string; name: string; platform: string; status: string; created_at: string }
export interface Group { id: string; name: string; created_at: string }
export interface AclRule { user_id: string; group_id: string; capabilities: Capability[] }
export interface Member { user_id: string; email: string; role: Role }
export interface Entry { device_id: string; alias: string; device_name: string; device_status: string }
export interface GrantItem {
  grant_id: string;
  operator_id: string;
  operator_email: string;
  device_id: string;
  device_name: string;
  capabilities: Capability[];
  mode: string;
  issued_at: string;
  expires_at: string;
  revoked_at: string | null;
  status: "active" | "expired" | "revoked";
}
export interface AuditEvent {
  id: number;
  actor_user_id: string | null;
  action: string;
  target: string | null;
  detail: unknown;
  created_at: string;
}
export interface AuditPage { events: AuditEvent[]; next_before: number | null }
export type LoginResult =
  | { token: string; expires_at: string }
  | { mfa_required: true; mfa_token: string; expires_at: string };

export function atLeast(role: Role | undefined, minimum: Role): boolean {
  const order: Role[] = ["member", "admin", "owner"];
  return role !== undefined && order.indexOf(role) >= order.indexOf(minimum);
}
