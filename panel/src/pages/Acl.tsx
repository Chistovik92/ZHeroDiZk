import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import type { Key } from "../i18n";
import { CAPABILITIES } from "../types";
import type { Capability } from "../types";
import { Async, Card, Empty, Notice, Page } from "../ui";

export function Acl() {
  const { t, org } = useApp();
  const rules = useLoad(() => api.acl(org.id), [org.id]);
  const members = useLoad(() => api.members(org.id), [org.id]);
  const groups = useLoad(() => api.groups(org.id), [org.id]);
  const [error, setError] = useState<unknown>();
  const [user, setUser] = useState("");
  const [group, setGroup] = useState("");
  const [caps, setCaps] = useState<Capability[]>(["view"]);

  const email = (id: string) => members.data?.find((m) => m.user_id === id)?.email ?? id;
  const groupName = (id: string) => groups.data?.find((g) => g.id === id)?.name ?? id;
  const capLabel = (c: Capability) => t(`cap_${c}` as Key);

  function toggle(c: Capability) {
    setCaps((current) => (current.includes(c) ? current.filter((x) => x !== c) : [...current, c]));
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    try {
      setError(undefined);
      await api.putAcl(org.id, { user_id: user, group_id: group, capabilities: caps });
      rules.reload();
    } catch (e) {
      setError(e);
    }
  }

  async function remove(u: string, g: string) {
    try {
      setError(undefined);
      await api.deleteAcl(org.id, u, g);
      rules.reload();
    } catch (e) {
      setError(e);
    }
  }

  return (
    <Page title={t("aclTitle")} actions={<button onClick={rules.reload}>{t("refresh")}</button>}>
      <Notice error={error} />
      <Card hint={t("aclHint")}>
        <form className="row wrap" onSubmit={save}>
          <select value={user} onChange={(e) => setUser(e.target.value)} required aria-label={t("member")}>
            <option value="">{t("member")}</option>
            {(members.data ?? []).map((m) => <option key={m.user_id} value={m.user_id}>{m.email}</option>)}
          </select>
          <select value={group} onChange={(e) => setGroup(e.target.value)} required aria-label={t("group")}>
            <option value="">{t("group")}</option>
            {(groups.data ?? []).map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
          </select>
          {CAPABILITIES.map((c) => (
            <label key={c} className="check">
              <input type="checkbox" checked={caps.includes(c)} onChange={() => toggle(c)} /> {capLabel(c)}
            </label>
          ))}
          <button className="primary" type="submit" disabled={caps.length === 0}>{t("save")}</button>
        </form>
      </Card>
      <Async state={rules}>
        {(list) =>
          list.length === 0 ? (
            <Empty />
          ) : (
            <table>
              <thead><tr><th>{t("member")}</th><th>{t("group")}</th><th>{t("capabilities")}</th><th /></tr></thead>
              <tbody>
                {list.map((r) => (
                  <tr key={`${r.user_id}/${r.group_id}`}>
                    <td>{email(r.user_id)}</td>
                    <td>{groupName(r.group_id)}</td>
                    <td>{r.capabilities.map(capLabel).join(", ")}</td>
                    <td><button className="danger" onClick={() => remove(r.user_id, r.group_id)}>{t("delete")}</button></td>
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
