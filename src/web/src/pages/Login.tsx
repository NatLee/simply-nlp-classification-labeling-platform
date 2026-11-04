import { LogIn } from "lucide-react";
import { useState } from "react";
import { Logo } from "../components/Layout";
import { Spinner, Toaster } from "../components/ui";
import { useSession } from "../lib/session";

/** 啟用內建登入、還沒登入時顯示的頁面。 */
export function LoginPage() {
  const { login, meta } = useSession();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await login(username, password);
    } catch (err) {
      setError((err as Error).message);
      setPassword("");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="grid min-h-dvh place-items-center bg-[radial-gradient(60rem_30rem_at_80%_-10%,var(--color-brand-100),transparent_70%)] px-4 dark:bg-[radial-gradient(60rem_30rem_at_80%_-10%,var(--color-brand-950),transparent_70%)]">
      <main className="w-full max-w-sm">
        <div className="mb-6 flex items-center gap-3">
          <Logo className="size-10" />
          <div className="leading-tight">
            <p className="text-lg font-semibold tracking-tight">Simply Label</p>
            <p className="text-sm text-zinc-500 dark:text-zinc-400">分類標注平台</p>
          </div>
        </div>
        <form onSubmit={submit} className="card space-y-4 p-6">
          <h1 className="text-base font-semibold">登入</h1>
          <div>
            <label className="label" htmlFor="username">
              帳號
            </label>
            <input id="username" className="input" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} autoFocus />
          </div>
          <div>
            <label className="label" htmlFor="password">
              密碼
            </label>
            <input id="password" type="password" className="input" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />
          </div>
          {error && (
            <p role="alert" className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-950/60 dark:text-red-300">
              {error}
            </p>
          )}
          <button className="btn btn-primary w-full" disabled={busy || !username.trim() || !password}>
            {busy ? <Spinner /> : <LogIn className="size-4" />}
            登入
          </button>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">帳號由管理員建立。忘記密碼請找管理員重設。</p>
        </form>
        <p className="mt-4 text-center text-[11px] text-zinc-400">v{meta?.version ?? "…"}</p>
      </main>
      <Toaster />
    </div>
  );
}
