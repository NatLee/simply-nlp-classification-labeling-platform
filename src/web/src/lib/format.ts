// 數字和時間的顯示方式。

export const percent = (x: number, digits = 0) => `${(x * 100).toFixed(digits)}%`;

export const fixed = (x: number | null | undefined, digits = 2) => (x == null ? "—" : x.toFixed(digits));

/**
 * kappa／alpha 的白話解讀。分級依 Landis & Koch (1977)，是最常被引用的一組門檻；
 * 只是慣例，不是統計檢定。
 */
export const AGREEMENT_LEVELS: { min: number; label: string; tone: "bad" | "weak" | "ok" | "good" }[] = [
  { min: 0.81, label: "幾乎完全一致", tone: "good" },
  { min: 0.61, label: "高度一致", tone: "good" },
  { min: 0.41, label: "中度一致", tone: "ok" },
  { min: 0.21, label: "尚可", tone: "weak" },
  { min: 0.0, label: "輕微", tone: "weak" },
  { min: -Infinity, label: "比碰巧還差", tone: "bad" },
];

export function agreementLevel(x: number | null | undefined) {
  if (x == null) return null;
  return AGREEMENT_LEVELS.find((l) => x >= l.min) ?? null;
}

export function dateTime(unix: number) {
  return new Date(unix * 1000).toLocaleString("zh-TW", { hour12: false });
}

export function compactNumber(n: number) {
  return new Intl.NumberFormat("zh-TW", { notation: n >= 10_000 ? "compact" : "standard" }).format(n);
}
