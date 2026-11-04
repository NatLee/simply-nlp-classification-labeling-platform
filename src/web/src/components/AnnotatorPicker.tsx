import { Check, UserPlus } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { useErrorToast, useSession } from "../lib/session";

/** 選擇或新增標注者。每個人的標注分開存，一致性統計靠這個區分是誰標的。 */
export function AnnotatorPicker({ onPicked }: { onPicked?: () => void }) {
  const { annotators, annotator, setAnnotatorId, reloadAnnotators } = useSession();
  const showError = useErrorToast();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const humans = annotators.filter((a) => a.kind === "human");

  async function create(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    try {
      const a = await api.createAnnotator(name.trim());
      await reloadAnnotators();
      setAnnotatorId(a.id);
      setName("");
      onPicked?.();
    } catch (err) {
      showError(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <p className="text-sm text-zinc-500 dark:text-zinc-400">
        選一個名字開始標注。每個人的標注分開記錄，之後可以比較標注者之間的一致性。
      </p>
      {humans.length > 0 && (
        <ul className="grid gap-2 sm:grid-cols-2">
          {humans.map((a) => (
            <li key={a.id}>
              <button
                className={`flex w-full items-center gap-2.5 rounded-xl border px-3 py-2 text-left text-sm transition ${
                  annotator?.id === a.id
                    ? "border-brand-500 bg-brand-50 dark:bg-brand-950/50"
                    : "border-zinc-200 hover:border-zinc-300 dark:border-zinc-800 dark:hover:border-zinc-700"
                }`}
                onClick={() => {
                  setAnnotatorId(a.id);
                  onPicked?.();
                }}
              >
                <span className="grid size-7 place-items-center rounded-full bg-zinc-900 text-xs font-semibold text-white dark:bg-zinc-100 dark:text-zinc-900">
                  {a.name.slice(0, 1).toUpperCase()}
                </span>
                <span className="flex-1 truncate font-medium">{a.name}</span>
                {annotator?.id === a.id && <Check className="size-4 text-brand-600" />}
              </button>
            </li>
          ))}
        </ul>
      )}
      <form onSubmit={create} className="flex gap-2">
        <input
          className="input"
          placeholder="新的標注者名字"
          value={name}
          onChange={(e) => setName(e.target.value)}
          maxLength={40}
          aria-label="新的標注者名字"
        />
        <button className="btn btn-primary" disabled={busy || !name.trim()}>
          <UserPlus className="size-4" />
          新增
        </button>
      </form>
    </div>
  );
}
