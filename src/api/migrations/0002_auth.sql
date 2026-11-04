-- 內建登入（選用）：設了 ADMIN_PASSWORD 才會用到這兩張表。

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,             -- argon2
    role          TEXT NOT NULL CHECK (role IN ('admin', 'annotator', 'reviewer')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER
);

-- 只存 token 的 SHA-256，資料庫外洩也不能拿來登入。
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);

-- 帳號和標注者用名字連在一起：登入後，標注者就是帳號本人。
ALTER TABLE annotators ADD COLUMN user_id INTEGER REFERENCES users(id);
CREATE UNIQUE INDEX annotators_user ON annotators(user_id) WHERE user_id IS NOT NULL;
