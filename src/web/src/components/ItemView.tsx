import { EyeOff, Hash, Image as ImageIcon, Music } from "lucide-react";
import { useState } from "react";
import { urls } from "../api";
import type { Item } from "../types";
import { Modal } from "./ui";

/** 一筆資料的內容：圖片或音訊在上，文字在下。 */
export function ItemView({ item }: { item: Item }) {
  const [zoom, setZoom] = useState(false);
  const meta = Object.entries(item.meta ?? {}).filter(([k]) => !k.startsWith("legacy_"));
  return (
    <article className="card overflow-hidden">
      {item.media?.kind === "image" && (
        <>
          <button
            type="button"
            className="block w-full bg-[repeating-conic-gradient(var(--color-zinc-100)_0_25%,transparent_0_50%)] bg-[length:16px_16px] dark:bg-[repeating-conic-gradient(var(--color-zinc-800)_0_25%,transparent_0_50%)]"
            onClick={() => setZoom(true)}
            title="點一下放大"
          >
            <img src={urls.media(item.media.id)} alt={item.text ? `圖片：${item.text.slice(0, 40)}` : "要標注的圖片"} className="mx-auto max-h-[22rem] object-contain" />
          </button>
          <Modal open={zoom} onClose={() => setZoom(false)} title="圖片" wide>
            <img src={urls.media(item.media.id)} alt="放大的圖片" className="mx-auto max-h-[75vh] object-contain" />
          </Modal>
        </>
      )}
      {item.media?.kind === "audio" && (
        <div className="flex items-center gap-3 border-b border-zinc-100 bg-zinc-50 px-5 py-4 dark:border-zinc-800 dark:bg-zinc-900">
          <Music className="size-5 text-brand-600" aria-hidden />
          <audio controls src={urls.media(item.media.id)} className="w-full" />
        </div>
      )}
      <div className="px-6 py-5">
        {item.text ? (
          <p className="text-lg leading-relaxed whitespace-pre-wrap text-zinc-900 sm:text-xl dark:text-zinc-50">{item.text}</p>
        ) : (
          <p className="text-sm text-zinc-400">（這筆只有{item.media?.kind === "audio" ? "音訊" : "圖片"}，沒有文字）</p>
        )}
        <div className="mt-4 flex flex-wrap items-center gap-1.5 text-xs text-zinc-500 dark:text-zinc-400">
          <span className="chip border-zinc-200 dark:border-zinc-700">
            <Hash className="size-3" />
            {item.id}
          </span>
          {item.media && (
            <span className="chip border-zinc-200 dark:border-zinc-700">
              {item.media.kind === "image" ? <ImageIcon className="size-3" /> : <Music className="size-3" />}
              {item.media.mime}
            </span>
          )}
          {item.hidden && (
            <span className="chip border-amber-300 text-amber-700 dark:border-amber-800 dark:text-amber-400">
              <EyeOff className="size-3" />
              已隱藏
            </span>
          )}
          {meta.slice(0, 4).map(([k, v]) => (
            <span key={k} className="chip border-zinc-200 dark:border-zinc-700" title={`${k}: ${String(v)}`}>
              <span className="text-zinc-400">{k}</span> {String(v).slice(0, 30)}
            </span>
          ))}
        </div>
      </div>
    </article>
  );
}
