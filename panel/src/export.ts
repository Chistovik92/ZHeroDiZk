import type { AuditEvent } from "./types";

/** Quotes a CSV field and neutralises spreadsheet formulas (leading = + - @ tab CR). */
export function csvField(value: string): string {
  const safe = /^[=+\-@\t\r]/.test(value) ? `'${value}` : value;
  return /[",\n\r]/.test(safe) ? `"${safe.replace(/"/g, '""')}"` : safe;
}

export function auditToCsv(events: AuditEvent[], actorName: (id: string | null) => string): string {
  const lines = ["id,time,actor,action,target,detail"];
  for (const e of events) {
    lines.push(
      [String(e.id), e.created_at, actorName(e.actor_user_id), e.action, e.target ?? "", JSON.stringify(e.detail ?? {})]
        .map(csvField)
        .join(","),
    );
  }
  return lines.join("\r\n") + "\r\n";
}

export function download(filename: string, mime: string, content: string): void {
  const url = URL.createObjectURL(new Blob([content], { type: mime }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}
