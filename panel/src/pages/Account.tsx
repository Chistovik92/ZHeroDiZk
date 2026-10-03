import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import { useApp } from "../app-context";
import { Card, Notice, Page } from "../ui";

export function Account() {
  const { t, me } = useApp();
  const [error, setError] = useState<unknown>();
  const [message, setMessage] = useState("");
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [enroll, setEnroll] = useState<{ secret: string; otpauth_uri: string }>();
  const [code, setCode] = useState("");
  const [recovery, setRecovery] = useState<string[]>();
  const [disablePassword, setDisablePassword] = useState("");
  const [disableCode, setDisableCode] = useState("");

  async function guarded(action: () => Promise<void>) {
    try {
      setError(undefined);
      setMessage("");
      await action();
    } catch (e) {
      setError(e);
    }
  }

  const changePassword = (event: FormEvent) => {
    event.preventDefault();
    void guarded(async () => {
      await api.changePassword(current, next);
      setCurrent("");
      setNext("");
      setMessage(t("passwordChanged"));
    });
  };

  return (
    <Page title={t("accountTitle")}>
      <p className="muted">{me.email}</p>
      <Notice error={error} />
      {message && <div className="notice ok">{message}</div>}
      <Card title={t("changePassword")}>
        <form className="stack" onSubmit={changePassword}>
          <input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} placeholder={t("currentPassword")} required />
          <input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} placeholder={t("newPassword")} minLength={12} required />
          <button className="primary" type="submit">{t("save")}</button>
        </form>
      </Card>
      <Card title={t("mfaTitle")}>
        {!enroll && !recovery && (
          <button className="primary" onClick={() => guarded(async () => setEnroll(await api.mfaEnroll()))}>{t("mfaStart")}</button>
        )}
        {enroll && (
          <form
            className="stack"
            onSubmit={(event) => {
              event.preventDefault();
              void guarded(async () => {
                const result = await api.mfaConfirm(code);
                setRecovery(result.recovery_codes);
                setEnroll(undefined);
                setCode("");
              });
            }}
          >
            <p>{t("mfaSecret")}: <code className="secret">{enroll.secret}</code></p>
            <p className="muted mono">{enroll.otpauth_uri}</p>
            <input value={code} onChange={(e) => setCode(e.target.value)} placeholder={t("mfaCode")} inputMode="numeric" required />
            <button className="primary" type="submit">{t("mfaConfirm")}</button>
          </form>
        )}
        {recovery && (
          <div>
            <p>{t("mfaRecovery")}</p>
            <pre className="secret">{recovery.join("\n")}</pre>
          </div>
        )}
        <hr />
        <form
          className="row wrap"
          onSubmit={(event) => {
            event.preventDefault();
            void guarded(async () => {
              await api.mfaDisable(disablePassword, disableCode);
              setDisablePassword("");
              setDisableCode("");
              setRecovery(undefined);
            });
          }}
        >
          <input type="password" value={disablePassword} onChange={(e) => setDisablePassword(e.target.value)} placeholder={t("password")} required />
          <input value={disableCode} onChange={(e) => setDisableCode(e.target.value)} placeholder={t("mfaCode")} required />
          <button className="danger" type="submit">{t("mfaDisable")}</button>
        </form>
      </Card>
    </Page>
  );
}
