import { ChartColumn, Info } from "lucide-react";
import { api } from "../api";
import { Bars, EmptyState, PageHeader, Spinner, StatTile } from "../components/ui";
import { FIELD_TYPE_LABELS } from "../lib/form";
import { useAsync } from "../lib/hooks";
import { agreementLevel, fixed, percent } from "../lib/format";
import { useSession } from "../lib/session";
import type { Agreement, FieldStats, Source } from "../types";
import { useProject } from "./ProjectLayout";

const SOURCE_LABELS: Record<Source, string> = {
  human: "人工",
  ai_assisted: "看過 AI 建議後送出",
  ai_auto: "AI 高信心自動接受",
  import: "匯入的檔案",
  legacy: "舊版資料",
};

const TONE: Record<string, string> = {
  good: "bg-brand-50 text-brand-800 dark:bg-brand-950 dark:text-brand-200",
  ok: "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-200",
  weak: "bg-amber-50 text-amber-800 dark:bg-amber-950 dark:text-amber-200",
  bad: "bg-red-50 text-red-700 dark:bg-red-950 dark:text-red-300",
};

function Metric({ name, value, help }: { name: string; value: number | null; help: string }) {
  const level = agreementLevel(value);
  return (
    <div className="rounded-xl border border-zinc-200 p-3 dark:border-zinc-800">
      <p className="flex items-center gap-1 text-xs font-medium text-zinc-500 dark:text-zinc-400" title={help}>
        {name}
        <Info className="size-3" aria-hidden />
      </p>
      <p className="mt-1 text-xl font-semibold tabular-nums">{fixed(value, 3)}</p>
      {level ? <span className={`chip mt-1 border-transparent ${TONE[level.tone]}`}>{level.label}</span> : <span className="text-xs text-zinc-400">資料不夠，無法計算</span>}
    </div>
  );
}

function AgreementBlock({ field, ag }: { field: FieldStats; ag: Agreement }) {
  if (ag.itemsUsed === 0) {
    return <p className="text-sm text-zinc-500 dark:text-zinc-400">還沒有資料被兩個人以上標過，等多人標同一筆之後才算得出一致性。</p>;
  }
  return (
    <div className="space-y-3">
      <div className="grid gap-2 sm:grid-cols-2">
        {field.type === "scale" ? (
          <Metric name="Krippendorff's alpha（interval）" value={ag.krippendorffAlpha} help="分數欄位：同一筆資料的分數差多少，和隨便給分比起來小多少。允許每筆的標注人數不同。" />
        ) : (
          <Metric name="Fleiss' kappa" value={ag.fleissKappa} help="所有被標兩次以上的資料一起算，扣掉碰巧選到一樣的機率。不用知道是誰標的，舊版的匿名標注也能算。" />
        )}
        <div className="rounded-xl border border-zinc-200 p-3 dark:border-zinc-800">
          <p className="text-xs font-medium text-zinc-500 dark:text-zinc-400">用來計算的資料</p>
          <p className="mt-1 text-xl font-semibold tabular-nums">{ag.itemsUsed}</p>
          <p className="text-xs text-zinc-400">筆被標兩次以上</p>
        </div>
      </div>
      {ag.pairs.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <caption className="mb-1 text-left text-xs text-zinc-500 dark:text-zinc-400">兩兩比較（Cohen's kappa）：兩位標注者都標過的資料</caption>
            <thead>
              <tr className="border-b border-zinc-100 dark:border-zinc-800">
                <th className="th">標注者</th>
                <th className="th text-right">共同資料</th>
                <th className="th text-right">一樣的比例</th>
                <th className="th text-right">kappa</th>
                <th className="th" />
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-100 dark:divide-zinc-800">
              {ag.pairs.map((p) => {
                const level = agreementLevel(p.cohenKappa);
                return (
                  <tr key={`${p.a}|${p.b}`}>
                    <td className="td">
                      {p.a} <span className="text-zinc-400">×</span> {p.b}
                    </td>
                    <td className="td text-right tabular-nums">{p.n}</td>
                    <td className="td text-right tabular-nums">{p.percent == null ? "—" : percent(p.percent)}</td>
                    <td className="td text-right font-medium tabular-nums">{fixed(p.cohenKappa, 3)}</td>
                    <td className="td">{level && <span className={`chip border-transparent ${TONE[level.tone]}`}>{level.label}</span>}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export function StatsPage() {
  const { project } = useProject();
  const { annotator } = useSession();
  const { data: s, loading } = useAsync(() => api.stats(project.id, annotator?.id), [project.id, annotator?.id]);
  if (loading && !s) {
    return (
      <div className="grid h-64 place-items-center text-zinc-400">
        <Spinner className="size-6" />
      </div>
    );
  }
  if (!s) return null;
  const sources = Object.entries(s.sources) as [Source, number][];
  const totalAnnotations = sources.reduce((a, [, n]) => a + n, 0);
  return (
    <div className="mx-auto max-w-6xl px-4 py-8 sm:px-8">
      <PageHeader title="進度與一致性" subtitle={project.name} />
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatTile label="可標注的資料" value={s.items.visible} hint={s.items.hidden ? `另有 ${s.items.hidden} 筆隱藏` : "沒有隱藏的資料"} />
        <StatTile label="覆蓋率" value={percent(s.items.coverRate, 1)} hint={`${s.items.labeled} 筆至少被標過一次`} />
        <StatTile label="每筆平均被標次數" value={fixed(s.items.meanLabels)} hint={`標準差 ${fixed(s.items.stdLabels)}`} />
        <StatTile label="我標了" value={s.mine ?? "—"} hint={annotator ? `以 ${annotator.name} 的身分` : "還沒選標注者"} />
      </div>

      <div className="mt-5 grid gap-5 lg:grid-cols-2">
        <section className="card p-5">
          <h2 className="mb-4 text-sm font-semibold">每位標注者標了幾筆</h2>
          <Bars
            unit="筆"
            rows={[
              ...s.annotators.map((a) => ({ key: String(a.id), label: a.name, count: a.count })),
              ...(s.anonymous ? [{ key: "anon", label: "（沒有標注者身分）", count: s.anonymous }] : []),
            ]}
          />
        </section>
        <section className="card p-5">
          <h2 className="mb-4 text-sm font-semibold">標注從哪裡來</h2>
          <Bars unit="筆" total={totalAnnotations} rows={sources.map(([k, n]) => ({ key: k, label: SOURCE_LABELS[k] ?? k, count: n }))} />
          {Object.keys(s.suggestions).length > 0 && (
            <p className="mt-4 text-xs text-zinc-500 dark:text-zinc-400">
              AI 建議：
              {Object.entries(s.suggestions)
                .map(([k, n]) => `${{ pending: "待確認", accepted: "接受", modified: "修改後接受", rejected: "忽略", auto: "自動接受" }[k] ?? k} ${n}`)
                .join("、")}
            </p>
          )}
        </section>
      </div>

      <h2 className="mt-8 mb-3 text-base font-semibold">各欄位</h2>
      {s.fields.length === 0 ? (
        <div className="card">
          <EmptyState icon={<ChartColumn className="size-6" />} title="沒有欄位" />
        </div>
      ) : (
        <div className="grid gap-5 lg:grid-cols-2">
          {s.fields.map((f) => (
            <section key={f.id} className="card p-5">
              <div className="mb-4 flex items-baseline justify-between gap-2">
                <h3 className="text-sm font-semibold">{f.label}</h3>
                <span className="text-xs text-zinc-400">{FIELD_TYPE_LABELS[f.type]}</span>
              </div>
              {f.type === "text" ? (
                <p className="text-sm text-zinc-500">文字說明沒有分布可以畫。</p>
              ) : (
                <Bars rows={f.distribution.map((b) => ({ key: String(b.value), label: b.label, count: b.count }))} />
              )}
              {f.agreement && (
                <div className="mt-5 border-t border-zinc-100 pt-4 dark:border-zinc-800">
                  <AgreementBlock field={f} ag={f.agreement} />
                </div>
              )}
            </section>
          ))}
        </div>
      )}
      <p className="mt-6 text-xs text-zinc-400">
        kappa 和 alpha：1 是完全一致，0 和碰巧差不多。「高度一致」等說法依 Landis &amp; Koch（1977）的分級，只是慣例。
      </p>
    </div>
  );
}
