import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import { atLeast } from "../types";
import { Async, Card, Empty, formatTime, Notice, Page } from "../ui";

export function Groups() {
  const { t, org } = useApp();
  const admin = atLeast(org.role, "admin");
  const groups = useLoad(() => api.groups(org.id), [org.id]);
  const devices = useLoad(() => api.devices(org.id), [org.id]);
  const [error, setError] = useState<unknown>();
  const [name, setName] = useState("");
  const [group, setGroup] = useState("");
  const [device, setDevice] = useState("");

  async function run(action: () => Promise<unknown>) {
    try {
      setError(undefined);
      await action();
      groups.reload();
    } catch (e) {
      setError(e);
    }
  }

  function create(event: FormEvent) {
    event.preventDefault();
    void run(async () => {
      await api.createGroup(org.id, name);
      setName("");
    });
  }

  const active = (devices.data ?? []).filter((d) => d.status === "active");

  return (
    <Page title={t("groupsTitle")} actions={<button onClick={groups.reload}>{t("refresh")}</button>}>
      <Notice error={error} />
      {admin && (
        <>
          <Card title={t("create")}>
            <form className="row" onSubmit={create}>
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("name")} required maxLength={100} />
              <button className="primary" type="submit">{t("create")}</button>
            </form>
          </Card>
          <Card title={t("addToGroup")}>
            <div className="row">
              <select value={group} onChange={(e) => setGroup(e.target.value)} aria-label={t("group")}>
                <option value="">{t("group")}</option>
                {(groups.data ?? []).map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
              </select>
              <select value={device} onChange={(e) => setDevice(e.target.value)} aria-label={t("groupDevice")}>
                <option value="">{t("groupDevice")}</option>
                {active.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
              </select>
              <button className="primary" disabled={!group || !device} onClick={() => run(() => api.addToGroup(org.id, group, device))}>
                {t("add")}
              </button>
              <button className="danger" disabled={!group || !device} onClick={() => run(() => api.removeFromGroup(org.id, group, device))}>
                {t("removeFromGroup")}
              </button>
            </div>
          </Card>
        </>
      )}
      <Async state={groups}>
        {(list) =>
          list.length === 0 ? (
            <Empty />
          ) : (
            <table>
              <thead><tr><th>{t("name")}</th><th>{t("created")}</th></tr></thead>
              <tbody>{list.map((g) => <tr key={g.id}><td>{g.name}</td><td>{formatTime(g.created_at)}</td></tr>)}</tbody>
            </table>
          )
        }
      </Async>
    </Page>
  );
}
