import { useState } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import type { Key } from "../i18n";
import { Async, Badge, Empty, formatTime, Notice, Page } from "../ui";

export function Grants() {
  const { t, org } = useApp();
  const state = useLoad(() => api.grants(org.id), [org.id]);
  const [error, setError] = useState<unknown>();

  async function revoke(id: string) {
    if (!window.confirm(t("confirmRevoke"))) return;
    try {
      setError(undefined);
      await api.revokeGrant(org.id, id);
      state.reload();
    } catch (e) {
      setError(e);
    }
  }

  return (
    <Page title={t("grantsTitle")} actions={<button onClick={state.reload}>{t("refresh")}</button>}>
      <p className="muted">{t("grantsHint")}</p>
      <Notice error={error} />
      <Async state={state}>
        {(list) =>
          list.length === 0 ? (
            <Empty />
          ) : (
            <table>
              <thead>
                <tr><th>{t("operator")}</th><th>{t("device")}</th><th>{t("capabilities")}</th><th>{t("mode")}</th><th>{t("issued")}</th><th>{t("status")}</th><th /></tr>
              </thead>
              <tbody>
                {list.map((g) => (
                  <tr key={g.grant_id}>
                    <td>{g.operator_email}</td>
                    <td>{g.device_name}</td>
                    <td>{g.capabilities.map((c) => t(`cap_${c}` as Key)).join(", ")}</td>
                    <td>{g.mode}</td>
                    <td>{formatTime(g.issued_at)}</td>
                    <td>
                      <Badge tone={g.status === "active" ? "ok" : g.status === "revoked" ? "warn" : "off"}>
                        {t(g.status === "active" ? "active" : g.status === "revoked" ? "revoked" : "expired")}
                      </Badge>
                    </td>
                    <td>{g.status === "active" && <button className="danger" onClick={() => revoke(g.grant_id)}>{t("revoke")}</button>}</td>
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
