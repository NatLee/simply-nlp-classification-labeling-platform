// 和後端（src/api）的 JSON 對應。欄位名稱都是 camelCase。

export type Choice = { value: string; label?: string; emoji?: string; description?: string };

type FieldBase = { id: string; label: string; description?: string; required?: boolean };

export type ScaleField = FieldBase & {
  type: "scale";
  min: number;
  max: number;
  step: number;
  default?: number | null;
  minLabel?: string;
  maxLabel?: string;
};
export type SingleField = FieldBase & { type: "single"; options: Choice[]; default?: string | null };
export type MultiField = FieldBase & { type: "multi"; options: Choice[]; maxSelected?: number | null };
export type TagsField = FieldBase & { type: "tags"; suggestions?: string[] };
export type TextField = FieldBase & { type: "text"; maxLength?: number };

export type Field = ScaleField | SingleField | MultiField | TagsField | TextField;
export type FieldType = Field["type"];
export type Schema = { fields: Field[] };

export type Value = number | string | string[];
export type Values = Record<string, Value>;

export type AiSettings = { instructions: string; autoAcceptThreshold: number | null };

export type Project = {
  id: number;
  name: string;
  description: string;
  schema: Schema;
  ai: AiSettings;
  createdAt: number;
  updatedAt: number;
  counts?: { items: number; visible: number; labeled: number };
};

export type Template = { id: string; name: string; description: string; schema: Schema; instructions: string };

export type Media = { id: number; kind: "image" | "audio"; mime: string; sha256: string };

export type Item = {
  id: number;
  projectId: number;
  text: string;
  media: Media | null;
  meta: Record<string, unknown>;
  hidden: boolean;
  createdAt: number;
  annotationCount: number;
};

export type Source = "human" | "ai_assisted" | "ai_auto" | "import" | "legacy";

export type Annotation = {
  id: number;
  itemId: number;
  annotatorId: number | null;
  annotatorName: string | null;
  annotatorKind: "human" | "ai" | null;
  values: Values;
  source: Source;
  suggestionId: number | null;
  createdAt: number;
  updatedAt: number;
};

export type Suggestion = {
  id: number;
  itemId: number;
  provider: string;
  model: string;
  values: Values;
  confidence: number | null;
  rationale: string;
  status: "pending" | "accepted" | "modified" | "rejected" | "auto";
  createdAt: number;
};

export type ItemDetail = {
  item: Item;
  annotations: Annotation[];
  mine: Annotation | null;
  suggestion: Suggestion | null;
  tagHints: Record<string, string[]>;
};

export type Annotator = { id: number; name: string; kind: "human" | "ai"; createdAt: number };

export type AiStatus = { enabled: true; provider: string; model: string } | { enabled: false; reason?: string | null };

export type Meta = { version: string; ai: AiStatus; maxUploadMb: number; auth: { enabled: boolean } };

/** 內建登入的角色：annotator 標注；reviewer 再加上匯出、隱藏資料、批次預標；admin 全部。 */
export type Role = "annotator" | "reviewer" | "admin";
export type AuthUser = { id: number; username: string; role: Role; annotatorId: number };
export type AuthState = { enabled: boolean; user: AuthUser | null };
export type Account = {
  id: number;
  username: string;
  role: Role;
  disabled: boolean;
  createdAt: number;
  lastLoginAt: number | null;
  annotatorId: number | null;
};

export type ItemFilter = "all" | "unlabeled" | "labeled" | "mine" | "pending" | "hidden";

export type ImportReport = {
  format: string;
  rows: number;
  created: number;
  duplicates: number;
  annotations: number;
  media: number;
};

export type LegacyImport = {
  projectId: number;
  items: number;
  hiddenItems: number;
  annotations: number;
  orphanLabels: number;
  addedOptions: string[];
};

export type JobStatus = {
  running: boolean;
  total: number;
  done: number;
  autoAccepted: number;
  failed: number;
  cancelled: boolean;
  threshold: number | null;
  errors: string[];
  startedAt: number;
};

export type Bucket = { value: Value; label: string; count: number };
export type PairAgreement = { a: string; b: string; n: number; cohenKappa: number | null; percent: number | null };
export type Agreement = {
  itemsUsed: number;
  fleissKappa: number | null;
  krippendorffAlpha: number | null;
  pairs: PairAgreement[];
};
export type FieldStats = { id: string; label: string; type: FieldType; distribution: Bucket[]; agreement: Agreement | null };

export type Stats = {
  items: {
    total: number;
    visible: number;
    hidden: number;
    labeled: number;
    coverRate: number;
    meanLabels: number;
    stdLabels: number;
  };
  mine: number | null;
  annotators: { id: number; name: string; kind: "human" | "ai"; count: number }[];
  anonymous: number;
  sources: Partial<Record<Source, number>>;
  suggestions: Partial<Record<Suggestion["status"], number>>;
  fields: FieldStats[];
};
