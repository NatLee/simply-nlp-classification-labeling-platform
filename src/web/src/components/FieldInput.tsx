import { Plus, X } from "lucide-react";
import { forwardRef, useState } from "react";
import { digitHint } from "../lib/form";
import type { Field, Value } from "../types";
import { Kbd } from "./ui";

type Props = {
  field: Field;
  value: Value | undefined;
  onChange: (v: Value | undefined) => void;
  active: boolean;
  onFocusField: () => void;
  /** 自由標籤欄位的提示（這筆資料以前用過的、整個專案常用的） */
  hints?: string[];
  /** AI 建議的值，用來在選項上加一個小記號 */
  suggested?: Value;
};

function OptionButton({
  selected,
  suggested,
  onClick,
  emoji,
  label,
  hotkey,
  multi,
}: {
  selected: boolean;
  suggested: boolean;
  onClick: () => void;
  emoji?: string;
  label: string;
  hotkey: string | null;
  multi?: boolean;
}) {
  return (
    <button
      type="button"
      role={multi ? "checkbox" : "radio"}
      aria-checked={selected}
      onClick={onClick}
      className={`group relative flex items-center gap-2 rounded-xl border px-3 py-2 text-left text-sm transition ${
        selected
          ? "border-brand-500 bg-brand-50 text-brand-900 shadow-sm ring-1 ring-brand-500 dark:bg-brand-950/60 dark:text-brand-100"
          : "border-zinc-200 bg-white text-zinc-700 hover:border-zinc-300 hover:bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-200 dark:hover:border-zinc-700 dark:hover:bg-zinc-800/60"
      }`}
    >
      {emoji && <span className="text-base leading-none">{emoji}</span>}
      <span className="min-w-0 flex-1 truncate font-medium">{label}</span>
      {suggested && (
        <span className="size-1.5 rounded-full bg-ai-500" title="AI 建議" aria-label="AI 建議" />
      )}
      {hotkey && <Kbd>{hotkey}</Kbd>}
    </button>
  );
}

export const FieldInput = forwardRef<HTMLInputElement | HTMLTextAreaElement, Props>(function FieldInput(
  { field, value, onChange, active, onFocusField, hints = [], suggested },
  ref,
) {
  const [draft, setDraft] = useState("");
  const header = (
    <div className="mb-3 flex items-baseline justify-between gap-3">
      <div className="min-w-0">
        <h3 className="text-sm font-semibold text-zinc-800 dark:text-zinc-100">
          {field.label}
          {field.required && <span className="ml-0.5 text-brand-600">*</span>}
        </h3>
        {field.description && <p className="text-xs text-zinc-500 dark:text-zinc-400">{field.description}</p>}
      </div>
      {field.type === "scale" && (
        <span className="text-2xl font-semibold tabular-nums text-brand-700 dark:text-brand-300">
          {typeof value === "number" ? value : "—"}
        </span>
      )}
    </div>
  );

  let body: React.ReactNode = null;
  switch (field.type) {
    case "scale": {
      const v = typeof value === "number" ? value : (field.default ?? field.min);
      const steps = Math.round((field.max - field.min) / field.step);
      const ticks = steps <= 10 ? Array.from({ length: steps + 1 }, (_, i) => +(field.min + i * field.step).toFixed(6)) : null;
      body = (
        <div>
          <input
            type="range"
            className="slider"
            min={field.min}
            max={field.max}
            step={field.step}
            value={v}
            aria-label={field.label}
            style={{ ["--fill" as string]: `${((v - field.min) / (field.max - field.min)) * 100}%` }}
            onChange={(e) => onChange(Number(e.target.value))}
            onFocus={onFocusField}
          />
          {ticks && (
            <div className="mt-1.5 flex justify-between px-0.5 text-xs tabular-nums text-zinc-400">
              {ticks.map((t) => (
                <button
                  type="button"
                  key={t}
                  onClick={() => onChange(t)}
                  className={`w-6 text-center hover:text-brand-600 ${t === value ? "font-semibold text-brand-700 dark:text-brand-300" : ""} ${t === suggested ? "underline decoration-ai-500 decoration-2 underline-offset-4" : ""}`}
                >
                  {t}
                </button>
              ))}
            </div>
          )}
          {(field.minLabel || field.maxLabel) && (
            <div className="mt-1 flex justify-between text-xs text-zinc-500 dark:text-zinc-400">
              <span>{field.minLabel}</span>
              <span>{field.maxLabel}</span>
            </div>
          )}
        </div>
      );
      break;
    }
    case "single":
    case "multi": {
      const multi = field.type === "multi";
      const selected = multi ? ((value as string[] | undefined) ?? []) : [value as string | undefined];
      const sugg = multi ? ((suggested as string[] | undefined) ?? []) : [suggested as string | undefined];
      body = (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-2" role={multi ? "group" : "radiogroup"} aria-label={field.label}>
          {field.options.map((o, i) => (
            <OptionButton
              key={o.value}
              multi={multi}
              selected={selected.includes(o.value)}
              suggested={sugg.includes(o.value)}
              emoji={o.emoji}
              label={o.label || o.value}
              hotkey={active ? digitHint(field, i) : null}
              onClick={() => {
                onFocusField();
                if (multi) {
                  const cur = selected as string[];
                  onChange(cur.includes(o.value) ? cur.filter((x) => x !== o.value) : [...cur, o.value]);
                } else {
                  onChange(o.value);
                }
              }}
            />
          ))}
        </div>
      );
      break;
    }
    case "tags": {
      const tags = (value as string[] | undefined) ?? [];
      const add = (t: string) => {
        const s = t.trim();
        if (s && !tags.includes(s)) onChange([...tags, s]);
      };
      const remaining = hints.filter((h) => !tags.includes(h));
      body = (
        <div className="space-y-2.5">
          <div className="flex flex-wrap items-center gap-1.5 rounded-xl border border-zinc-200 bg-white px-2 py-1.5 focus-within:border-brand-500 focus-within:ring-2 focus-within:ring-brand-500/25 dark:border-zinc-800 dark:bg-zinc-950">
            {tags.map((t) => (
              <span key={t} className="chip border-brand-200 bg-brand-50 text-brand-800 dark:border-brand-900 dark:bg-brand-950 dark:text-brand-200">
                {t}
                <button type="button" onClick={() => onChange(tags.filter((x) => x !== t))} aria-label={`移除 ${t}`}>
                  <X className="size-3" />
                </button>
              </span>
            ))}
            <input
              ref={ref as React.Ref<HTMLInputElement>}
              className="min-w-[8rem] flex-1 bg-transparent px-1 py-0.5 text-sm outline-none placeholder:text-zinc-400"
              placeholder={tags.length ? "" : "輸入標籤，按 Enter 新增"}
              value={draft}
              aria-label={`${field.label}：新增標籤`}
              onFocus={onFocusField}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if ((e.key === "Enter" || e.key === ",") && draft.trim()) {
                  e.preventDefault();
                  e.stopPropagation();
                  add(draft);
                  setDraft("");
                } else if (e.key === "Backspace" && !draft && tags.length) {
                  onChange(tags.slice(0, -1));
                } else if (e.key === "Escape") {
                  (e.target as HTMLInputElement).blur();
                }
              }}
            />
          </div>
          {remaining.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {remaining.slice(0, 12).map((h) => (
                <button
                  type="button"
                  key={h}
                  onClick={() => add(h)}
                  className="chip border-dashed border-zinc-300 text-zinc-500 hover:border-brand-400 hover:text-brand-700 dark:border-zinc-700 dark:text-zinc-400"
                >
                  <Plus className="size-3" />
                  {h}
                </button>
              ))}
            </div>
          )}
        </div>
      );
      break;
    }
    case "text": {
      const s = (value as string | undefined) ?? "";
      const max = field.maxLength ?? 2000;
      body = (
        <div>
          <textarea
            ref={ref as React.Ref<HTMLTextAreaElement>}
            className="input min-h-20 resize-y"
            value={s}
            maxLength={max}
            aria-label={field.label}
            placeholder="選填"
            onFocus={onFocusField}
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && (e.target as HTMLTextAreaElement).blur()}
          />
          <p className="hint text-right tabular-nums">
            {s.length} / {max}
          </p>
        </div>
      );
      break;
    }
  }

  return (
    <section
      onClick={onFocusField}
      data-active={active}
      className={`rounded-2xl border p-4 transition ${
        active
          ? "border-brand-300 bg-white shadow-soft ring-4 ring-brand-500/10 dark:border-brand-800 dark:bg-zinc-900"
          : "border-zinc-200/80 bg-white/70 dark:border-zinc-800 dark:bg-zinc-900/50"
      }`}
    >
      {header}
      {body}
    </section>
  );
});
