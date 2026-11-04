// 標注表單的純邏輯：初始值、鍵盤快捷鍵、比較。和畫面分開，方便測試。
import type { Field, FieldType, Schema, Value, Values } from "../types";

export const FIELD_TYPE_LABELS: Record<FieldType, string> = {
  scale: "分數滑桿",
  single: "單選",
  multi: "多選",
  tags: "自由標籤",
  text: "文字說明",
};

/** 把分數對齊到格子上，並限制在範圍內（和後端的 snap 一樣）。 */
export function snap(value: number, min: number, max: number, step: number): number {
  const steps = Math.round((value - min) / step);
  const v = Math.min(max, Math.max(min, min + steps * step));
  return Math.round(v * 1e9) / 1e9;
}

/** 表單的起始值：自己標過就用自己的；沒有就用欄位的預設值（和舊版一樣，分數預設 3、情緒預設 wow）。 */
export function initialValues(schema: Schema, existing?: Values | null): Values {
  if (existing) return { ...existing };
  const out: Values = {};
  for (const f of schema.fields) {
    if (f.type === "scale" && f.default != null) out[f.id] = f.default;
    if (f.type === "single" && f.default) out[f.id] = f.default;
  }
  return out;
}

/** 去掉空的值（空字串、空陣列），送出前用。 */
export function compact(values: Values): Values {
  const out: Values = {};
  for (const [k, v] of Object.entries(values)) {
    if (v === "" || (Array.isArray(v) && v.length === 0)) continue;
    out[k] = typeof v === "string" ? v.trim() : v;
  }
  return out;
}

export function missingRequired(schema: Schema, values: Values): Field[] {
  const v = compact(values);
  return schema.fields.filter((f) => f.required && v[f.id] === undefined);
}

function sameValue(a: Value | undefined, b: Value | undefined): boolean {
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x) => b.includes(x));
  return a === b;
}

export function valuesEqual(a: Values, b: Values): boolean {
  const ca = compact(a);
  const cb = compact(b);
  const keys = new Set([...Object.keys(ca), ...Object.keys(cb)]);
  return [...keys].every((k) => sameValue(ca[k], cb[k]));
}

/** 數字鍵 1–9：選第 n 個選項（多選是切換）；分數欄位直接設成那個分數（在範圍內、落在格子上才算）。 */
export function applyDigit(field: Field, values: Values, digit: number): Values | null {
  switch (field.type) {
    case "single": {
      const o = field.options[digit - 1];
      return o ? { ...values, [field.id]: o.value } : null;
    }
    case "multi": {
      const o = field.options[digit - 1];
      if (!o) return null;
      const current = (values[field.id] as string[] | undefined) ?? [];
      const next = current.includes(o.value) ? current.filter((x) => x !== o.value) : [...current, o.value];
      if (field.maxSelected && next.length > field.maxSelected) return null;
      return { ...values, [field.id]: next };
    }
    case "scale": {
      if (digit < field.min || digit > field.max) return null;
      if (Math.abs(snap(digit, field.min, field.max, field.step) - digit) > 1e-9) return null;
      return { ...values, [field.id]: digit };
    }
    default:
      return null;
  }
}

/** ←／→：分數加減一格；單選換到上一個或下一個選項。 */
export function applyArrow(field: Field, values: Values, delta: -1 | 1): Values | null {
  if (field.type === "scale") {
    const current = typeof values[field.id] === "number" ? (values[field.id] as number) : (field.default ?? field.min);
    return { ...values, [field.id]: snap(current + delta * field.step, field.min, field.max, field.step) };
  }
  if (field.type === "single") {
    const i = field.options.findIndex((o) => o.value === values[field.id]);
    const n = field.options.length;
    const next = i < 0 ? (delta > 0 ? 0 : n - 1) : (i + delta + n) % n;
    return { ...values, [field.id]: field.options[next].value };
  }
  return null;
}

/** 這個欄位可以用數字鍵操作。 */
export function digitHint(field: Field, index: number): string | null {
  if (field.type === "single" || field.type === "multi") return index < 9 ? String(index + 1) : null;
  return null;
}

/** 新欄位的範本。 */
export function newField(type: FieldType, existing: Field[]): Field {
  let n = existing.length + 1;
  while (existing.some((f) => f.id === `field_${n}`)) n++;
  const base = { id: `field_${n}`, label: `新的${FIELD_TYPE_LABELS[type]}`, description: "", required: false };
  switch (type) {
    case "scale":
      return { ...base, type, min: 1, max: 5, step: 1, default: 3, minLabel: "", maxLabel: "" };
    case "single":
      return { ...base, type, options: [{ value: "a", label: "選項 A" }, { value: "b", label: "選項 B" }], default: null };
    case "multi":
      return { ...base, type, options: [{ value: "a", label: "選項 A" }, { value: "b", label: "選項 B" }], maxSelected: null };
    case "tags":
      return { ...base, type, suggestions: [] };
    case "text":
      return { ...base, type, maxLength: 2000 };
  }
}

export function formatValue(field: Field | undefined, v: Value | undefined): string {
  if (v === undefined) return "—";
  if (!field) return Array.isArray(v) ? v.join("、") : String(v);
  if (field.type === "single") {
    const o = field.options.find((x) => x.value === v);
    return o ? `${o.emoji ? `${o.emoji} ` : ""}${o.label || o.value}` : String(v);
  }
  if (field.type === "multi") {
    return (v as string[]).map((x) => field.options.find((o) => o.value === x)?.label || x).join("、");
  }
  if (Array.isArray(v)) return v.join("、");
  return String(v);
}
