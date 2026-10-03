import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import type { Key } from "../i18n";
import { atLeast } from "../types";
import { Async, Card, Empty, Notice, Page } from "../ui";

export function Members() {
  const { t, org } = useApp();
  const state = useLoad(() => api.members(org.id), [org.id]);
  const [error, setError] = useState<unknown>();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<"member" | "admin">("member");

  async function add(event: FormEvent) {
    event.preventDefault();
    try {
      setError(undefined);
      await api.addMember(org.id, email, role);
      setEmail("");
      state.reload();
    } catch (e) {
      setError(e);
    }
  }

  return (
    <Page title={t("membersTitle")} actions={<button onClick={state.reload}>{t("refresh")}</button>}>
      <Notice error={error} />
      <Card title={t("add")}>
        <form className="row" onSubmit={add}>
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder={t("email")} required />
          <select value={role} onChange={(e) => setRole(e.target.value as "member" | "admin")} aria-label={t("role")}>
            <option value="member">{t("roleMember")}</option>
            {atLeast(org.role, "owner") && <option value="admin">{t("roleAdmin")}</option>}
          </select>
          <button className="primary" type="submit">{t("add")}</button>
        </form>
      </Card>
      <Async state={state}>
        {(list) =>
          list.length === 0 ? (
            <Empty />
          ) : (
            <table>
              <thead><tr><th>{t("email")}</th><th>{t("role")}</th></tr></thead>
              <tbody>
                {list.map((m) => (
                  <tr key={m.user_id}>
                    <td>{m.email}</td>
                    <td>{t(`role${m.role[0].toUpperCase()}${m.role.slice(1)}` as Key)}</td>
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
