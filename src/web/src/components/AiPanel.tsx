import { Check, CornerDownLeft, RefreshCw, Sparkles, ThumbsDown, Wand2 } from "lucide-react";
import { formatValue, valuesEqual } from "../lib/form";
import { percent } from "../lib/format";
import type { AiStatus, Project, Suggestion, Values } from "../types";
import { Kbd, Spinner } from "./ui";

const STATUS_TEXT: Record<Suggestion["status"], string> = {
  pending: "等你確認",
  accepted: "已接受",
  modified: "修改後送出",
  rejected: "已忽略",
  auto: "高信心，已自動接受",
};

/** 信心的「高／中／低」分界，只影響顯示，不影響自動接受（那是專案設定的門檻）。 */
const CONFIDENCE_LEVELS: [number, string][] = [
  [0.85, "高"],
  [0.6, "中"],
  [0, "低"],
];

export function ConfidenceMeter({ value, threshold }: { value: number | null; threshold: number | null }) {
  if (value == null) return <p className="text-xs text-zinc-500">模型沒有給信心分數</p>;
  const level = CONFIDENCE_LEVELS.find(([min]) => value >= min)?.[1] ?? "低";
  return (
    <div>
      <div className="mb-1 flex items-baseline justify-between text-xs">
        <span className="font-medium text-zinc-600 dark:text-zinc-300">模型自評信心</span>
        <span className="tabular-nums text-zinc-700 dark:text-zinc-200">
          <span className="text-base font-semibold">{percent(value)}</span> · {level}
        </span>
      </div>
      <div className="relative h-2 rounded-full bg-ai-100 dark:bg-ai-950" role="meter" aria-valuenow={Math.round(value * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="模型自評信心">
        <div className="h-full rounded-full bg-ai-500" style={{ width: percent(value) }} />
        {threshold != null && (
          <div
            className="absolute -top-1 h-4 w-0.5 rounded bg-zinc-900 dark:bg-zinc-100"
            style={{ left: `calc(${percent(threshold)} - 1px)` }}
            title={`自動接受門檻 ${percent(threshold)}`}
          />
        )}
      </div>
      {threshold != null && (
        <p className="mt-1 text-[11px] text-zinc-500 dark:text-zinc-400">黑線是自動接受的門檻（{percent(threshold)}）</p>
      )}
    </div>
  );
}

type Props = {
  project: Project;
  ai: AiStatus | undefined;
  suggestion: Suggestion | null;
  values: Values;
  busy: boolean;
  onGenerate: () => void;
  onFill: (s: Suggestion) => void;
  onAccept: (s: Suggestion) => void;
  onReject: (s: Suggestion) => void;
};

export function AiPanel({ project, ai, suggestion, values, busy, onGenerate, onFill, onAccept, onReject }: Props) {
  const fields = project.schema.fields;
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <span className="grid size-7 place-items-center rounded-lg bg-ai-100 text-ai-700 dark:bg-ai-950 dark:text-ai-400">
          <Sparkles className="size-4" aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">AI 建議</h2>
          <p className="truncate text-xs text-zinc-500 dark:text-zinc-400">
            {ai?.enabled ? `${ai.provider === "anthropic" ? "Claude API" : "OpenAI 相容"} · ${ai.model}` : "未開啟"}
          </p>
        </div>
      </div>

      {!ai?.enabled ? (
        <div className="rounded-xl border border-dashed border-zinc-300 p-4 text-sm text-zinc-500 dark:border-zinc-700 dark:text-zinc-400">
          <p>設定環境變數後，模型會先給分數、類別和理由，你按接受或修改就好。</p>
          <pre className="mt-3 overflow-x-auto rounded-lg bg-zinc-100 p-2.5 text-[11px] leading-relaxed text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300">
            {"LLM_PROVIDER=anthropic\nANTHROPIC_API_KEY=…\n\n# 或 Ollama 等 OpenAI 相容服務\nLLM_PROVIDER=openai\nOPENAI_BASE_URL=http://ollama:11434/v1\nOPENAI_MODEL=qwen3-vl:2b-instruct"}
          </pre>
          {ai && !ai.enabled && ai.reason && <p className="mt-2 text-xs">目前：{ai.reason}</p>}
        </div>
      ) : !suggestion ? (
        <div className="rounded-xl border border-dashed border-ai-200 bg-ai-50/40 p-4 text-center dark:border-ai-900 dark:bg-ai-950/20">
          <p className="mb-3 text-sm text-zinc-600 dark:text-zinc-300">這筆還沒有建議。</p>
          <button className="btn btn-secondary" onClick={onGenerate} disabled={busy}>
            {busy ? <Spinner /> : <Wand2 className="size-4" />}
            產生建議 <Kbd>G</Kbd>
          </button>
        </div>
      ) : (
        <div className="space-y-4 rounded-2xl border border-ai-200 bg-gradient-to-b from-ai-50/70 to-white p-4 dark:border-ai-900/70 dark:from-ai-950/40 dark:to-zinc-900">
          <ConfidenceMeter value={suggestion.confidence} threshold={project.ai.autoAcceptThreshold} />
          {suggestion.rationale && (
            <blockquote className="border-l-2 border-ai-400 pl-3 text-sm leading-relaxed text-zinc-700 dark:text-zinc-200">
              {suggestion.rationale}
            </blockquote>
          )}
          <dl className="space-y-1.5 text-sm">
            {fields.map((f) => {
              const v = suggestion.values[f.id];
              const same = valuesEqual({ x: values[f.id] ?? "" }, { x: v ?? "" });
              return (
                <div key={f.id} className="flex items-baseline justify-between gap-3">
                  <dt className="shrink-0 text-zinc-500 dark:text-zinc-400">{f.label}</dt>
                  <dd className={`truncate text-right font-medium ${same ? "text-zinc-800 dark:text-zinc-100" : "text-ai-700 dark:text-ai-400"}`} title={same ? "和表單一樣" : "和目前的表單不同"}>
                    {formatValue(f, v)}
                  </dd>
                </div>
              );
            })}
          </dl>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            {STATUS_TEXT[suggestion.status]} · {suggestion.model}
          </p>
          <div className="grid grid-cols-2 gap-2">
            <button className="btn btn-primary col-span-2" onClick={() => onAccept(suggestion)} disabled={busy}>
              <Check className="size-4" />
              接受並下一筆 <Kbd>A</Kbd>
            </button>
            <button className="btn btn-secondary" onClick={() => onFill(suggestion)} disabled={busy}>
              <CornerDownLeft className="size-4" />
              帶入修改 <Kbd>F</Kbd>
            </button>
            <button className="btn btn-ghost" onClick={() => onReject(suggestion)} disabled={busy || suggestion.status === "rejected"}>
              <ThumbsDown className="size-4" />
              忽略
            </button>
          </div>
          <button className="btn btn-ghost w-full text-xs" onClick={onGenerate} disabled={busy}>
            {busy ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />}
            重新產生 <Kbd>G</Kbd>
          </button>
        </div>
      )}
    </div>
  );
}

export const SHORTCUTS: [string, string][] = [
  ["1–9", "選第 n 個選項；在分數欄位直接設分數"],
  ["↑ ↓", "切換欄位"],
  ["← →", "調整分數，或換單選的選項"],
  ["I", "在標籤或文字欄位輸入"],
  ["Enter", "送出並下一筆"],
  ["S", "跳過這筆"],
  ["B", "回到上一筆"],
  ["G", "產生 AI 建議"],
  ["A", "接受 AI 建議並下一筆"],
  ["F", "把 AI 建議帶入表單"],
  ["?", "顯示這張表"],
];

export function ShortcutList() {
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs">
      {SHORTCUTS.map(([k, d]) => (
        <div key={k} className="contents">
          <dt>
            <Kbd>{k}</Kbd>
          </dt>
          <dd className="text-zinc-500 dark:text-zinc-400">{d}</dd>
        </div>
      ))}
    </dl>
  );
}
