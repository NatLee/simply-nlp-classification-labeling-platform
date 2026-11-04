import { CircleAlert, CircleCheck, Info, LoaderCircle, X } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import { useSession } from "../lib/session";

export function Spinner({ className = "size-4" }: { className?: string }) {
  return <LoaderCircle className={`${className} animate-spin`} aria-hidden />;
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function PageHeader({ title, subtitle, actions }: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="text-xl font-semibold tracking-tight text-zinc-900 dark:text-zinc-50">{title}</h1>
        {subtitle && <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">{subtitle}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function EmptyState({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center px-6 py-14 text-center">
      <div className="mb-4 grid size-12 place-items-center rounded-2xl bg-brand-50 text-brand-600 dark:bg-brand-950 dark:text-brand-400">
        {icon}
      </div>
      <h3 className="text-base font-semibold">{title}</h3>
      {children && <div className="mt-2 max-w-md text-sm text-zinc-500 dark:text-zinc-400">{children}</div>}
    </div>
  );
}

export function ProgressBar({ value, label }: { value: number; label: string }) {
  const pct = Math.max(0, Math.min(1, value)) * 100;
  return (
    <div
      className="h-1.5 w-full overflow-hidden rounded-full bg-brand-100 dark:bg-brand-950"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
    >
      <div className="h-full rounded-full bg-brand-500 transition-[width] duration-500" style={{ width: `${pct}%` }} />
    </div>
  );
}

export function Modal({
  open,
  onClose,
  title,
  children,
  footer,
  wide,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d || !open) return;
    // 測試環境（jsdom）沒有 showModal，直接打開
    if (typeof d.showModal === "function") {
      if (!d.open) d.showModal();
    } else {
      d.setAttribute("open", "");
    }
  }, [open]);
  if (!open) return null;
  return (
    <dialog
      ref={ref}
      onClose={onClose}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      aria-label={title}
      className={`fixed inset-0 z-50 m-auto w-[calc(100%-2rem)] ${wide ? "max-w-3xl" : "max-w-lg"} rounded-2xl border border-zinc-200 bg-white p-0 text-zinc-900 shadow-2xl backdrop:bg-zinc-950/40 backdrop:backdrop-blur-sm dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-100`}
    >
      <div className="flex items-center justify-between border-b border-zinc-100 px-5 py-4 dark:border-zinc-800">
        <h2 className="text-base font-semibold">{title}</h2>
        <button className="btn btn-ghost -mr-2 px-2" onClick={onClose} aria-label="關閉">
          <X className="size-4" />
        </button>
      </div>
      <div className="max-h-[70vh] overflow-y-auto px-5 py-4">{children}</div>
      {footer && (
        <div className="flex justify-end gap-2 border-t border-zinc-100 px-5 py-3 dark:border-zinc-800">{footer}</div>
      )}
    </dialog>
  );
}

export function Toaster() {
  const { toasts, dismiss } = useSession();
  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-[60] flex w-[min(24rem,calc(100%-2rem))] flex-col gap-2" aria-live="polite">
      {toasts.map((t) => {
        const Icon = t.tone === "ok" ? CircleCheck : t.tone === "error" ? CircleAlert : Info;
        const tone =
          t.tone === "ok"
            ? "text-brand-600 dark:text-brand-400"
            : t.tone === "error"
              ? "text-red-600 dark:text-red-400"
              : "text-zinc-500";
        return (
          <div key={t.id} role="status" className="card pointer-events-auto flex gap-3 p-3 text-sm">
            <Icon className={`mt-0.5 size-4 shrink-0 ${tone}`} aria-hidden />
            <div className="min-w-0 flex-1">
              <p className="font-medium">{t.text}</p>
              {t.details && t.details.length > 0 && (
                <ul className="mt-1 max-h-40 list-disc space-y-0.5 overflow-y-auto pl-4 text-xs text-zinc-500 dark:text-zinc-400">
                  {t.details.map((d, i) => (
                    <li key={i}>{d}</li>
                  ))}
                </ul>
              )}
            </div>
            <button className="text-zinc-400 hover:text-zinc-600" onClick={() => dismiss(t.id)} aria-label="關閉提示">
              <X className="size-4" />
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** 單一序列的橫條圖：一個顏色、數值在尾端、滑過顯示說明。 */
export function Bars({ rows, total, unit = "次" }: { rows: { key: string; label: ReactNode; count: number }[]; total?: number; unit?: string }) {
  const max = Math.max(1, ...rows.map((r) => r.count));
  const sum = total ?? rows.reduce((a, r) => a + r.count, 0);
  if (rows.length === 0) return <p className="text-sm text-zinc-400">還沒有資料</p>;
  return (
    <ul className="space-y-2">
      {rows.map((r) => {
        const share = sum > 0 ? Math.round((r.count / sum) * 100) : 0;
        return (
          <li key={r.key} className="group grid grid-cols-[minmax(5rem,9rem)_1fr_auto] items-center gap-3 text-sm" title={`${r.count} ${unit}（${share}%）`}>
            <span className="truncate text-zinc-600 dark:text-zinc-300">{r.label}</span>
            <span className="h-3.5 rounded-r bg-zinc-100 dark:bg-zinc-800">
              <span
                className="block h-full rounded-r-[4px] bg-brand-500 transition-[width] group-hover:bg-brand-600 dark:bg-brand-400 dark:group-hover:bg-brand-300"
                style={{ width: `${(r.count / max) * 100}%` }}
              />
            </span>
            <span className="w-16 text-right font-medium tabular-nums text-zinc-700 dark:text-zinc-200">
              {r.count}
              <span className="ml-1 text-xs font-normal text-zinc-400">{share}%</span>
            </span>
          </li>
        );
      })}
    </ul>
  );
}

export function StatTile({ label, value, hint }: { label: string; value: ReactNode; hint?: ReactNode }) {
  return (
    <div className="card p-4">
      <p className="text-xs font-medium text-zinc-500 dark:text-zinc-400">{label}</p>
      <p className="mt-1 text-2xl font-semibold tracking-tight tabular-nums">{value}</p>
      {hint && <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{hint}</p>}
    </div>
  );
}
