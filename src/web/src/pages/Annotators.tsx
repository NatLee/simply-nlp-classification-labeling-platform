import { Bot, Check, Pencil, Trash2, UserPlus, X } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { PageHeader } from "../components/ui";
import { dateTime } from "../lib/format";
import { useErrorToast, useSession } from "../lib/session";
import type { Annotator } from "../types";

function Row({ a }: { a: Annotator }) {
  const { annotator, setAnnotatorId, reloadAnnotators, toast } = useSession();
  const showError = useErrorToast();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(a.name);
  const isAi = a.kind === "ai";

  async function rename() {
    try {
      await api.renameAnnotator(a.id, name);
      await reloadAnnotators();
      setEditing(false);
    } catch (e) {
      showError(e);
    }
  }

  async function remove() {
    try {
      await api.deleteAnnotator(a.id);
      if (annotator?.id === a.id) setAnnotatorId(null);
      await reloadAnnotators();
      toast("ok", `已刪除「${a.name}」`);
    } catch (e) {
      showError(e);
    }
  }

  return (
    <li className="flex items-center gap-3 px-4 py-3">
      <span className={`grid size-8 place-items-center rounded-full text-xs font-semibold ${isAi ? "bg-ai-100 text-ai-700 dark:bg-ai-950 dark:text-ai-400" : "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"}`}>
        {isAi ? <Bot className="size-4" /> : a.name.slice(0, 1).toUpperCase()}
      </span>
      <div className="min-w-0 flex-1">
        {editing ? (
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              rename();
            }}
          >
            <input className="input max-w-xs" value={name} onChange={(e) => setName(e.target.value)} autoFocus aria-label="新的名字" />
            <button className="btn btn-primary px-2" aria-label="儲存">
              <Check className="size-4" />
            </button>
            <button type="button" className="btn btn-ghost px-2" onClick={() => setEditing(false)} aria-label="取消">
              <X className="size-4" />
            </button>
          </form>
        ) : (
          <>
            <p className="truncate text-sm font-medium">
              {a.name}
              {annotator?.id === a.id && <span className="chip ml-2 border-brand-200 bg-brand-50 text-brand-700 dark:border-brand-900 dark:bg-brand-950 dark:text-brand-300">目前的標注者</span>}
            </p>
            <p className="text-xs text-zinc-500 dark:text-zinc-400">
              {isAi ? "模型標注者（批次預標時自動建立）" : "人"} · {dateTime(a.createdAt)} 建立
            </p>
          </>
        )}
      </div>
      {!isAi && !editing && (
        <div className="flex gap-1">
          {annotator?.id !== a.id && (
            <button className="btn btn-secondary text-xs" onClick={() => setAnnotatorId(a.id)}>
              切換成這個人
            </button>
          )}
          <button className="btn btn-ghost px-2" onClick={() => setEditing(true)} aria-label={`改 ${a.name} 的名字`}>
            <Pencil className="size-4" />
          </button>
          <button className="btn btn-ghost px-2 text-red-600" onClick={remove} aria-label={`刪除 ${a.name}`} title="已經有標注的標注者不能刪">
            <Trash2 className="size-4" />
          </button>
        </div>
      )}
    </li>
  );
}

export function AnnotatorsPage() {
  const { annotators, reloadAnnotators, setAnnotatorId } = useSession();
  const showError = useErrorToast();
  const [name, setName] = useState("");

  async function create(e: React.FormEvent) {
    e.preventDefault();
    try {
      const a = await api.createAnnotator(name);
      await reloadAnnotators();
      setAnnotatorId(a.id);
      setName("");
    } catch (err) {
      showError(err);
    }
  }

  return (
    <div className="mx-auto max-w-3xl px-4 py-8 sm:px-8">
      <PageHeader title="標注者" subtitle="每個人的標注分開記錄。這裡沒有密碼，選誰就以誰的身分標注；請放在信任的網路裡使用。" />
      <form onSubmit={create} className="card mb-5 flex gap-2 p-4">
        <input className="input" placeholder="新的標注者名字" value={name} onChange={(e) => setName(e.target.value)} maxLength={40} aria-label="新的標注者名字" />
        <button className="btn btn-primary" disabled={!name.trim()}>
          <UserPlus className="size-4" />
          新增
        </button>
      </form>
      <ul className="card divide-y divide-zinc-100 dark:divide-zinc-800">
        {annotators.length === 0 && <li className="px-4 py-6 text-center text-sm text-zinc-500">還沒有標注者</li>}
        {annotators.map((a) => (
          <Row key={a.id} a={a} />
        ))}
      </ul>
    </div>
  );
}
