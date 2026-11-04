// 全站共用的狀態：伺服器資訊、標注者清單、目前的標注者、深淺色、提示訊息。
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api } from "../api";
import type { Annotator, Meta } from "../types";

export type ThemeChoice = "system" | "light" | "dark";
type Toast = { id: number; tone: "ok" | "error" | "info"; text: string; details?: string[] };

type Session = {
  meta: Meta | null;
  annotators: Annotator[];
  annotator: Annotator | null;
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
    reloadAnnotators().catch(() => {});
  }, [reloadAnnotators]);

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

  const value = useMemo<Session>(() => {
    const annotator = annotators.find((a) => a.id === annotatorId && a.kind === "human") ?? null;
    return {
      meta,
      annotators,
      annotator,
      setAnnotatorId: (id) => {
        setAnnotatorIdState(id);
        writeStorage(ANNOTATOR_KEY, id === null ? null : String(id));
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
  }, [meta, annotators, annotatorId, reloadAnnotators, theme, toasts, toast, dismiss]);

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
