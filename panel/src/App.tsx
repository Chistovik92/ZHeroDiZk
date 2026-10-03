import { useCallback, useEffect, useMemo, useState } from "react";
import { api, configure, setToken } from "./api";
import { explainError, Provider, useLoad } from "./app-context";
import type { AppContext } from "./app-context";
import { detectLang, translate } from "./i18n";
import type { Key, Lang } from "./i18n";
import { Login } from "./Login";
import { Account } from "./pages/Account";
import { Acl } from "./pages/Acl";
import { Audit } from "./pages/Audit";
import { Book } from "./pages/Book";
import { Devices } from "./pages/Devices";
import { Grants } from "./pages/Grants";
import { Groups } from "./pages/Groups";
import { Home } from "./pages/Home";
import { Members } from "./pages/Members";
import { atLeast } from "./types";
import type { Me, Role } from "./types";

type Tab = "home" | "devices" | "groups" | "acl" | "book" | "grants" | "audit" | "members" | "account";

const TABS: { id: Tab; label: Key; icon: string; minimum: Role }[] = [
  { id: "home", label: "navHome", icon: "◉", minimum: "member" },
  { id: "devices", label: "navDevices", icon: "▣", minimum: "member" },
  { id: "groups", label: "navGroups", icon: "▤", minimum: "member" },
  { id: "acl", label: "navAcl", icon: "⚿", minimum: "admin" },
  { id: "book", label: "navBook", icon: "☰", minimum: "member" },
  { id: "grants", label: "navGrants", icon: "⇄", minimum: "member" },
  { id: "audit", label: "navAudit", icon: "≣", minimum: "admin" },
  { id: "members", label: "navMembers", icon: "☺", minimum: "admin" },
  { id: "account", label: "navAccount", icon: "⚙", minimum: "member" },
];

const SESSION_KEY = "zhd.session";

function stored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return allowed.includes(value as T) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}

function remember(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable (private mode); the setting then lasts for this page only.
  }
}

export function App() {
  const [lang, setLangState] = useState<Lang>(() => stored("zhd.lang", ["ru", "en"] as const, detectLang(navigator.language)));
  const [theme, setThemeState] = useState(() => stored("zhd.theme", ["auto", "light", "dark"] as const, "auto"));
  const [session, setSession] = useState<string | null>(() => {
    try {
      return sessionStorage.getItem(SESSION_KEY);
    } catch {
      return null;
    }
  });
  const [serverUp, setServerUp] = useState<boolean>();
  const [tab, setTab] = useState<Tab>("home");
  const [orgId, setOrgId] = useState<string>();
  const [notice, setNotice] = useState("");

  const t = useCallback((key: Key) => translate(lang, key), [lang]);
  const explain = useCallback((error: unknown) => explainError(error, t), [t]);

  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dataset.theme = theme;
  }, [lang, theme]);

  const signOut = useCallback((message = "") => {
    setToken(null);
    setSession(null);
    setNotice(message);
    try {
      sessionStorage.removeItem(SESSION_KEY);
    } catch {
      // nothing to clear
    }
  }, []);

  useEffect(() => {
    configure({ onUnauthorized: () => signOut(translate(lang, "sessionEnded")) });
  }, [lang, signOut]);

  useEffect(() => {
    setToken(session);
  }, [session]);

  useEffect(() => {
    let cancelled = false;
    fetch("config.json")
      .then((r) => (r.ok ? r.json() : {}))
      .catch(() => ({}))
      .then((config: { apiBase?: string }) => {
        if (!cancelled && typeof config.apiBase === "string") configure({ base: config.apiBase });
      })
      .finally(() => {
        api.health().then(() => !cancelled && setServerUp(true)).catch(() => !cancelled && setServerUp(false));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  function onToken(token: string) {
    setToken(token);
    setSession(token);
    setNotice("");
    try {
      sessionStorage.setItem(SESSION_KEY, token);
    } catch {
      // the session then lasts until this page is reloaded
    }
  }

  const me = useLoad<Me | null>(() => (session ? api.me() : Promise.resolve(null)), [session]);
  const orgs = useLoad(() => (session ? api.orgs() : Promise.resolve([])), [session]);

  const org = useMemo(() => orgs.data?.find((o) => o.id === orgId) ?? orgs.data?.[0], [orgs.data, orgId]);
  const context: AppContext | null = me.data && org ? { t, me: me.data, org, explain } : null;

  if (!session) {
    return (
      <>
        {notice && <div className="notice error top">{notice}</div>}
        <Login t={t} explain={explain} serverUp={serverUp} onToken={onToken} />
      </>
    );
  }

  const tabs = TABS.filter((entry) => atLeast(org?.role, entry.minimum) || (!org && entry.id === "account"));
  const active = tabs.some((x) => x.id === tab) ? tab : "home";

  return (
    <div className="shell">
      <nav className="side" aria-label="main">
        <div className="brand"><img src="./icon.svg" alt="" width={28} height={28} /> <b>ZHeroDiZk</b></div>
        {tabs.map((entry) => (
          <button key={entry.id} className={`nav ${active === entry.id ? "on" : ""}`} onClick={() => setTab(entry.id)}>
            <span aria-hidden="true">{entry.icon}</span> {t(entry.label)}
          </button>
        ))}
        <div className="spacer" />
        <label className="field">
          {t("language")}
          <select value={lang} onChange={(e) => { setLangState(e.target.value as Lang); remember("zhd.lang", e.target.value); }}>
            <option value="ru">Русский</option>
            <option value="en">English</option>
          </select>
        </label>
        <label className="field">
          {t("theme")}
          <select value={theme} onChange={(e) => { setThemeState(e.target.value as typeof theme); remember("zhd.theme", e.target.value); }}>
            <option value="auto">{t("themeAuto")}</option>
            <option value="light">{t("themeLight")}</option>
            <option value="dark">{t("themeDark")}</option>
          </select>
        </label>
        <button className="nav" onClick={() => { void api.logout().catch(() => undefined); signOut(); }}>⏻ {t("signOut")}</button>
      </nav>
      <div className="main">
        {orgs.data && orgs.data.length > 0 && (
          <div className="topbar">
            <label className="field inline">
              {t("organisation")}
              <select value={org?.id ?? ""} onChange={(e) => setOrgId(e.target.value)}>
                {orgs.data.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}
              </select>
            </label>
            <NewOrg t={t} explain={explain} onCreated={(id) => { setOrgId(id); orgs.reload(); }} />
          </div>
        )}
        {me.error !== undefined && <div className="notice error">{explain(me.error)}</div>}
        {orgs.data && orgs.data.length === 0 && me.data ? (
          <div className="page">
            <p className="muted">{t("noOrg")}</p>
            <NewOrg t={t} explain={explain} onCreated={(id) => { setOrgId(id); orgs.reload(); }} />
          </div>
        ) : context ? (
          <Provider value={context}>
            {active === "home" && <Home />}
            {active === "devices" && <Devices />}
            {active === "groups" && <Groups />}
            {active === "acl" && <Acl />}
            {active === "book" && <Book />}
            {active === "grants" && <Grants />}
            {active === "audit" && <Audit />}
            {active === "members" && <Members />}
            {active === "account" && <Account />}
          </Provider>
        ) : (
          <p className="muted page">{t("loading")}</p>
        )}
      </div>
    </div>
  );
}

function NewOrg({ t, explain, onCreated }: { t: (key: Key) => string; explain: (e: unknown) => string; onCreated: (id: string) => void }) {
  const [name, setName] = useState("");
  const [error, setError] = useState<unknown>();
  return (
    <form
      className="row"
      onSubmit={async (event) => {
        event.preventDefault();
        try {
          setError(undefined);
          const created = await api.createOrg(name);
          setName("");
          onCreated(created.id);
        } catch (e) {
          setError(e);
        }
      }}
    >
      <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("newOrganisation")} maxLength={100} required />
      <button type="submit">{t("create")}</button>
      {error !== undefined && <span className="notice error">{explain(error)}</span>}
    </form>
  );
}
