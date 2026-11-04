-- 時間欄位都是 Unix 秒數（UTC）。JSON 欄位存成文字。

-- 標注專案：一組資料和一份標注結構（schema）。
CREATE TABLE projects (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    schema      TEXT NOT NULL,              -- 標注結構，見 src/schema.rs
    ai          TEXT NOT NULL DEFAULT '{}', -- AI 輔助設定（標注說明、自動接受門檻）
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- 圖片和音訊檔。檔案放在資料夾裡，用內容的 SHA-256 命名，同一個檔案只存一份。
CREATE TABLE media (
    id            INTEGER PRIMARY KEY,
    sha256        TEXT NOT NULL UNIQUE,
    kind          TEXT NOT NULL CHECK (kind IN ('image', 'audio')),
    mime          TEXT NOT NULL,
    size          INTEGER NOT NULL,
    path          TEXT NOT NULL,            -- 相對於資料夾的路徑
    original_name TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL
);

-- 要標注的資料：文字、圖片或音訊，或文字加媒體。
CREATE TABLE items (
    id         INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    text       TEXT NOT NULL DEFAULT '',
    media_id   INTEGER REFERENCES media(id),
    meta       TEXT NOT NULL DEFAULT '{}',  -- 匯入時多出來的欄位
    hidden     INTEGER NOT NULL DEFAULT 0,  -- 1 = 不出現在標注佇列（舊版的 flag=0）
    dedupe_key TEXT NOT NULL,               -- 文字＋媒體的雜湊，同一個專案裡重複的資料只留一筆
    created_at INTEGER NOT NULL,
    UNIQUE (project_id, dedupe_key)
);
CREATE INDEX items_project ON items(project_id, hidden);

-- 標注者。kind = 'ai' 是批次預標時自動建立的模型標注者。
CREATE TABLE annotators (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    kind       TEXT NOT NULL DEFAULT 'human' CHECK (kind IN ('human', 'ai')),
    created_at INTEGER NOT NULL
);

-- 模型給的建議。人接受或修改後，會變成一筆 annotations。
CREATE TABLE suggestions (
    id         INTEGER PRIMARY KEY,
    item_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    provider   TEXT NOT NULL,
    model      TEXT NOT NULL,
    "values"   TEXT NOT NULL,
    confidence REAL,
    rationale  TEXT NOT NULL DEFAULT '',
    status     TEXT NOT NULL DEFAULT 'pending'
               CHECK (status IN ('pending', 'accepted', 'modified', 'rejected', 'auto')),
    created_at INTEGER NOT NULL
);
CREATE INDEX suggestions_item ON suggestions(item_id, created_at);

-- 標注。annotator_id 是 NULL 時代表沒有標注者身分（舊版資料或匯入的資料）。
-- source：human = 人工、ai_assisted = 人看過 AI 建議後送出、ai_auto = 高信心自動接受、
--         import = 匯入的檔案帶的標注、legacy = 從舊版 emoLabel.db 遷移。
CREATE TABLE annotations (
    id            INTEGER PRIMARY KEY,
    item_id       INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    annotator_id  INTEGER REFERENCES annotators(id),
    "values"      TEXT NOT NULL,
    source        TEXT NOT NULL CHECK (source IN ('human', 'ai_assisted', 'ai_auto', 'import', 'legacy')),
    suggestion_id INTEGER REFERENCES suggestions(id) ON DELETE SET NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);
CREATE UNIQUE INDEX annotations_one_per_annotator ON annotations(item_id, annotator_id)
    WHERE annotator_id IS NOT NULL;
CREATE INDEX annotations_item ON annotations(item_id);
