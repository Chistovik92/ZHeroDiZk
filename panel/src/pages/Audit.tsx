import { useEffect, useState } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import { auditToCsv, download } from "../export";
import type { AuditEvent } from "../types";
import { Empty, formatTime, Notice, Page, shortId } from "../ui";

export function Audit() {
  const { t, org } = useApp();
  const members = useLoad(() => api.members(org.id), [org.id]);
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);

  async function load(before?: number) {
    setBusy(true);
    try {
      setError(undefined);
      const page = await api.audit(org.id, before);
      setEvents((current) => (before ? [...current, ...page.events] : page.events));
      setNext(page.next_before);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    setEvents([]);
    setNext(null);
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id]);

  const actor = (id: string | null) => (id ? members.data?.find((m) => m.user_id === id)?.email ?? shortId(id) : "—");
  const stamp = () => new Date().toISOString().slice(0, 10);

  return (
    <Page
      title={t("auditTitle")}
      actions={
        <>
          <button onClick={() => load()}>{t("refresh")}</button>
          <button disabled={events.length === 0} onClick={() => download(`audit-${stamp()}.csv`, "text/csv", auditToCsv(events, actor))}>{t("exportCsv")}</button>
          <button disabled={events.length === 0} onClick={() => download(`audit-${stamp()}.json`, "application/json", JSON.stringify(events, null, 2))}>{t("exportJson")}</button>
        </>
      }
    >
      <Notice error={error} />
      {events.length === 0 && !busy && !error ? (
        <Empty />
      ) : (
        <table>
          <thead><tr><th>{t("time")}</th><th>{t("actor")}</th><th>{t("action")}</th><th>{t("target")}</th><th>{t("detail")}</th></tr></thead>
          <tbody>
            {events.map((e) => (
              <tr key={e.id}>
                <td>{formatTime(e.created_at)}</td>
                <td>{actor(e.actor_user_id)}</td>
                <td><code>{e.action}</code></td>
                <td>{e.target ?? ""}</td>
                <td className="mono">{JSON.stringify(e.detail)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {next !== null && <button disabled={busy} onClick={() => load(next)}>{t("auditMore")}</button>}
    </Page>
  );
}
