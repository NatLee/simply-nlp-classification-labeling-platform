// 測試用的假後端：攔下 fetch，依「方法＋路徑」回應，並記下每個請求。
import { vi } from "vitest";
import type { Project, Schema } from "../types";

export type Call = { method: string; path: string; body: unknown };
type Handler = (call: Call) => unknown | Promise<unknown>;

export function fakeApi(routes: Record<string, Handler | unknown>) {
  const calls: Call[] = [];
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), "http://test");
    const method = (init?.method ?? "GET").toUpperCase();
    let body: unknown = undefined;
    if (typeof init?.body === "string") body = JSON.parse(init.body);
    else if (init?.body instanceof FormData) body = init.body;
    const call = { method, path: url.pathname.replace(/^\/api/, "") + url.search, body };
    calls.push(call);
    // 先找完整的路徑（含查詢字串），再找不含查詢字串的
    const route = routes[`${method} ${call.path}`] ?? routes[`${method} ${url.pathname.replace(/^\/api/, "")}`];
    if (route === undefined) {
      return new Response(JSON.stringify({ error: `no fake for ${method} ${call.path}` }), { status: 404 });
    }
    try {
      const result = typeof route === "function" ? await (route as Handler)(call) : route;
      if (result instanceof Response) return result;
      return new Response(result === undefined ? null : JSON.stringify(result), { status: result === undefined ? 204 : 200 });
    } catch (e) {
      return new Response(JSON.stringify({ error: String(e) }), { status: 500 });
    }
  });
  vi.stubGlobal("fetch", fetchMock);
  return { calls, fetchMock };
}

export const error = (status: number, message: string, details: string[] = []) =>
  new Response(JSON.stringify({ error: message, details }), { status });

export const legacySchema: Schema = {
  fields: [
    { id: "score", label: "評分", type: "scale", min: 1, max: 5, step: 1, default: 3, required: true },
    {
      id: "tag",
      label: "情緒",
      type: "single",
      required: true,
      default: "wow",
      options: [
        { value: "happy", label: "happy", emoji: "😀" },
        { value: "angry", label: "angry", emoji: "😡" },
        { value: "wow", label: "wow", emoji: "😲" },
      ],
    },
    { id: "type", label: "類型", type: "single", default: "其它", options: [{ value: "其它" }, { value: "閒聊" }] },
    { id: "tag_opt", label: "自訂標籤", type: "tags" },
  ],
};

export function project(schema: Schema = legacySchema, extra: Partial<Project> = {}): Project {
  return {
    id: 1,
    name: "測試專案",
    description: "",
    schema,
    ai: { instructions: "", autoAcceptThreshold: 0.9 },
    createdAt: 0,
    updatedAt: 0,
    counts: { items: 2, visible: 2, labeled: 0 },
    ...extra,
  };
}

export const stats = {
  items: { total: 2, visible: 2, hidden: 0, labeled: 0, coverRate: 0, meanLabels: 0, stdLabels: 0 },
  mine: 0,
  annotators: [],
  anonymous: 0,
  sources: {},
  suggestions: {},
  fields: [],
};

export function detail(id: number, text: string, extra: Record<string, unknown> = {}) {
  return {
    item: { id, projectId: 1, text, media: null, meta: {}, hidden: false, createdAt: 0, annotationCount: 0 },
    annotations: [],
    mine: null,
    suggestion: null,
    tagHints: { tag_opt: ["政治"] },
    ...extra,
  };
}

export const annotators = [{ id: 7, name: "Amy", kind: "human", createdAt: 0 }];
export const metaOff = { version: "test", ai: { enabled: false, reason: "沒有設定 LLM_PROVIDER" }, maxUploadMb: 200 };
export const metaOn = { version: "test", ai: { enabled: true, provider: "anthropic", model: "claude-opus-5-5" }, maxUploadMb: 200 };
