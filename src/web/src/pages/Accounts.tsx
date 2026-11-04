import { KeyRound, ShieldCheck, UserPlus } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { EmptyState, PageHeader, Spinner } from "../components/ui";
import { dateTime } from "../lib/format";
import { useAsync } from "../lib/hooks";
import { ROLE_LABELS, useErrorToast, useSession } from "../lib/session";
import type { Account, Role } from "../types";

const ROLES: Role[] = ["annotator", "reviewer", "admin"];
const ROLE_HELP: Record<Role, string> = {
  annotator: "看資料、標注、要 AI 建議",
  reviewer: "再加上匯出、隱藏資料、批次預標",
  admin: "全部，包括專案設定、匯入和帳號管理",
};

function Row({ a, me, onChange }: { a: Account; me: number | undefined; onChange: (a: Account) => void }) {
  const showError = useErrorToast();
  const { toast } = useSession();
  const [password, setPassword] = useState("");
  const [resetting, setResetting] = useState(false);
  const self = a.id === me;

  async function update(body: { role?: Role; disabled?: boolean; password?: string }) {
    try {
      onChange(await api.updateAccount(a.id, body));
      return true;
    } catch (e) {
      showError(e);
      return false;
    }
  }

  return (
    <li className={`flex flex-wrap items-center gap-3 px-4 py-3 ${a.disabled ? "opacity-60" : ""}`}>
      <span className="grid size-8 place-items-center rounded-full bg-zinc-900 text-xs font-semibold text-white dark:bg-zinc-100 dark:text-zinc-900">
        {a.username.slice(0, 1).toUpperCase()}
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium">
          {a.username}
          {self && <span className="chip ml-2 border-brand-200 bg-brand-50 text-brand-700 dark:border-brand-900 dark:bg-brand-950 dark:text-brand-300">你</span>}
          {a.disabled && <span className="chip ml-2 border-zinc-300 text-zinc-500">已停用</span>}
        </p>
        <p className="text-xs text-zinc-500 dark:text-zinc-400">
          {a.lastLoginAt ? `上次登入 ${dateTime(a.lastLoginAt)}` : "還沒登入過"}
        </p>
      </div>
      <select
        className="input w-32"
        value={a.role}
        disabled={self}
        aria-label={`${a.username} 的角色`}
        onChange={(e) => update({ role: e.target.value as Role })}
      >
        {ROLES.map((r) => (
          <option key={r} value={r}>
            {ROLE_LABELS[r]}
          </option>
        ))}
      </select>
      {resetting ? (
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            if (await update({ password })) {
              toast("ok", `已重設 ${a.username} 的密碼，他已登入的裝置都會被登出`);
              setResetting(false);
              setPassword("");
            }
          }}
        >
          <input type="password" className="input w-40" placeholder="新密碼（至少 8 字）" value={password} onChange={(e) => setPassword(e.target.value)} aria-label={`${a.username} 的新密碼`} autoFocus />
          <button className="btn btn-primary" disabled={password.length < 8}>
            儲存
          </button>
          <button type="button" className="btn btn-ghost" onClick={() => setResetting(false)}>
            取消
          </button>
        </form>
      ) : (
        <button className="btn btn-ghost" onClick={() => setResetting(true)}>
          <KeyRound className="size-4" />
          重設密碼
        </button>
      )}
      {!self && (
        <button className="btn btn-secondary" onClick={() => update({ disabled: !a.disabled })}>
          {a.disabled ? "啟用" : "停用"}
        </button>
      )}
    </li>
  );
}

export function AccountsPage() {
  const { auth, can, reloadAnnotators } = useSession();
  const showError = useErrorToast();
  const { data, loading, setData } = useAsync(() => api.accounts(), []);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<Role>("annotator");

  if (!auth?.enabled || !can("admin")) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-8 sm:px-8">
        <EmptyState icon={<ShieldCheck className="size-6" />} title="只有管理員可以管理帳號">
          {auth?.enabled ? "請找管理員協助。" : "目前沒有啟用登入。設定 ADMIN_PASSWORD 環境變數後重新啟動，就會啟用。"}
        </EmptyState>
      </div>
    );
  }

  async function create(e: React.FormEvent) {
    e.preventDefault();
    try {
      const a = await api.createAccount({ username, password, role });
      setData([...(data ?? []), a].sort((x, y) => x.username.localeCompare(y.username)));
      setUsername("");
      setPassword("");
      await reloadAnnotators();
    } catch (err) {
      showError(err);
    }
  }

  return (
    <div className="mx-auto max-w-4xl px-4 py-8 sm:px-8">
      <PageHeader
        title="帳號管理"
        subtitle="每個帳號就是一位標注者（同名的標注者會連在一起，啟用登入前用這個名字標的資料會歸到這個帳號）。停用或重設密碼後，那個人已登入的裝置會被登出。"
      />
      <form onSubmit={create} className="card mb-5 grid gap-3 p-4 sm:grid-cols-[1fr_1fr_10rem_auto] sm:items-end">
        <div>
          <label className="label" htmlFor="new-user">
            帳號
          </label>
          <input id="new-user" className="input" value={username} onChange={(e) => setUsername(e.target.value)} maxLength={40} />
        </div>
        <div>
          <label className="label" htmlFor="new-pass">
            密碼（至少 8 字）
          </label>
          <input id="new-pass" type="password" className="input" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
        </div>
        <div>
          <label className="label" htmlFor="new-role">
            角色
          </label>
          <select id="new-role" className="input" value={role} onChange={(e) => setRole(e.target.value as Role)}>
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {ROLE_LABELS[r]}
              </option>
            ))}
          </select>
        </div>
        <button className="btn btn-primary" disabled={!username.trim() || password.length < 8}>
          <UserPlus className="size-4" />
          新增帳號
        </button>
        <p className="hint sm:col-span-4">{ROLE_HELP[role]}</p>
      </form>
      {loading && !data ? (
        <div className="grid h-32 place-items-center text-zinc-400">
          <Spinner className="size-6" />
        </div>
      ) : (
        <ul className="card divide-y divide-zinc-100 dark:divide-zinc-800">
          {(data ?? []).map((a) => (
            <Row key={a.id} a={a} me={auth.user?.id} onChange={(next) => setData((data ?? []).map((x) => (x.id === next.id ? next : x)))} />
          ))}
        </ul>
      )}
      <dl className="mt-5 grid gap-2 text-sm sm:grid-cols-3">
        {ROLES.map((r) => (
          <div key={r} className="rounded-xl border border-zinc-200 p-3 dark:border-zinc-800">
            <dt className="font-medium">{ROLE_LABELS[r]}</dt>
            <dd className="text-xs text-zinc-500 dark:text-zinc-400">{ROLE_HELP[r]}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
