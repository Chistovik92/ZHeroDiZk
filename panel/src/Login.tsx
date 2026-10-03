import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "./api";
import type { Key } from "./i18n";

interface Props {
  t: (key: Key) => string;
  explain: (error: unknown) => string;
  serverUp: boolean | undefined;
  onToken: (token: string) => void;
}

export function Login({ t, explain, serverUp, onToken }: Props) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [mfaToken, setMfaToken] = useState<string>();
  const [code, setCode] = useState("");
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    try {
      setError(undefined);
      if (mfaToken) {
        onToken((await api.loginMfa(mfaToken, code)).token);
        return;
      }
      if (creating) await api.register(email, password);
      const result = await api.login(email, password);
      if ("mfa_required" in result) setMfaToken(result.mfa_token);
      else onToken(result.token);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="login">
      <form className="card login-card" onSubmit={submit}>
        <img src="./icon.svg" alt="" width={64} height={64} />
        <h1>ZHeroDiZk</h1>
        <p className="muted">{t("panel")}</p>
        {mfaToken ? (
          <input value={code} onChange={(e) => setCode(e.target.value)} placeholder={t("mfaCode")} autoComplete="one-time-code" autoFocus required />
        ) : (
          <>
            <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder={t("email")} autoComplete="username" required autoFocus />
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={t("password")} autoComplete={creating ? "new-password" : "current-password"} required />
          </>
        )}
        {error !== undefined && <div className="notice error" role="alert">{explain(error)}</div>}
        <button className="primary" type="submit" disabled={busy}>
          {mfaToken ? t("continue") : creating ? t("register") : t("signIn")}
        </button>
        {!mfaToken && (
          <button type="button" className="link" onClick={() => setCreating((v) => !v)}>
            {creating ? t("signIn") : t("register")}
          </button>
        )}
        {creating && <p className="muted small">{t("registerHint")}</p>}
        <p className="small status">
          <span className={`dot ${serverUp ? "ok" : "off"}`} /> {serverUp === undefined ? t("loading") : serverUp ? t("serverOk") : t("serverDown")}
        </p>
      </form>
    </main>
  );
}
