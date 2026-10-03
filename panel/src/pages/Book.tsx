import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import { Async, Badge, Card, Empty, Notice, Page } from "../ui";

export function Book() {
  const { t, org } = useApp();
  const entries = useLoad(() => api.addressBook(org.id), [org.id]);
  const devices = useLoad(() => api.devices(org.id), [org.id]);
  const [error, setError] = useState<unknown>();
  const [device, setDevice] = useState("");
  const [alias, setAlias] = useState("");

  async function save(event: FormEvent) {
    event.preventDefault();
    try {
      setError(undefined);
      await api.saveEntry(org.id, device, alias);
      setAlias("");
      entries.reload();
    } catch (e) {
      setError(e);
    }
  }

  async function remove(id: string) {
    try {
      setError(undefined);
      await api.deleteEntry(org.id, id);
      entries.reload();
    } catch (e) {
      setError(e);
    }
  }

  return (
    <Page title={t("bookTitle")} actions={<button onClick={entries.reload}>{t("refresh")}</button>}>
      <Notice error={error} />
      <Card>
        <form className="row" onSubmit={save}>
          <select value={device} onChange={(e) => setDevice(e.target.value)} required aria-label={t("device")}>
            <option value="">{t("device")}</option>
            {(devices.data ?? []).filter((d) => d.status === "active").map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
          </select>
          <input value={alias} onChange={(e) => setAlias(e.target.value)} placeholder={t("alias")} required maxLength={100} />
          <button className="primary" type="submit">{t("save")}</button>
        </form>
      </Card>
      <Async state={entries}>
        {(list) =>
          list.length === 0 ? (
            <Empty />
          ) : (
            <table>
              <thead><tr><th>{t("alias")}</th><th>{t("device")}</th><th>{t("status")}</th><th /></tr></thead>
              <tbody>
                {list.map((e) => (
                  <tr key={e.device_id}>
                    <td>{e.alias}</td>
                    <td>{e.device_name}</td>
                    <td><Badge tone={e.device_status === "active" ? "ok" : "off"}>{e.device_status === "active" ? t("active") : t("revoked")}</Badge></td>
                    <td><button className="danger" onClick={() => remove(e.device_id)}>{t("delete")}</button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          )
        }
      </Async>
    </Page>
  );
}
