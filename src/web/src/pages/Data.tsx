import { Bot, Download, FileArchive, FileJson, FileSpreadsheet, MessagesSquare, Square, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, urls } from "../api";
import { PageHeader, ProgressBar, Spinner } from "../components/ui";
import { percent } from "../lib/format";
import { useErrorToast, useSession } from "../lib/session";
import type { ImportReport, JobStatus } from "../types";
import { useProject } from "./ProjectLayout";

const SAMPLES = [
  { name: "textDataExample.csv", label: "舊版文字範本（Text）" },
  { name: "labelDataExample.csv", label: "舊版含標注範本（Level, Classes, …）" },
  { name: "sentiment-sample.jsonl", label: "情緒分類範例（JSONL）" },
  { name: "image-text-sample.zip", label: "圖文範例（zip）" },
];

type ExportChoice = { id: string; icon: React.ReactNode; title: string; body: string; params: Record<string, string> };

const EXPORTS: ExportChoice[] = [
  { id: "csv", icon: <FileSpreadsheet className="size-5" />, title: "CSV（每筆標注一列）", body: "每位標注者的每筆標注各一列，Excel 可以直接開。", params: { format: "csv", mode: "annotations" } },
  { id: "csv-items", icon: <FileSpreadsheet className="size-5" />, title: "CSV（每筆資料一列）", body: "多人標注合併成過半數的共識，適合直接拿去訓練分類器。", params: { format: "csv", mode: "items" } },
  { id: "jsonl", icon: <FileJson className="size-5" />, title: "JSONL", body: "每筆資料一行：內容、所有標注、共識。可以再匯入。", params: { format: "jsonl" } },
  { id: "zip", icon: <FileArchive className="size-5" />, title: "zip（JSONL＋媒體檔）", body: "連同圖片、音訊一起打包，換一台機器匯入就能接著標。", params: { format: "zip" } },
  { id: "chat", icon: <MessagesSquare className="size-5" />, title: "聊天格式 JSONL（微調／評估）", body: "messages 格式：system 是標注說明、user 是資料（含圖片）、assistant 是共識標注。OpenAI 微調、TRL、LLaMA-Factory 都讀得懂。", params: { format: "chat-jsonl" } },
];

function ImportCard() {
  const { project } = useProject();
  const { meta, toast } = useSession();
  const showError = useErrorToast();
  const [drag, setDrag] = useState(false);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const input = useRef<HTMLInputElement>(null);

  async function send(file: File) {
    setBusy(true);
    setReport(null);
    try {
      const r = await api.importFile(project.id, file);
      setReport(r);
      toast("ok", `匯入完成：新增 ${r.created} 筆`);
    } catch (e) {
      showError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card p-5">
      <h2 className="mb-1 text-base font-semibold">匯入資料</h2>
      <p className="mb-4 text-sm text-zinc-500 dark:text-zinc-400">
        支援 CSV、JSONL 和 zip（圖片或音訊，可以附 items.csv／items.jsonl 清單）。和標注欄位同名的欄位會當成已經標好的值；重複的資料會自動略過。有任何一列不對，整批都不會匯入，改好再傳一次就好。
      </p>
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setDrag(true);
        }}
        onDragLeave={() => setDrag(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDrag(false);
          const f = e.dataTransfer.files[0];
          if (f) send(f);
        }}
        className={`flex flex-col items-center justify-center rounded-2xl border-2 border-dashed px-6 py-10 text-center transition ${
          drag ? "border-brand-500 bg-brand-50 dark:bg-brand-950/40" : "border-zinc-200 dark:border-zinc-800"
        }`}
      >
        <span className="mb-3 grid size-11 place-items-center rounded-xl bg-brand-50 text-brand-600 dark:bg-brand-950 dark:text-brand-400">
          {busy ? <Spinner className="size-5" /> : <Upload className="size-5" />}
        </span>
        <p className="text-sm font-medium">把檔案拖到這裡</p>
        <p className="mb-3 text-xs text-zinc-500">或</p>
        <button className="btn btn-secondary" onClick={() => input.current?.click()} disabled={busy}>
          選擇檔案
        </button>
        <input
          ref={input}
          type="file"
          className="hidden"
          accept=".csv,.txt,.jsonl,.ndjson,.json,.zip"
          aria-label="選擇要匯入的檔案"
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) send(f);
            e.target.value = "";
          }}
        />
        <p className="mt-3 text-xs text-zinc-400">上限 {meta?.maxUploadMb ?? 200} MB</p>
      </div>
      {report && (
        <dl className="mt-4 grid grid-cols-2 gap-2 text-sm sm:grid-cols-4">
          {[
            ["新增", report.created],
            ["重複略過", report.duplicates],
            ["附帶的標注", report.annotations],
            ["媒體檔", report.media],
          ].map(([k, v]) => (
            <div key={k} className="rounded-xl bg-zinc-50 px-3 py-2 dark:bg-zinc-800/60">
              <dt className="text-xs text-zinc-500">{k}</dt>
              <dd className="text-lg font-semibold tabular-nums">{v}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="mt-5 border-t border-zinc-100 pt-4 dark:border-zinc-800">
        <p className="section-title mb-2">範本和範例</p>
        <div className="flex flex-wrap gap-2">
          <a className="btn btn-secondary text-xs" href={urls.importTemplate(project.id)}>
            <Download className="size-3.5" />
            這個專案的 CSV 範本
          </a>
          {SAMPLES.map((s) => (
            <a key={s.name} className="btn btn-ghost text-xs" href={urls.sample(s.name)}>
              <Download className="size-3.5" />
              {s.label}
            </a>
          ))}
        </div>
      </div>
    </section>
  );
}

function ExportCard() {
  const { project } = useProject();
  const [choice, setChoice] = useState("csv");
  const [excludeAuto, setExcludeAuto] = useState(false);
  const [includeHidden, setIncludeHidden] = useState(false);
  const selected = EXPORTS.find((e) => e.id === choice)!;
  const href = urls.export(project.id, { ...selected.params, excludeAuto, includeHidden });
  return (
    <section className="card p-5">
      <h2 className="mb-1 text-base font-semibold">匯出</h2>
      <p className="mb-4 text-sm text-zinc-500 dark:text-zinc-400">共識的規則是「過半數」：分數取平均、單選要超過一半的人選同一個、多選和標籤要超過一半的人選。</p>
      <div className="grid gap-2" role="radiogroup" aria-label="匯出格式">
        {EXPORTS.map((e) => (
          <button
            key={e.id}
            role="radio"
            aria-checked={choice === e.id}
            onClick={() => setChoice(e.id)}
            className={`flex gap-3 rounded-xl border p-3 text-left transition ${
              choice === e.id ? "border-brand-500 bg-brand-50 ring-1 ring-brand-500 dark:bg-brand-950/50" : "border-zinc-200 hover:border-zinc-300 dark:border-zinc-800 dark:hover:border-zinc-700"
            }`}
          >
            <span className="mt-0.5 text-brand-600 dark:text-brand-400">{e.icon}</span>
            <span>
              <span className="block text-sm font-semibold">{e.title}</span>
              <span className="block text-xs text-zinc-500 dark:text-zinc-400">{e.body}</span>
            </span>
          </button>
        ))}
      </div>
      <div className="mt-4 space-y-2 text-sm">
        <label className="flex items-center gap-2">
          <input type="checkbox" className="accent-brand-600" checked={excludeAuto} onChange={(e) => setExcludeAuto(e.target.checked)} />
          共識不算「高信心自動接受」的 AI 標注（做評估資料時用）
        </label>
        <label className="flex items-center gap-2">
          <input type="checkbox" className="accent-brand-600" checked={includeHidden} onChange={(e) => setIncludeHidden(e.target.checked)} />
          包含隱藏的資料
        </label>
      </div>
      <a className="btn btn-primary mt-4 w-full" href={href}>
        <Download className="size-4" />
        下載
      </a>
    </section>
  );
}

function BatchCard() {
  const { project } = useProject();
  const { meta } = useSession();
  const showError = useErrorToast();
  const [limit, setLimit] = useState(50);
  const [job, setJob] = useState<JobStatus | null>(null);
  const threshold = project.ai.autoAcceptThreshold;

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    let alive = true;
    const poll = async () => {
      try {
        const r = await api.batchStatus(project.id);
        if (!alive) return;
        setJob(r.job);
        if (r.job?.running) timer = setTimeout(poll, 1000);
      } catch {
        // 下次再試
      }
    };
    poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [project.id, job?.running]);

  async function start() {
    try {
      setJob(await api.startBatch(project.id, limit, null));
    } catch (e) {
      showError(e);
    }
  }

  const enabled = meta?.ai.enabled;
  return (
    <section className="card p-5">
      <div className="mb-1 flex items-center gap-2">
        <Bot className="size-5 text-ai-600" />
        <h2 className="text-base font-semibold">AI 批次預標</h2>
      </div>
      <p className="mb-4 text-sm text-zinc-500 dark:text-zinc-400">
        讓模型先看還沒人標、也還沒有建議的資料。
        {threshold != null
          ? `信心 ≥ ${percent(threshold)} 的直接存成「AI · 模型名稱」標注者的標注，其他的留給人在標注頁確認。`
          : "目前沒有設定自動接受門檻，所有建議都會留給人確認（可以在專案設定打開）。"}
      </p>
      {!enabled ? (
        <p className="rounded-xl bg-zinc-50 p-3 text-sm text-zinc-500 dark:bg-zinc-800/60">AI 輔助沒有開啟。設定 LLM_PROVIDER 等環境變數後重新啟動。</p>
      ) : job?.running ? (
        <div className="space-y-3">
          <ProgressBar value={job.total ? job.done / job.total : 1} label="批次預標進度" />
          <p className="text-sm tabular-nums">
            {job.done} / {job.total} · 自動接受 {job.autoAccepted} · 失敗 {job.failed}
          </p>
          <button className="btn btn-secondary" onClick={() => api.cancelBatch(project.id).catch(showError)}>
            <Square className="size-3.5" />
            停止
          </button>
        </div>
      ) : (
        <div className="space-y-3">
          <div className="flex items-end gap-2">
            <div className="w-32">
              <label className="label" htmlFor="limit">
                最多幾筆
              </label>
              <input id="limit" type="number" min={1} max={5000} className="input" value={limit} onChange={(e) => setLimit(Number(e.target.value) || 1)} />
            </div>
            <button className="btn btn-primary" onClick={start}>
              <Bot className="size-4" />
              開始預標
            </button>
          </div>
          {job && (
            <div className="rounded-xl bg-zinc-50 p-3 text-sm dark:bg-zinc-800/60">
              <p className="tabular-nums">
                上一次：{job.done} / {job.total} 筆，自動接受 {job.autoAccepted}，失敗 {job.failed}
                {job.cancelled && "（中途停止）"}
              </p>
              {job.errors.length > 0 && (
                <ul className="mt-1 list-disc pl-4 text-xs text-red-600 dark:text-red-400">
                  {job.errors.slice(0, 5).map((e, i) => (
                    <li key={i}>{e}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}

export function DataPage() {
  const { project } = useProject();
  const { can } = useSession();
  return (
    <div className="mx-auto max-w-6xl px-4 py-8 sm:px-8">
      <PageHeader title="匯入匯出" subtitle={project.name} />
      {!can("review") ? (
        <p className="card p-5 text-sm text-zinc-500 dark:text-zinc-400">匯入匯出要審核者或管理員的帳號。</p>
      ) : (
        <div className="grid gap-5 lg:grid-cols-2">
          <div className="space-y-5">
            {can("admin") && <ImportCard />}
            <BatchCard />
          </div>
          <ExportCard />
        </div>
      )}
    </div>
  );
}
