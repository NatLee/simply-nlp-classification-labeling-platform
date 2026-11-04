import { ChevronLeft, ChevronRight, Eye, EyeOff, Image as ImageIcon, Music, PenLine, Search } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { api, urls } from "../api";
import { EmptyState, PageHeader, Spinner } from "../components/ui";
import { useAsync } from "../lib/hooks";
import { useErrorToast, useSession } from "../lib/session";
import type { ItemFilter } from "../types";
import { useProject } from "./ProjectLayout";

const FILTERS: { v: ItemFilter; label: string }[] = [
  { v: "all", label: "全部" },
  { v: "unlabeled", label: "還沒人標" },
  { v: "labeled", label: "有標注" },
  { v: "mine", label: "我標過的" },
  { v: "pending", label: "AI 建議待確認" },
  { v: "hidden", label: "已隱藏" },
];

const PER_PAGE = 50;

export function ItemsPage() {
  const { project } = useProject();
  const { annotator } = useSession();
  const showError = useErrorToast();
  const [filter, setFilter] = useState<ItemFilter>("all");
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const { data, loading, reload } = useAsync(
    () => api.items(project.id, { filter, q: search, annotator: annotator?.id, page, perPage: PER_PAGE }),
    [project.id, filter, search, annotator?.id, page],
  );
  const pages = data ? Math.max(1, Math.ceil(data.total / PER_PAGE)) : 1;

  async function toggleHidden(id: number, hidden: boolean) {
    try {
      await api.setHidden(id, !hidden);
      reload();
    } catch (e) {
      showError(e);
    }
  }

  return (
    <div className="mx-auto max-w-6xl px-4 py-8 sm:px-8">
      <PageHeader title="資料" subtitle={`${project.name} · 共 ${data?.total ?? "…"} 筆符合條件`} />
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="flex flex-wrap gap-1 rounded-xl bg-zinc-100 p-1 dark:bg-zinc-900" role="tablist">
          {FILTERS.map((f) => (
            <button
              key={f.v}
              role="tab"
              aria-selected={filter === f.v}
              onClick={() => {
                setFilter(f.v);
                setPage(1);
              }}
              disabled={f.v === "mine" && !annotator}
              className={`rounded-lg px-3 py-1 text-sm font-medium transition disabled:opacity-40 ${
                filter === f.v ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-800 dark:text-zinc-50" : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
              }`}
            >
              {f.label}
            </button>
          ))}
        </div>
        <form
          className="relative ml-auto w-full sm:w-72"
          onSubmit={(e) => {
            e.preventDefault();
            setSearch(query.trim());
            setPage(1);
          }}
        >
          <Search className="pointer-events-none absolute top-2.5 left-3 size-4 text-zinc-400" />
          <input className="input pl-9" placeholder="搜尋文字，按 Enter" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="搜尋文字" />
        </form>
      </div>
      <div className="card overflow-hidden">
        {loading && !data ? (
          <div className="grid h-48 place-items-center text-zinc-400">
            <Spinner className="size-6" />
          </div>
        ) : !data || data.items.length === 0 ? (
          <EmptyState icon={<Search className="size-6" />} title="沒有符合的資料">
            換個篩選條件，或到「匯入匯出」上傳資料。
          </EmptyState>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[40rem]">
              <thead className="border-b border-zinc-100 bg-zinc-50/60 dark:border-zinc-800 dark:bg-zinc-900/60">
                <tr>
                  <th className="th w-16">#</th>
                  <th className="th w-16">媒體</th>
                  <th className="th">文字</th>
                  <th className="th w-24 text-right">標注數</th>
                  <th className="th w-36" />
                </tr>
              </thead>
              <tbody className="divide-y divide-zinc-100 dark:divide-zinc-800">
                {data.items.map((it) => (
                  <tr key={it.id} className={`hover:bg-zinc-50/70 dark:hover:bg-zinc-800/40 ${it.hidden ? "opacity-60" : ""}`}>
                    <td className="td tabular-nums text-zinc-400">{it.id}</td>
                    <td className="td">
                      {it.media?.kind === "image" ? (
                        <img src={urls.media(it.media.id)} alt="" className="size-10 rounded-lg border border-zinc-200 object-cover dark:border-zinc-700" loading="lazy" />
                      ) : it.media?.kind === "audio" ? (
                        <Music className="size-4 text-zinc-400" aria-label="音訊" />
                      ) : (
                        <ImageIcon className="size-4 text-zinc-200 dark:text-zinc-700" aria-hidden />
                      )}
                    </td>
                    <td className="td">
                      <p className="line-clamp-2 max-w-2xl">{it.text || <span className="text-zinc-400">（沒有文字）</span>}</p>
                    </td>
                    <td className="td text-right tabular-nums">{it.annotationCount}</td>
                    <td className="td text-right whitespace-nowrap">
                      <Link className="btn btn-ghost px-2" to={`/p/${project.id}?item=${it.id}`} title="標注這筆">
                        <PenLine className="size-4" />
                        標注
                      </Link>
                      <button className="btn btn-ghost px-2" onClick={() => toggleHidden(it.id, it.hidden)} title={it.hidden ? "顯示（放回標注佇列）" : "隱藏（不出現在標注佇列）"}>
                        {it.hidden ? <Eye className="size-4" /> : <EyeOff className="size-4" />}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      {pages > 1 && (
        <div className="mt-4 flex items-center justify-end gap-2 text-sm">
          <button className="btn btn-secondary px-2" disabled={page <= 1} onClick={() => setPage((p) => p - 1)} aria-label="上一頁">
            <ChevronLeft className="size-4" />
          </button>
          <span className="tabular-nums text-zinc-500">
            {page} / {pages}
          </span>
          <button className="btn btn-secondary px-2" disabled={page >= pages} onClick={() => setPage((p) => p + 1)} aria-label="下一頁">
            <ChevronRight className="size-4" />
          </button>
        </div>
      )}
    </div>
  );
}
