import type {
  Annotation,
  Annotator,
  ImportReport,
  Item,
  ItemDetail,
  ItemFilter,
  JobStatus,
  LegacyImport,
  Meta,
  Project,
  Schema,
  AiSettings,
  Stats,
  Suggestion,
  Template,
  Values,
  AiStatus,
} from "./types";

/** API 錯誤：`message` 是後端的 `error`，`details` 是逐項的問題（例如每一列的匯入錯誤）。 */
export class ApiError extends Error {
  status: number;
  details: string[];
  constructor(status: number, message: string, details: string[] = []) {
    super(message);
    this.status = status;
    this.details = details;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, headers: {} };
  if (body instanceof FormData) {
    init.body = body;
  } else if (body !== undefined) {
    init.body = JSON.stringify(body);
    (init.headers as Record<string, string>)["Content-Type"] = "application/json";
  }
  const res = await fetch(`/api${path}`, init);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  const data = text ? JSON.parse(text) : undefined;
  if (!res.ok) {
    throw new ApiError(res.status, data?.error ?? `${res.status} ${res.statusText}`, data?.details ?? []);
  }
  return data as T;
}

const get = <T,>(path: string) => request<T>("GET", path);
const post = <T,>(path: string, body?: unknown) => request<T>("POST", path, body ?? {});
const put = <T,>(path: string, body: unknown) => request<T>("PUT", path, body);
const patch = <T,>(path: string, body: unknown) => request<T>("PATCH", path, body);
const del = (path: string) => request<void>("DELETE", path);

function upload<T>(path: string, file: File, fields: Record<string, string> = {}) {
  const form = new FormData();
  form.append("file", file);
  for (const [k, v] of Object.entries(fields)) form.append(k, v);
  return request<T>("POST", path, form);
}

export const api = {
  meta: () => get<Meta>("/meta"),
  templates: () => get<Template[]>("/templates"),

  projects: () => get<Project[]>("/projects"),
  project: (id: number) => get<Project>(`/projects/${id}`),
  createProject: (body: { name: string; description?: string; templateId?: string; schema?: Schema }) =>
    post<Project>("/projects", body),
  updateProject: (id: number, body: { name?: string; description?: string; schema?: Schema; ai?: AiSettings }) =>
    put<Project>(`/projects/${id}`, body),
  deleteProject: (id: number) => del(`/projects/${id}`),

  items: (id: number, q: { filter?: ItemFilter; q?: string; annotator?: number | null; page?: number; perPage?: number }) => {
    const params = new URLSearchParams();
    for (const [k, v] of Object.entries(q)) if (v !== undefined && v !== null && v !== "") params.set(k, String(v));
    return get<{ items: Item[]; total: number; page: number; perPage: number }>(`/projects/${id}/items?${params}`);
  },
  next: (id: number, annotator: number, exclude: number[]) =>
    get<{ itemId: number | null }>(`/projects/${id}/next?annotator=${annotator}&exclude=${exclude.join(",")}`),
  stats: (id: number, annotator?: number | null) =>
    get<Stats>(`/projects/${id}/stats${annotator ? `?annotator=${annotator}` : ""}`),

  item: (id: number, annotator?: number | null) =>
    get<ItemDetail>(`/items/${id}${annotator ? `?annotator=${annotator}` : ""}`),
  setHidden: (id: number, hidden: boolean) => patch<Item>(`/items/${id}`, { hidden }),
  saveAnnotation: (itemId: number, body: { annotatorId: number; values: Values; suggestionId?: number | null }) =>
    put<Annotation>(`/items/${itemId}/annotation`, body),
  deleteAnnotation: (id: number) => del(`/annotations/${id}`),
  suggest: (itemId: number) => post<Suggestion>(`/items/${itemId}/suggest`),
  rejectSuggestion: (id: number) => post<Suggestion>(`/suggestions/${id}/reject`),

  importFile: (id: number, file: File) => upload<ImportReport>(`/projects/${id}/import`, file),
  importLegacy: (file: File, name: string) =>
    upload<LegacyImport>("/import/legacy-db", file, {
      name,
      // 舊版存的是伺服器當地時間；假設舊伺服器和這台瀏覽器在同一個時區
      utcOffsetMinutes: String(-new Date().getTimezoneOffset()),
    }),

  batchStatus: (id: number) => get<{ job: JobStatus | null; ai: AiStatus }>(`/projects/${id}/ai/batch`),
  startBatch: (id: number, limit: number, threshold: number | null) =>
    post<JobStatus>(`/projects/${id}/ai/batch`, { limit, threshold }),
  cancelBatch: (id: number) => del(`/projects/${id}/ai/batch`),

  annotators: () => get<Annotator[]>("/annotators"),
  createAnnotator: (name: string) => post<Annotator>("/annotators", { name }),
  renameAnnotator: (id: number, name: string) => put<Annotator>(`/annotators/${id}`, { name }),
  deleteAnnotator: (id: number) => del(`/annotators/${id}`),
};

/** 下載用的網址（瀏覽器直接開，帶 Content-Disposition）。 */
export const urls = {
  media: (id: number) => `/api/media/${id}`,
  export: (id: number, params: Record<string, string | boolean>) =>
    `/api/projects/${id}/export?${new URLSearchParams(Object.entries(params).map(([k, v]) => [k, String(v)]))}`,
  importTemplate: (id: number) => `/api/projects/${id}/import-template.csv`,
  sample: (name: string) => `/api/samples/${name}`,
};
