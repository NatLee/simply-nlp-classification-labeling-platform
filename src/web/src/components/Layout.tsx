import {
  ArrowDownUp,
  ChartColumn,
  ChevronsUpDown,
  FolderKanban,
  Menu,
  Monitor,
  Moon,
  Settings2,
  Sparkles,
  Sun,
  Table2,
  Tags,
  Users,
  X,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { NavLink, Outlet, useLocation, useMatch } from "react-router";
import { api } from "../api";
import { useAsync } from "../lib/hooks";
import { useSession, type ThemeChoice } from "../lib/session";
import { AnnotatorPicker } from "./AnnotatorPicker";
import { Modal, Toaster } from "./ui";

export function Logo({ className = "size-8" }: { className?: string }) {
  return (
    <span className={`${className} grid shrink-0 place-items-center rounded-xl bg-gradient-to-br from-brand-400 to-brand-700 text-white shadow-sm shadow-brand-900/20`}>
      <Tags className="size-[55%]" aria-hidden />
    </span>
  );
}

function NavItem({ to, icon, children, end }: { to: string; icon: ReactNode; children: ReactNode; end?: boolean }) {
  return (
    <NavLink
      to={to}
      end={end}
      className={({ isActive }) =>
        `flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm font-medium transition ${
          isActive
            ? "bg-brand-50 text-brand-700 dark:bg-brand-950/70 dark:text-brand-300"
            : "text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800/70 dark:hover:text-zinc-100"
        }`
      }
    >
      <span className="[&>svg]:size-4">{icon}</span>
      {children}
    </NavLink>
  );
}

function ThemeSwitch() {
  const { theme, setTheme } = useSession();
  const options: { v: ThemeChoice; icon: ReactNode; label: string }[] = [
    { v: "light", icon: <Sun className="size-3.5" />, label: "淺色" },
    { v: "system", icon: <Monitor className="size-3.5" />, label: "跟系統" },
    { v: "dark", icon: <Moon className="size-3.5" />, label: "深色" },
  ];
  return (
    <div className="flex rounded-lg bg-zinc-100 p-0.5 dark:bg-zinc-800" role="radiogroup" aria-label="外觀">
      {options.map((o) => (
        <button
          key={o.v}
          role="radio"
          aria-checked={theme === o.v}
          title={o.label}
          onClick={() => setTheme(o.v)}
          className={`flex flex-1 items-center justify-center rounded-md py-1 transition ${
            theme === o.v
              ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-700 dark:text-zinc-50"
              : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
          }`}
        >
          {o.icon}
        </button>
      ))}
    </div>
  );
}

function ProjectSwitcher({ projectId }: { projectId: number }) {
  const { data: projects } = useAsync(() => api.projects(), [projectId]);
  const current = projects?.find((p) => p.id === projectId);
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <button
        className="flex w-full items-center gap-2 rounded-lg border border-zinc-200 bg-white px-2.5 py-2 text-left text-sm shadow-xs hover:border-zinc-300 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:border-zinc-700"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="grid size-6 shrink-0 place-items-center rounded-md bg-brand-100 text-xs font-semibold text-brand-700 dark:bg-brand-900 dark:text-brand-200">
          {current?.name.slice(0, 1) ?? "#"}
        </span>
        <span className="min-w-0 flex-1 truncate font-medium">{current?.name ?? `專案 #${projectId}`}</span>
        <ChevronsUpDown className="size-3.5 text-zinc-400" />
      </button>
      {open && (
        <div className="absolute inset-x-0 top-full z-30 mt-1 max-h-72 overflow-y-auto rounded-xl border border-zinc-200 bg-white p-1 shadow-xl dark:border-zinc-800 dark:bg-zinc-900">
          {projects?.map((p) => (
            <NavLink
              key={p.id}
              to={`/p/${p.id}`}
              onClick={() => setOpen(false)}
              className={`block truncate rounded-lg px-2.5 py-1.5 text-sm ${p.id === projectId ? "bg-brand-50 text-brand-700 dark:bg-brand-950 dark:text-brand-300" : "hover:bg-zinc-100 dark:hover:bg-zinc-800"}`}
            >
              {p.name}
            </NavLink>
          ))}
          <NavLink to="/" onClick={() => setOpen(false)} className="mt-1 block rounded-lg border-t border-zinc-100 px-2.5 py-1.5 text-sm text-zinc-500 hover:bg-zinc-100 dark:border-zinc-800 dark:hover:bg-zinc-800">
            所有專案…
          </NavLink>
        </div>
      )}
    </div>
  );
}

function Initials({ name }: { name: string }) {
  return (
    <span className="grid size-7 shrink-0 place-items-center rounded-full bg-zinc-900 text-xs font-semibold text-white dark:bg-zinc-100 dark:text-zinc-900">
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
}

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const match = useMatch("/p/:projectId/*");
  const projectId = match ? Number(match.params.projectId) : null;
  const { meta, annotator } = useSession();
  const [picking, setPicking] = useState(false);
  const ai = meta?.ai;
  return (
    <nav className="flex h-full flex-col gap-5 px-3 py-4" onClick={(e) => (e.target as HTMLElement).closest("a") && onNavigate?.()}>
      <div className="flex items-center gap-2.5 px-1.5">
        <Logo />
        <div className="leading-tight">
          <p className="text-sm font-semibold tracking-tight">Simply Label</p>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">分類標注平台</p>
        </div>
      </div>

      <div className="space-y-0.5">
        <NavItem to="/" end icon={<FolderKanban />}>
          所有專案
        </NavItem>
      </div>

      {projectId !== null && (
        <div className="space-y-2">
          <p className="section-title px-2.5">目前的專案</p>
          <ProjectSwitcher projectId={projectId} />
          <div className="space-y-0.5">
            <NavItem to={`/p/${projectId}`} end icon={<Tags />}>
              標注
            </NavItem>
            <NavItem to={`/p/${projectId}/items`} icon={<Table2 />}>
              資料
            </NavItem>
            <NavItem to={`/p/${projectId}/data`} icon={<ArrowDownUp />}>
              匯入匯出
            </NavItem>
            <NavItem to={`/p/${projectId}/stats`} icon={<ChartColumn />}>
              進度與一致性
            </NavItem>
            <NavItem to={`/p/${projectId}/settings`} icon={<Settings2 />}>
              專案設定
            </NavItem>
          </div>
        </div>
      )}

      <div className="space-y-0.5">
        <p className="section-title mb-2 px-2.5">團隊</p>
        <NavItem to="/annotators" icon={<Users />}>
          標注者
        </NavItem>
      </div>

      <div className="mt-auto space-y-3">
        <div
          className={`flex items-start gap-2 rounded-xl border px-3 py-2.5 text-xs ${
            ai?.enabled
              ? "border-brand-200 bg-brand-50/60 text-brand-800 dark:border-brand-900 dark:bg-brand-950/40 dark:text-brand-200"
              : "border-zinc-200 text-zinc-500 dark:border-zinc-800 dark:text-zinc-400"
          }`}
        >
          <Sparkles className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          <div className="min-w-0">
            <p className="font-medium">{ai?.enabled ? "AI 輔助已開啟" : "AI 輔助未開啟"}</p>
            <p className="truncate opacity-80" title={ai?.enabled ? `${ai.provider} · ${ai.model}` : (ai?.reason ?? "")}>
              {ai?.enabled ? `${ai.provider === "anthropic" ? "Claude" : "OpenAI 相容"} · ${ai.model}` : "設定 LLM_PROVIDER 後開啟"}
            </p>
          </div>
        </div>
        <button
          onClick={() => setPicking(true)}
          className="flex w-full items-center gap-2.5 rounded-xl px-2 py-1.5 text-left hover:bg-zinc-100 dark:hover:bg-zinc-800/70"
        >
          {annotator ? <Initials name={annotator.name} /> : <span className="size-7 rounded-full border border-dashed border-zinc-300" />}
          <span className="min-w-0 flex-1">
            <span className="block text-xs text-zinc-500 dark:text-zinc-400">目前的標注者</span>
            <span className="block truncate text-sm font-medium">{annotator?.name ?? "還沒選擇"}</span>
          </span>
        </button>
        <ThemeSwitch />
        <p className="px-2 text-[11px] text-zinc-400">v{meta?.version ?? "…"}</p>
      </div>
      <Modal open={picking} onClose={() => setPicking(false)} title="切換標注者">
        <AnnotatorPicker onPicked={() => setPicking(false)} />
      </Modal>
    </nav>
  );
}

export function Layout() {
  const [drawer, setDrawer] = useState(false);
  const location = useLocation();
  useEffect(() => setDrawer(false), [location.pathname]);
  return (
    <div className="min-h-dvh lg:grid lg:grid-cols-[15.5rem_1fr]">
      <div className="hidden border-r border-zinc-200/80 bg-white/70 lg:block dark:border-zinc-800 dark:bg-zinc-900/60">
        <aside className="sticky top-0 h-dvh">
          <Sidebar />
        </aside>
      </div>
      <header className="sticky top-0 z-30 flex items-center gap-3 border-b border-zinc-200 bg-white/80 px-4 py-2.5 backdrop-blur lg:hidden dark:border-zinc-800 dark:bg-zinc-900/80">
        <button className="btn btn-ghost px-2" onClick={() => setDrawer(true)} aria-label="開啟選單">
          <Menu className="size-5" />
        </button>
        <Logo className="size-7" />
        <span className="text-sm font-semibold">Simply Label</span>
      </header>
      {drawer && (
        <div className="fixed inset-0 z-40 lg:hidden">
          <div className="absolute inset-0 bg-zinc-950/40" onClick={() => setDrawer(false)} />
          <aside className="absolute inset-y-0 left-0 w-72 border-r border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-900">
            <button className="btn btn-ghost absolute top-3 right-2 px-2" onClick={() => setDrawer(false)} aria-label="關閉選單">
              <X className="size-4" />
            </button>
            <Sidebar onNavigate={() => setDrawer(false)} />
          </aside>
        </div>
      )}
      <main className="min-w-0">
        <Outlet />
      </main>
      <Toaster />
    </div>
  );
}
