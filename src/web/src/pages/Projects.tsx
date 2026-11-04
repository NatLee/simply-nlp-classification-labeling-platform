import { ArrowRight, Database, FolderPlus, History, Image as ImageIcon, Layers, SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { api } from "../api";
import { EmptyState, Modal, PageHeader, ProgressBar, Spinner } from "../components/ui";
import { FIELD_TYPE_LABELS } from "../lib/form";
import { useAsync } from "../lib/hooks";
import { percent } from "../lib/format";
import { useErrorToast, useSession } from "../lib/session";
import type { Project, Template } from "../types";

const TEMPLATE_ICONS: Record<string, React.ReactNode> = {
  "legacy-emotion": <History className="size-5" />,
  sentiment: <SlidersHorizontal className="size-5" />,
  "image-text": <ImageIcon className="size-5" />,
};

function ProjectCard({ p }: { p: Project }) {
  const c = p.counts ?? { items: 0, visible: 0, labeled: 0 };
  const cover = c.visible ? c.labeled / c.visible : 0;
  return (
    <Link to={`/p/${p.id}`} className="card group flex flex-col p-5 transition hover:-translate-y-0.5 hover:border-brand-300 hover:shadow-lg dark:hover:border-brand-800">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="truncate text-base font-semibold">{p.name}</h3>
          <p className="mt-0.5 line-clamp-2 min-h-10 text-sm text-zinc-500 dark:text-zinc-400">{p.description || "沒有說明"}</p>
        </div>
        <ArrowRight className="size-4 shrink-0 text-zinc-300 transition group-hover:translate-x-0.5 group-hover:text-brand-600" />
      </div>
      <div className="mt-4 flex flex-wrap gap-1.5">
        {p.schema.fields.slice(0, 4).map((f) => (
          <span key={f.id} className="chip border-zinc-200 text-zinc-600 dark:border-zinc-700 dark:text-zinc-300">
            {f.label}
            <span className="text-zinc-400">· {FIELD_TYPE_LABELS[f.type]}</span>
          </span>
        ))}
        {p.schema.fields.length > 4 && <span className="chip border-zinc-200 text-zinc-400 dark:border-zinc-700">+{p.schema.fields.length - 4}</span>}
      </div>
      <div className="mt-auto pt-5">
        <div className="mb-1.5 flex justify-between text-xs text-zinc-500 dark:text-zinc-400">
          <span>
            {c.labeled} / {c.visible} 筆有標注
          </span>
          <span className="font-medium text-zinc-700 dark:text-zinc-200">{percent(cover)}</span>
        </div>
        <ProgressBar value={cover} label={`${p.name} 的標注覆蓋率`} />
      </div>
    </Link>
  );
}

function CreateProject({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate();
  const showError = useErrorToast();
  const { data: templates } = useAsync(() => api.templates(), []);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [template, setTemplate] = useState<string>("legacy-emotion");
  const [busy, setBusy] = useState(false);

  async function create() {
    setBusy(true);
    try {
      const p = await api.createProject({ name, description, templateId: template });
      onClose();
      navigate(`/p/${p.id}/data`);
    } catch (e) {
      showError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="新增專案"
      wide
      footer={
        <>
          <button className="btn btn-ghost" onClick={onClose}>
            取消
          </button>
          <button className="btn btn-primary" onClick={create} disabled={busy || !name.trim()}>
            {busy && <Spinner />}
            建立專案
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <div>
            <label className="label" htmlFor="pname">
              名稱
            </label>
            <input id="pname" className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：客服對話情緒" autoFocus />
          </div>
          <div>
            <label className="label" htmlFor="pdesc">
              說明（選填）
            </label>
            <input id="pdesc" className="input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
        </div>
        <div>
          <p className="label">標注結構</p>
          <p className="hint mb-2 mt-0">先選一個範本，建立後可以在「專案設定」改欄位。</p>
          <div className="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label="範本">
            {(templates ?? []).map((t: Template) => (
              <button
                key={t.id}
                role="radio"
                aria-checked={template === t.id}
                onClick={() => setTemplate(t.id)}
                className={`flex flex-col gap-2 rounded-xl border p-3 text-left transition ${
                  template === t.id
                    ? "border-brand-500 bg-brand-50 ring-1 ring-brand-500 dark:bg-brand-950/50"
                    : "border-zinc-200 hover:border-zinc-300 dark:border-zinc-800 dark:hover:border-zinc-700"
                }`}
              >
                <span className="text-brand-600 dark:text-brand-400">{TEMPLATE_ICONS[t.id] ?? <Layers className="size-5" />}</span>
                <span className="text-sm font-semibold">{t.name}</span>
                <span className="text-xs text-zinc-500 dark:text-zinc-400">{t.description}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    </Modal>
  );
}

function LegacyImport({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate();
  const showError = useErrorToast();
  const { toast } = useSession();
  const [file, setFile] = useState<File | null>(null);
  const [name, setName] = useState("舊版情緒評分資料");
  const [busy, setBusy] = useState(false);

  async function run() {
    if (!file) return;
    setBusy(true);
    try {
      const r = await api.importLegacy(file, name);
      toast("ok", `匯入 ${r.items} 筆文字、${r.annotations} 筆標注`, [
        ...(r.hiddenItems ? [`${r.hiddenItems} 筆是舊版隱藏的（flag=0），不會出現在標注佇列`] : []),
        ...(r.orphanLabels ? [`${r.orphanLabels} 筆標注找不到對應的文字，已略過`] : []),
        ...(r.addedOptions.length ? [`舊資料裡有舊版沒有的選項，已加入：${r.addedOptions.join("、")}`] : []),
      ]);
      onClose();
      navigate(`/p/${r.projectId}/stats`);
    } catch (e) {
      showError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="匯入舊版 emoLabel.db"
      footer={
        <>
          <button className="btn btn-ghost" onClick={onClose}>
            取消
          </button>
          <button className="btn btn-primary" onClick={run} disabled={!file || busy}>
            {busy && <Spinner />}
            匯入
          </button>
        </>
      }
    >
      <div className="space-y-4 text-sm">
        <p className="text-zinc-500 dark:text-zinc-400">
          舊版（Flask 版）的資料庫會變成一個新專案：評分、情緒、類型、自訂標籤都會保留，隱藏的文字（flag=0）也會帶過來。舊檔只會被讀取，不會被改動。
        </p>
        <div>
          <label className="label" htmlFor="lname">
            專案名稱
          </label>
          <input id="lname" className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div>
          <label className="label" htmlFor="lfile">
            資料庫檔
          </label>
          <input id="lfile" type="file" accept=".db,.sqlite,.sqlite3" className="input file:mr-3 file:rounded-md file:border-0 file:bg-brand-50 file:px-3 file:py-1 file:text-brand-700" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
          <p className="hint">舊版存的時間沒有時區，會當成和這台電腦同一個時區。</p>
        </div>
      </div>
    </Modal>
  );
}

export function ProjectsPage() {
  const { data: projects, loading } = useAsync(() => api.projects(), []);
  const { can } = useSession();
  const [creating, setCreating] = useState(false);
  const [legacy, setLegacy] = useState(false);
  return (
    <div className="mx-auto max-w-6xl px-4 py-8 sm:px-8">
      <PageHeader
        title="所有專案"
        subtitle="每個專案有自己的資料和標注結構。文字、圖片、文字加圖片都可以標。"
        actions={
          can("admin") && (
          <>
            <button className="btn btn-secondary" onClick={() => setLegacy(true)}>
              <Database className="size-4" />
              匯入舊版 emoLabel.db
            </button>
            <button className="btn btn-primary" onClick={() => setCreating(true)}>
              <FolderPlus className="size-4" />
              新增專案
            </button>
          </>
          )
        }
      />
      {loading && !projects ? (
        <div className="grid h-48 place-items-center text-zinc-400">
          <Spinner className="size-6" />
        </div>
      ) : projects && projects.length > 0 ? (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {projects.map((p) => (
            <ProjectCard key={p.id} p={p} />
          ))}
        </div>
      ) : (
        <div className="card">
          <EmptyState icon={<FolderPlus className="size-6" />} title="還沒有專案">
            建一個專案，選好標注結構，再匯入要標的資料。用過舊版的話，也可以直接匯入舊的 emoLabel.db。
            {can("admin") && (
              <div className="mt-4 flex justify-center gap-2">
                <button className="btn btn-primary" onClick={() => setCreating(true)}>
                  新增專案
                </button>
              </div>
            )}
          </EmptyState>
        </div>
      )}
      <CreateProject open={creating} onClose={() => setCreating(false)} />
      <LegacyImport open={legacy} onClose={() => setLegacy(false)} />
    </div>
  );
}
