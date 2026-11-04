import { ArrowLeft, CircleCheckBig, Keyboard, SkipForward, Send, UserRound } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api } from "../api";
import { AiPanel, ShortcutList } from "../components/AiPanel";
import { AnnotatorPicker } from "../components/AnnotatorPicker";
import { FieldInput } from "../components/FieldInput";
import { ItemView } from "../components/ItemView";
import { EmptyState, Kbd, Modal, ProgressBar, Spinner } from "../components/ui";
import { applyArrow, applyDigit, compact, initialValues, missingRequired } from "../lib/form";
import { isTypingTarget } from "../lib/hooks";
import { percent } from "../lib/format";
import { useErrorToast, useSession } from "../lib/session";
import type { ItemDetail, Stats, Suggestion, Values } from "../types";
import { useProject } from "./ProjectLayout";

/** 跳過的資料最多記幾筆（抽下一筆時盡量避開）。 */
const MAX_SKIPPED = 50;

export function LabelPage() {
  const { project } = useProject();
  const { annotator, meta, toast } = useSession();
  const showError = useErrorToast();
  const [params, setParams] = useSearchParams();
  const requested = params.get("item") ? Number(params.get("item")) : null;

  const [detail, setDetail] = useState<ItemDetail | null>(null);
  const [values, setValues] = useState<Values>({});
  const [active, setActive] = useState(0);
  const [filledFrom, setFilledFrom] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [stats, setStats] = useState<Stats | null>(null);
  const [help, setHelp] = useState(false);
  const history = useRef<number[]>([]);
  const skipped = useRef<number[]>([]);
  const inputs = useRef<(HTMLInputElement | HTMLTextAreaElement | null)[]>([]);
  const fields = project.schema.fields;

  const refreshStats = useCallback(() => {
    api.stats(project.id, annotator?.id).then(setStats).catch(() => {});
  }, [project.id, annotator?.id]);

  const show = useCallback(
    async (itemId: number) => {
      setLoading(true);
      try {
        const d = await api.item(itemId, annotator?.id);
        setDetail(d);
        setValues(initialValues(project.schema, d.mine?.values));
        setFilledFrom(d.mine?.suggestionId ?? null);
        setActive(0);
        setDone(false);
      } catch (e) {
        showError(e);
      } finally {
        setLoading(false);
      }
    },
    [annotator?.id, project.schema, showError],
  );

  const loadNext = useCallback(async () => {
    if (!annotator) return;
    setLoading(true);
    try {
      const { itemId } = await api.next(project.id, annotator.id, skipped.current);
      if (itemId === null) {
        setDetail(null);
        setDone(true);
        setLoading(false);
      } else {
        await show(itemId);
      }
    } catch (e) {
      showError(e);
      setLoading(false);
    }
  }, [annotator, project.id, show, showError]);

  useEffect(() => {
    if (!annotator) return;
    refreshStats();
    if (requested) show(requested);
    else loadNext();
    // 只在換人、換專案或指定資料時重新開始（show／loadNext 會跟著這些值變）
  }, [annotator?.id, project.id, requested]);

  const goNext = useCallback(() => {
    if (detail) history.current.push(detail.item.id);
    if (requested) setParams({}, { replace: true });
    else loadNext();
  }, [detail, requested, setParams, loadNext]);

  const save = useCallback(
    async (vals: Values, suggestionId: number | null) => {
      if (!detail || !annotator) return;
      const missing = missingRequired(project.schema, vals);
      if (missing.length) {
        toast("error", `還沒填：${missing.map((f) => f.label).join("、")}`);
        setActive(fields.findIndex((f) => f.id === missing[0].id));
        return;
      }
      setSaving(true);
      try {
        await api.saveAnnotation(detail.item.id, { annotatorId: annotator.id, values: compact(vals), suggestionId });
        skipped.current = skipped.current.filter((id) => id !== detail.item.id);
        refreshStats();
        goNext();
      } catch (e) {
        showError(e);
      } finally {
        setSaving(false);
      }
    },
    [detail, annotator, project.schema, fields, toast, refreshStats, goNext, showError],
  );

  const submit = useCallback(() => save(values, filledFrom), [save, values, filledFrom]);

  const skip = useCallback(() => {
    if (!detail) return;
    skipped.current = [...skipped.current.filter((id) => id !== detail.item.id), detail.item.id].slice(-MAX_SKIPPED);
    goNext();
  }, [detail, goNext]);

  const back = useCallback(() => {
    const prev = history.current.pop();
    if (prev !== undefined) show(prev);
  }, [show]);

  const generate = useCallback(async () => {
    if (!detail || !meta?.ai.enabled) return;
    setAiBusy(true);
    try {
      const s = await api.suggest(detail.item.id);
      setDetail((d) => (d && d.item.id === s.itemId ? { ...d, suggestion: s } : d));
    } catch (e) {
      showError(e);
    } finally {
      setAiBusy(false);
    }
  }, [detail, meta?.ai.enabled, showError]);

  const fill = useCallback((s: Suggestion) => {
    setValues((v) => ({ ...v, ...s.values }));
    setFilledFrom(s.id);
  }, []);

  const accept = useCallback((s: Suggestion) => save({ ...values, ...s.values }, s.id), [save, values]);

  const reject = useCallback(
    async (s: Suggestion) => {
      try {
        const r = await api.rejectSuggestion(s.id);
        setDetail((d) => (d ? { ...d, suggestion: r } : d));
        if (filledFrom === s.id) setFilledFrom(null);
      } catch (e) {
        showError(e);
      }
    },
    [filledFrom, showError],
  );

  // 鍵盤快捷鍵
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.ctrlKey && e.key === "Enter") {
        e.preventDefault();
        submit();
        return;
      }
      if (isTypingTarget(e) || e.altKey || e.metaKey || e.ctrlKey || !detail || saving) return;
      const field = fields[active];
      const key = e.key;
      let handled = true;
      if (/^[1-9]$/.test(key) && field) {
        const next = applyDigit(field, values, Number(key));
        if (next) setValues(next);
      } else if (key === "ArrowDown") {
        setActive((a) => Math.min(fields.length - 1, a + 1));
      } else if (key === "ArrowUp") {
        setActive((a) => Math.max(0, a - 1));
      } else if ((key === "ArrowLeft" || key === "ArrowRight") && field) {
        const next = applyArrow(field, values, key === "ArrowLeft" ? -1 : 1);
        if (next) setValues(next);
        else handled = false;
      } else if (key === "Enter") {
        submit();
      } else if (key === "s" || key === "S") {
        skip();
      } else if (key === "b" || key === "B") {
        back();
      } else if (key === "g" || key === "G") {
        generate();
      } else if ((key === "a" || key === "A") && detail.suggestion) {
        accept(detail.suggestion);
      } else if ((key === "f" || key === "F") && detail.suggestion) {
        fill(detail.suggestion);
      } else if ((key === "i" || key === "I") && field && (field.type === "tags" || field.type === "text")) {
        inputs.current[active]?.focus();
      } else if (key === "?") {
        setHelp(true);
      } else {
        handled = false;
      }
      if (handled) e.preventDefault();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, fields, values, detail, saving, submit, skip, back, generate, accept, fill]);

  // 目前的欄位捲進畫面
  useEffect(() => {
    document.querySelector(`[data-field-index="${active}"]`)?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
  }, [active]);

  if (!annotator) {
    return (
      <div className="mx-auto max-w-lg px-4 py-16">
        <div className="card p-6">
          <div className="mb-4 flex items-center gap-2">
            <UserRound className="size-5 text-brand-600" />
            <h1 className="text-base font-semibold">先選一位標注者</h1>
          </div>
          <AnnotatorPicker />
        </div>
      </div>
    );
  }

  const visible = stats?.items.visible ?? project.counts?.visible ?? 0;
  const mine = stats?.mine ?? 0;

  return (
    <div className="grid min-h-dvh xl:grid-cols-[minmax(0,1fr)_23rem]">
      <div className="min-w-0 px-4 py-6 sm:px-8">
        <header className="mb-5 flex flex-wrap items-center gap-x-6 gap-y-3">
          <div className="min-w-0 basis-full sm:basis-auto sm:flex-1">
            <p className="text-xs font-medium text-brand-700 dark:text-brand-400">標注</p>
            <h1 className="truncate text-xl font-semibold tracking-tight">{project.name}</h1>
          </div>
          <div className="w-full max-w-xs space-y-1.5 sm:w-64">
            <div className="flex justify-between text-xs text-zinc-500 dark:text-zinc-400">
              <span>
                我標了 <b className="font-semibold text-zinc-800 dark:text-zinc-100">{mine}</b> / {visible}
              </span>
              <span>整體覆蓋 {percent(stats?.items.coverRate ?? 0)}</span>
            </div>
            <ProgressBar value={visible ? mine / visible : 0} label="我的進度" />
          </div>
          <button className="btn btn-ghost" onClick={() => setHelp(true)}>
            <Keyboard className="size-4" />
            快捷鍵 <Kbd>?</Kbd>
          </button>
        </header>

        {loading && !detail ? (
          <div className="grid h-80 place-items-center text-zinc-400">
            <Spinner className="size-6" />
          </div>
        ) : done || !detail ? (
          <div className="card">
            <EmptyState icon={<CircleCheckBig className="size-6" />} title={visible ? "這個專案你都標完了" : "這個專案還沒有資料"}>
              {visible ? (
                <>可以到「資料」頁檢查或修改標注，或到「進度與一致性」看大家標得一不一致。</>
              ) : (
                <>
                  先到 <Link className="font-medium text-brand-700 underline dark:text-brand-400" to={`/p/${project.id}/data`}>匯入匯出</Link> 上傳 CSV、JSONL 或圖片 zip。
                </>
              )}
            </EmptyState>
          </div>
        ) : (
          <div className={`space-y-4 transition-opacity ${loading ? "opacity-50" : ""}`}>
            <ItemView item={detail.item} />
            {detail.mine && (
              <p className="text-xs text-zinc-500 dark:text-zinc-400">你之前標過這筆，送出會更新你的標注。</p>
            )}
            <div className="space-y-3">
              {fields.map((f, i) => (
                <div key={f.id} data-field-index={i}>
                  <FieldInput
                    ref={(el) => {
                      inputs.current[i] = el;
                    }}
                    field={f}
                    value={values[f.id]}
                    active={i === active}
                    onFocusField={() => setActive(i)}
                    hints={detail.tagHints[f.id]}
                    suggested={detail.suggestion?.status !== "rejected" ? detail.suggestion?.values[f.id] : undefined}
                    onChange={(v) =>
                      setValues((cur) => {
                        const next = { ...cur };
                        if (v === undefined) delete next[f.id];
                        else next[f.id] = v;
                        return next;
                      })
                    }
                  />
                </div>
              ))}
            </div>
            <div className="sticky bottom-0 -mx-4 flex flex-wrap items-center gap-2 border-t border-zinc-200/80 bg-zinc-50/90 px-4 py-3 backdrop-blur sm:-mx-8 sm:px-8 dark:border-zinc-800 dark:bg-zinc-950/90">
              <button className="btn btn-ghost" onClick={back} disabled={!history.current.length || saving}>
                <ArrowLeft className="size-4" />
                上一筆 <Kbd>B</Kbd>
              </button>
              <button className="btn btn-secondary" onClick={skip} disabled={saving}>
                <SkipForward className="size-4" />
                跳過 <Kbd>S</Kbd>
              </button>
              <div className="flex-1" />
              {filledFrom && <span className="text-xs text-ai-700 dark:text-ai-400">已帶入 AI 建議</span>}
              <button className="btn btn-primary px-5" onClick={submit} disabled={saving}>
                {saving ? <Spinner /> : <Send className="size-4" />}
                送出 <kbd className="kbd border-white/30 bg-white/15 text-white shadow-none dark:text-zinc-900">Enter</kbd>
              </button>
            </div>
          </div>
        )}
      </div>

      <aside className="border-t border-zinc-200/80 bg-white/60 px-4 py-6 xl:sticky xl:top-0 xl:h-dvh xl:overflow-y-auto xl:border-t-0 xl:border-l dark:border-zinc-800 dark:bg-zinc-900/40">
        {detail ? (
          <AiPanel
            project={project}
            ai={meta?.ai}
            suggestion={detail.suggestion}
            values={values}
            busy={aiBusy || saving}
            onGenerate={generate}
            onFill={fill}
            onAccept={accept}
            onReject={reject}
          />
        ) : (
          <p className="text-sm text-zinc-400">AI 建議會顯示在這裡。</p>
        )}
        <div className="mt-6 border-t border-zinc-200/80 pt-5 dark:border-zinc-800">
          <p className="section-title mb-3">快捷鍵</p>
          <ShortcutList />
        </div>
      </aside>

      <Modal open={help} onClose={() => setHelp(false)} title="鍵盤快捷鍵">
        <ShortcutList />
      </Modal>
    </div>
  );
}
