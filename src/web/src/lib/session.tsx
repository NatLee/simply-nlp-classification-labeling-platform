// 全站共用的狀態：伺服器資訊、登入狀態、標注者清單、目前的標注者、深淺色、提示訊息。
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, UNAUTHORIZED_EVENT } from "../api";
import type { Annotator, AuthState, AuthUser, Meta, Role } from "../types";

export type ThemeChoice = "system" | "light" | "dark";
type Toast = { id: number; tone: "ok" | "error" | "info"; text: string; details?: string[] };

/** 介面上的權限，和後端 auth::required_level 的分級一致：user 是任何登入的人。 */
export type Level = "user" | "review" | "admin";
const ROLE_RANK: Record<Role, number> = { annotator: 1, reviewer: 2, admin: 3 };
const LEVEL_RANK: Record<Level, number> = { user: 1, review: 2, admin: 3 };

export const ROLE_LABELS: Record<Role, string> = { annotator: "標注者", reviewer: "審核者", admin: "管理員" };

type Session = {
  meta: Meta | null;
  /** null 表示還在問伺服器 */
  auth: AuthState | null;
  login: (username: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  /** 沒有啟用登入時一律可以 */
  can: (level: Level) => boolean;
  annotators: Annotator[];
  annotator: Annotator | null;
  /** 沒啟用登入時可以自己選標注者；啟用後標注者就是帳號本人 */
  canPickAnnotator: boolean;
  setAnnotatorId: (id: number | null) => void;
  reloadAnnotators: () => Promise<void>;
  theme: ThemeChoice;
  setTheme: (t: ThemeChoice) => void;
  toasts: Toast[];
  toast: (tone: Toast["tone"], text: string, details?: string[]) => void;
  dismiss: (id: number) => void;
};

const SessionContext = createContext<Session | null>(null);

const ANNOTATOR_KEY = "snl.annotator";
const THEME_KEY = "snl.theme";

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // 無痕模式或被擋住時就不記
  }
}

function applyTheme(choice: ThemeChoice) {
  const dark = choice === "dark" || (choice === "system" && window.matchMedia?.("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", !!dark);
}

let toastSeq = 0;

export function SessionProvider({ children }: { children: ReactNode }) {
  const [meta, setMeta] = useState<Meta | null>(null);
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [annotators, setAnnotators] = useState<Annotator[]>([]);
  const [annotatorId, setAnnotatorIdState] = useState<number | null>(() => {
    const v = readStorage(ANNOTATOR_KEY);
    return v ? Number(v) : null;
  });
  const [theme, setThemeState] = useState<ThemeChoice>(() => (readStorage(THEME_KEY) as ThemeChoice) || "system");
  const [toasts, setToasts] = useState<Toast[]>([]);

  const reloadAnnotators = useCallback(async () => {
    setAnnotators(await api.annotators());
  }, []);

  useEffect(() => {
    api.meta().then(setMeta).catch(() => setMeta(null));
    api
      .authState()
      .then(setAuth)
      .catch(() => setAuth({ enabled: false, user: null }));
  }, []);

  // 登入狀態確定之後（或換了人）再讀標注者清單
  const signedIn = auth !== null && (!auth.enabled || auth.user !== null);
  const userId = auth?.user?.id;
  useEffect(() => {
    if (signedIn) reloadAnnotators().catch(() => {});
  }, [signedIn, userId, reloadAnnotators]);

  useEffect(() => {
    const onUnauthorized = () => setAuth((a) => (a?.enabled ? { enabled: true, user: null } : a));
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, []);

  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq?.addEventListener?.("change", onChange);
    return () => mq?.removeEventListener?.("change", onChange);
  }, [theme]);

  const dismiss = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);
  const toast = useCallback(
    (tone: Toast["tone"], text: string, details?: string[]) => {
      const id = ++toastSeq;
      setToasts((t) => [...t.slice(-3), { id, tone, text, details }]);
      setTimeout(() => dismiss(id), tone === "error" ? 8000 : 3500);
    },
    [dismiss],
  );

  const login = useCallback(async (username: string, password: string) => {
    setAuth(await api.login(username, password));
  }, []);
  const logout = useCallback(async () => {
    await api.logout();
    setAuth({ enabled: true, user: null });
    setAnnotators([]);
  }, []);

  const value = useMemo<Session>(() => {
    const user: AuthUser | null = auth?.enabled ? auth.user : null;
    const canPickAnnotator = !auth?.enabled;
    const id = user ? user.annotatorId : annotatorId;
    const annotator =
      annotators.find((a) => a.id === id && a.kind === "human") ??
      (user ? { id: user.annotatorId, name: user.username, kind: "human" as const, createdAt: 0 } : null);
    return {
      meta,
      auth,
      login,
      logout,
      can: (level) => !auth?.enabled || (!!user && ROLE_RANK[user.role] >= LEVEL_RANK[level]),
      annotators,
      annotator,
      canPickAnnotator,
      setAnnotatorId: (next) => {
        if (!canPickAnnotator) return;
        setAnnotatorIdState(next);
        writeStorage(ANNOTATOR_KEY, next === null ? null : String(next));
      },
      reloadAnnotators,
      theme,
      setTheme: (t) => {
        setThemeState(t);
        writeStorage(THEME_KEY, t);
      },
      toasts,
      toast,
      dismiss,
    };
  }, [meta, auth, login, logout, annotators, annotatorId, reloadAnnotators, theme, toasts, toast, dismiss]);

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession() {
  const s = useContext(SessionContext);
  if (!s) throw new Error("useSession must be used inside <SessionProvider>");
  return s;
}

/** 顯示 API 錯誤（含逐項的問題）。 */
export function useErrorToast() {
  const { toast } = useSession();
  return useCallback(
    (e: unknown) => {
      const err = e as { message?: string; details?: string[] };
      toast("error", err.message ?? String(e), err.details);
    },
    [toast],
  );
}
