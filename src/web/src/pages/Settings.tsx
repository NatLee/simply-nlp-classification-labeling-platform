import { ArrowDown, ArrowUp, Braces, Plus, Save, Sparkles, Trash2, X } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router";
import { api } from "../api";
import { PageHeader, Spinner } from "../components/ui";
import { FIELD_TYPE_LABELS, newField } from "../lib/form";
import { percent } from "../lib/format";
import { useErrorToast, useSession } from "../lib/session";
import type { AiSettings, Choice, Field, FieldType, Schema } from "../types";
import { useProject } from "./ProjectLayout";

const TYPES: FieldType[] = ["scale", "single", "multi", "tags", "text"];
/** 打開「自動接受」時先帶入的門檻。 */
const DEFAULT_AUTO_ACCEPT = 0.9;
/** 門檻滑桿的範圍：低於一半信心的建議不適合自動接受。 */
const THRESHOLD_MIN = 0.5;

function NumberInput({ label, value, onChange, step = "any" }: { label: string; value: number | null | undefined; onChange: (v: number | null) => void; step?: string }) {
  return (
    <div>
      <label className="label text-xs">{label}</label>
      <input
        type="number"
        step={step}
        className="input"
        value={value ?? ""}
        aria-label={label}
        onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
      />
    </div>
  );
}

function OptionsEditor({ options, onChange }: { options: Choice[]; onChange: (o: Choice[]) => void }) {
  const set = (i: number, patch: Partial<Choice>) => onChange(options.map((o, j) => (j === i ? { ...o, ...patch } : o)));
  return (
    <div className="space-y-2">
      <div className="grid grid-cols-[3rem_1fr_1fr_auto] gap-2 text-xs font-medium text-zinc-500">
        <span>圖示</span>
        <span>值（匯出用）</span>
        <span>顯示名稱</span>
        <span className="w-8" />
      </div>
      {options.map((o, i) => (
        <div key={i} className="grid grid-cols-[3rem_1fr_1fr_auto] items-center gap-2">
          <input className="input px-2 text-center" value={o.emoji ?? ""} onChange={(e) => set(i, { emoji: e.target.value })} aria-label={`選項 ${i + 1} 圖示`} maxLength={4} />
          <input className="input font-mono text-xs" value={o.value} onChange={(e) => set(i, { value: e.target.value })} aria-label={`選項 ${i + 1} 的值`} />
          <input className="input" value={o.label ?? ""} onChange={(e) => set(i, { label: e.target.value })} aria-label={`選項 ${i + 1} 的顯示名稱`} />
          <button type="button" className="btn btn-ghost w-8 px-0" onClick={() => onChange(options.filter((_, j) => j !== i))} aria-label={`刪除選項 ${i + 1}`}>
            <X className="size-4" />
          </button>
        </div>
      ))}
      <button
        type="button"
        className="btn btn-ghost text-xs"
        onClick={() => {
          let n = options.length + 1;
          while (options.some((o) => o.value === `option_${n}`)) n++;
          onChange([...options, { value: `option_${n}`, label: `選項 ${n}` }]);
        }}
      >
        <Plus className="size-3.5" />
        新增選項
      </button>
    </div>
  );
}

function FieldEditor({
  field,
  index,
  count,
  onChange,
  onMove,
  onRemove,
}: {
  field: Field;
  index: number;
  count: number;
  onChange: (f: Field) => void;
  onMove: (d: -1 | 1) => void;
  onRemove: () => void;
}) {
  const set = (patch: Partial<Field>) => onChange({ ...field, ...patch } as Field);
  return (
    <div className="rounded-2xl border border-zinc-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-900">
      <div className="mb-3 flex items-center gap-2">
        <span className="chip border-brand-200 bg-brand-50 text-brand-700 dark:border-brand-900 dark:bg-brand-950 dark:text-brand-300">{FIELD_TYPE_LABELS[field.type]}</span>
        <span className="min-w-0 flex-1 truncate text-sm font-semibold">{field.label || field.id}</span>
        <button className="btn btn-ghost px-1.5" onClick={() => onMove(-1)} disabled={index === 0} aria-label="往上移">
          <ArrowUp className="size-4" />
        </button>
        <button className="btn btn-ghost px-1.5" onClick={() => onMove(1)} disabled={index === count - 1} aria-label="往下移">
          <ArrowDown className="size-4" />
        </button>
        <button className="btn btn-ghost px-1.5 text-red-600" onClick={onRemove} aria-label={`刪除欄位 ${field.label}`}>
          <Trash2 className="size-4" />
        </button>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <div>
          <label className="label text-xs">顯示名稱</label>
          <input className="input" value={field.label} onChange={(e) => set({ label: e.target.value })} aria-label="顯示名稱" />
        </div>
        <div>
          <label className="label text-xs">id（匯出的欄名，小寫英數字和底線）</label>
          <input className="input font-mono text-xs" value={field.id} onChange={(e) => set({ id: e.target.value })} aria-label="欄位 id" />
        </div>
        <div className="sm:col-span-2">
          <label className="label text-xs">說明（標注者和模型都看得到）</label>
          <input className="input" value={field.description ?? ""} onChange={(e) => set({ description: e.target.value })} aria-label="說明" />
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" className="accent-brand-600" checked={!!field.required} onChange={(e) => set({ required: e.target.checked })} />
          必填
        </label>
      </div>
      <div className="mt-4 border-t border-zinc-100 pt-4 dark:border-zinc-800">
        {field.type === "scale" && (
          <div className="grid gap-3 sm:grid-cols-4">
            <NumberInput label="最小值" value={field.min} onChange={(v) => set({ min: v ?? 0 })} />
            <NumberInput label="最大值" value={field.max} onChange={(v) => set({ max: v ?? 1 })} />
            <NumberInput label="間隔" value={field.step} onChange={(v) => set({ step: v ?? 1 })} />
            <NumberInput label="預設值" value={field.default} onChange={(v) => set({ default: v })} />
            <div className="sm:col-span-2">
              <label className="label text-xs">最小值的意思</label>
              <input className="input" value={field.minLabel ?? ""} onChange={(e) => set({ minLabel: e.target.value })} placeholder="例如：很負面" aria-label="最小值的意思" />
            </div>
            <div className="sm:col-span-2">
              <label className="label text-xs">最大值的意思</label>
              <input className="input" value={field.maxLabel ?? ""} onChange={(e) => set({ maxLabel: e.target.value })} placeholder="例如：很正面" aria-label="最大值的意思" />
            </div>
          </div>
        )}
        {field.type === "single" && (
          <div className="space-y-3">
            <OptionsEditor options={field.options} onChange={(options) => set({ options })} />
            <div className="w-56">
              <label className="label text-xs">預設選項</label>
              <select className="input" value={field.default ?? ""} onChange={(e) => set({ default: e.target.value || null })} aria-label="預設選項">
                <option value="">（不預選）</option>
                {field.options.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label || o.value}
                  </option>
                ))}
              </select>
            </div>
          </div>
        )}
        {field.type === "multi" && (
          <div className="space-y-3">
            <OptionsEditor options={field.options} onChange={(options) => set({ options })} />
            <div className="w-40">
              <NumberInput label="最多選幾個（空白＝不限）" value={field.maxSelected} onChange={(v) => set({ maxSelected: v })} step="1" />
            </div>
          </div>
        )}
        {field.type === "tags" && (
          <div>
            <label className="label text-xs">建議的標籤（用逗號分隔）</label>
            <input
              className="input"
              value={(field.suggestions ?? []).join(", ")}
              onChange={(e) => set({ suggestions: e.target.value.split(/[,，]/).map((s) => s.trim()).filter(Boolean) })}
              aria-label="建議的標籤"
            />
            <p className="hint">標注時也會列出這筆資料以前用過、和整個專案常用的標籤。</p>
          </div>
        )}
        {field.type === "text" && (
          <div className="w-40">
            <NumberInput label="字數上限" value={field.maxLength} onChange={(v) => set({ maxLength: v ?? 2000 })} step="1" />
          </div>
        )}
      </div>
    </div>
  );
}

export function SettingsPage() {
  const { project, setProject } = useProject();
  const { toast } = useSession();
  const showError = useErrorToast();
  const navigate = useNavigate();
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description);
  const [schema, setSchema] = useState<Schema>(project.schema);
  const [ai, setAi] = useState<AiSettings>(project.ai);
  const [jsonMode, setJsonMode] = useState(false);
  const [json, setJson] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmName, setConfirmName] = useState("");

  const fields = schema.fields;
  const setFields = (f: Field[]) => setSchema({ fields: f });

  async function save() {
    let next = schema;
    if (jsonMode) {
      try {
        next = JSON.parse(json);
      } catch (e) {
        toast("error", `JSON 格式錯誤：${(e as Error).message}`);
        return;
      }
    }
    setBusy(true);
    try {
      const p = await api.updateProject(project.id, { name, description, schema: next, ai });
      setProject(p);
      setSchema(p.schema);
      toast("ok", "已儲存");
    } catch (e) {
      showError(e);
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    try {
      await api.deleteProject(project.id);
      toast("ok", `已刪除「${project.name}」`);
      navigate("/");
    } catch (e) {
      showError(e);
    }
  }

  return (
    <div className="mx-auto max-w-4xl px-4 py-8 sm:px-8">
      <PageHeader
        title="專案設定"
        subtitle="改標注結構時，已經有的標注要仍然合法：可以新增欄位和選項、改名稱；拿掉用過的選項會被擋下。"
        actions={
          <button className="btn btn-primary" onClick={save} disabled={busy}>
            {busy ? <Spinner /> : <Save className="size-4" />}
            儲存
          </button>
        }
      />
      <div className="space-y-6">
        <section className="card grid gap-4 p-5 sm:grid-cols-2">
          <div>
            <label className="label" htmlFor="sname">
              專案名稱
            </label>
            <input id="sname" className="input" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div>
            <label className="label" htmlFor="sdesc">
              說明
            </label>
            <input id="sdesc" className="input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
        </section>

        <section className="space-y-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-base font-semibold">標注結構</h2>
            <button
              className="btn btn-ghost text-xs"
              onClick={() => {
                if (!jsonMode) setJson(JSON.stringify(schema, null, 2));
                else {
                  try {
                    setSchema(JSON.parse(json));
                  } catch {
                    toast("error", "JSON 格式錯誤，先改好再切回表單");
                    return;
                  }
                }
                setJsonMode(!jsonMode);
              }}
            >
              <Braces className="size-3.5" />
              {jsonMode ? "用表單編輯" : "用 JSON 編輯"}
            </button>
          </div>
          {jsonMode ? (
            <textarea className="input min-h-96 font-mono text-xs" value={json} onChange={(e) => setJson(e.target.value)} aria-label="標注結構 JSON" spellCheck={false} />
          ) : (
            <>
              {fields.map((f, i) => (
                <FieldEditor
                  key={i}
                  field={f}
                  index={i}
                  count={fields.length}
                  onChange={(nf) => setFields(fields.map((x, j) => (j === i ? nf : x)))}
                  onMove={(d) => {
                    const next = [...fields];
                    [next[i], next[i + d]] = [next[i + d], next[i]];
                    setFields(next);
                  }}
                  onRemove={() => setFields(fields.filter((_, j) => j !== i))}
                />
              ))}
              <div className="flex flex-wrap items-center gap-2 rounded-2xl border border-dashed border-zinc-300 p-3 dark:border-zinc-700">
                <span className="text-sm text-zinc-500">新增欄位：</span>
                {TYPES.map((t) => (
                  <button key={t} className="btn btn-secondary text-xs" onClick={() => setFields([...fields, newField(t, fields)])}>
                    <Plus className="size-3.5" />
                    {FIELD_TYPE_LABELS[t]}
                  </button>
                ))}
              </div>
            </>
          )}
        </section>

        <section className="card space-y-4 p-5">
          <div className="flex items-center gap-2">
            <Sparkles className="size-4 text-ai-600" />
            <h2 className="text-base font-semibold">AI 輔助</h2>
          </div>
          <div>
            <label className="label" htmlFor="instructions">
              標注說明
            </label>
            <textarea
              id="instructions"
              className="input min-h-28"
              value={ai.instructions}
              onChange={(e) => setAi({ ...ai, instructions: e.target.value })}
              placeholder="例如：依照文字表達的情緒評分，1 是很負面、5 是很正面；反諷要依實際意思判斷。"
            />
            <p className="hint">模型會看到這段說明和每個欄位的選項。匯出聊天格式時也會放在 system 訊息裡。</p>
          </div>
          <div>
            <label className="flex items-center gap-2 text-sm font-medium">
              <input
                type="checkbox"
                className="accent-brand-600"
                checked={ai.autoAcceptThreshold != null}
                onChange={(e) => setAi({ ...ai, autoAcceptThreshold: e.target.checked ? DEFAULT_AUTO_ACCEPT : null })}
              />
              批次預標時，高信心的建議直接存成標注
            </label>
            {ai.autoAcceptThreshold != null && (
              <div className="mt-3 max-w-md">
                <div className="mb-1 flex justify-between text-xs text-zinc-500">
                  <span>門檻</span>
                  <span className="font-semibold tabular-nums text-zinc-800 dark:text-zinc-100">{percent(ai.autoAcceptThreshold)}</span>
                </div>
                <input
                  type="range"
                  className="slider"
                  min={THRESHOLD_MIN}
                  max={1}
                  step={0.01}
                  value={ai.autoAcceptThreshold}
                  aria-label="自動接受門檻"
                  style={{ ["--fill" as string]: `${((ai.autoAcceptThreshold - THRESHOLD_MIN) / (1 - THRESHOLD_MIN)) * 100}%` }}
                  onChange={(e) => setAi({ ...ai, autoAcceptThreshold: Number(e.target.value) })}
                />
                <p className="hint">信心是模型自己估的，不一定準。建議先抽一些自動接受的資料人工檢查，或在統計頁看 AI 和人的一致性。</p>
              </div>
            )}
          </div>
        </section>

        <section className="rounded-2xl border border-red-200 p-5 dark:border-red-900/60">
          <h2 className="text-base font-semibold text-red-700 dark:text-red-400">刪除專案</h2>
          <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">資料、標注和 AI 建議都會刪掉，沒辦法復原。要刪的話，先輸入專案名稱。</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <input className="input max-w-xs" value={confirmName} onChange={(e) => setConfirmName(e.target.value)} placeholder={project.name} aria-label="輸入專案名稱確認刪除" />
            <button className="btn btn-danger" disabled={confirmName !== project.name} onClick={remove}>
              <Trash2 className="size-4" />
              刪除這個專案
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}
