import { useState } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import { atLeast } from "../types";
import { Async, Badge, Card, Empty, formatTime, Notice, Page } from "../ui";

export function Devices() {
  const { t, org } = useApp();
  const admin = atLeast(org.role, "admin");
  const state = useLoad(() => api.devices(org.id), [org.id]);
  const [error, setError] = useState<unknown>();
  const [token, setToken] = useState<{ token: string; expires_at: string }>();

  async function revoke(id: string) {
    if (!window.confirm(t("confirmRevoke"))) return;
    try {
      setError(undefined);
      await api.revokeDevice(org.id, id);
      state.reload();
    } catch (e) {
      setError(e);
    }
  }

  async function enrol() {
    try {
      setError(undefined);
      setToken(await api.enrollmentToken(org.id));
    } catch (e) {
      setError(e);
    }
  }

  return (
    <Page title={t("devicesTitle")} actions={<button onClick={state.reload}>{t("refresh")}</button>}>
      <Notice error={error} />
      {admin && (
        <Card title={t("enrollTitle")} hint={t("enrollHint")}>
          <button className="primary" onClick={enrol}>{t("enrollCreate")}</button>
          {token && (
            <p>
              {t("token")}: <code className="secret">{token.token}</code>
              <br />
              <span className="muted">{t("expires")}: {formatTime(token.expires_at)}</span>
            </p>
          )}
        </Card>
      )}
      <Async state={state}>
        {(devices) =>
          devices.length === 0 ? (
            <Empty />
          ) : (
            <table>
              <thead>
                <tr><th>{t("name")}</th><th>{t("platform")}</th><th>{t("status")}</th><th>{t("created")}</th>{admin && <th />}</tr>
              </thead>
              <tbody>
                {devices.map((d) => (
                  <tr key={d.id}>
                    <td>{d.name}</td>
                    <td>{d.platform}</td>
                    <td><Badge tone={d.status === "active" ? "ok" : "off"}>{d.status === "active" ? t("active") : t("revoked")}</Badge></td>
                    <td>{formatTime(d.created_at)}</td>
                    {admin && (
                      <td>{d.status === "active" && <button className="danger" onClick={() => revoke(d.id)}>{t("revoke")}</button>}</td>
                    )}
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
