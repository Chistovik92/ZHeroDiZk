import type { ReactNode } from "react";
import { useApp } from "./app-context";
import type { Loaded } from "./app-context";

export function Page({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="page">
      <header className="page-head">
        <h1>{title}</h1>
        <div className="row">{actions}</div>
      </header>
      {children}
    </section>
  );
}

export function Card({ title, hint, children }: { title?: string; hint?: string; children: ReactNode }) {
  return (
    <div className="card">
      {title && <h2>{title}</h2>}
      {hint && <p className="muted">{hint}</p>}
      {children}
    </div>
  );
}

export function Notice({ error }: { error: unknown }) {
  const { explain } = useApp();
  if (!error) return null;
  return (
    <div className="notice error" role="alert">
      {explain(error)}
    </div>
  );
}

/** Shows loading text, an error, or the children once data has arrived. */
export function Async<T>({ state, children }: { state: Loaded<T>; children: (data: T) => ReactNode }) {
  const { t } = useApp();
  if (state.error) return <Notice error={state.error} />;
  if (state.data === undefined) return <p className="muted">{t("loading")}</p>;
  return <>{children(state.data)}</>;
}

export function Empty() {
  const { t } = useApp();
  return <p className="muted">{t("empty")}</p>;
}

export function Badge({ tone, children }: { tone: "ok" | "warn" | "off"; children: ReactNode }) {
  return <span className={`badge ${tone}`}>{children}</span>;
}

export function formatTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function shortId(id: string): string {
  return id.slice(0, 8);
}
