import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ApiError } from "./api";
import type { Key } from "./i18n";
import type { Me, Org } from "./types";

export interface AppContext {
  t: (key: Key) => string;
  me: Me;
  org: Org;
  /** Human-readable text for an error thrown by the API client. */
  explain: (error: unknown) => string;
}

export const AppCtx = createContext<AppContext | null>(null);

export function useApp(): AppContext {
  const value = useContext(AppCtx);
  if (!value) throw new Error("AppCtx is missing");
  return value;
}

export interface Loaded<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => void;
}

/** Runs `load` on mount and whenever `deps` change; stale answers are dropped. */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[]): Loaded<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<unknown>();
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const latest = useRef(0);
  useEffect(() => {
    const mine = ++latest.current;
    setLoading(true);
    load()
      .then((value) => {
        if (mine === latest.current) {
          setData(value);
          setError(undefined);
        }
      })
      .catch((e: unknown) => {
        if (mine === latest.current) setError(e);
      })
      .finally(() => {
        if (mine === latest.current) setLoading(false);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  const reload = useCallback(() => setTick((n) => n + 1), []);
  return { data, error, loading, reload };
}

export function explainError(error: unknown, t: (key: Key) => string): string {
  if (error instanceof ApiError) return error.status === 0 ? t("networkError") : error.message;
  return error instanceof Error ? error.message : String(error);
}

export function Provider({ value, children }: { value: AppContext; children: ReactNode }) {
  return <AppCtx.Provider value={value}>{children}</AppCtx.Provider>;
}
