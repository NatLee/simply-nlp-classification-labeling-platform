# 踩坑紀錄

開發時遇到的問題。每一條都寫：症狀、原因、怎麼修、以後怎麼避免。

## 資料庫（SQLite）

### 批次預標時出現「database is locked」

- **症狀**：批次預標同時跑兩筆時，有一筆失敗，錯誤是 `database is locked`，而且是立刻失敗，沒有等 `busy_timeout` 的 10 秒。
- **原因**：預設的 `BEGIN` 是「延後」的交易：先讀的時候只拿讀取鎖，第一次寫入才升級成寫入鎖。WAL 模式下，如果在這之間別的連線寫入了，SQLite 不會等，直接回 `SQLITE_BUSY`（等下去也不會成功，因為讀到的資料已經舊了）。
- **修法**：所有會寫入的交易都用 `BEGIN IMMEDIATE`，一開始就拿寫入鎖，拿不到就照 `busy_timeout` 等（`db::write_tx`）。
- **避免**：要寫入的交易一律用 `write_tx`，不要用 `pool.begin()`。

### 舊版資料庫的純數字文字變成整數

- **症狀**：舊版 emoLabel.db 裡的「2019」匯入後可能變成數字。
- **原因**：舊版的 `emotion.text` 宣告成 `INTEGER`。SQLite 的型別親和性會把看起來像數字的文字存成整數（或浮點數）。
- **修法**：讀舊資料時用 `CAST(text AS TEXT)` 轉回字串。整合測試用舊版一模一樣的 `CREATE TABLE` 重現。
- **限制**：很長的數字（超過 64 位元整數）在舊版存進去時就已經變成浮點數，轉回來會變成科學記號，沒辦法還原。

## AI 服務

### Claude API 版本 2026-10-21 不接受取樣參數

- 新版的 API 版本送 `temperature`、`top_p`、`top_k` 會回 400，所以兩種服務都不送，用預設值。分類結果的格式靠結構化輸出（JSON Schema）固定。
- Claude Opus 5.5 的思考不能關，也不能用 `budget_tokens`，用 `output_config.effort` 控制深度。分類這種短任務預設用 `low`（`ANTHROPIC_EFFORT`）。
- 思考也算在 `max_tokens` 裡，上限設太小會在思考到一半就被截斷（`stop_reason: "max_tokens"`），所以用 16,000。

### 結構化輸出不支援數字範圍

- Claude 和 OpenAI 的 JSON Schema 都不支援 `minimum`、`maximum`、`multipleOf`。範圍寫在 description 裡，模型回來的分數再夾回範圍、對齊到格子（`Schema::coerce_suggestion`）。
- OpenAI 的 strict 模式要求每個屬性都列在 `required`，選填的欄位改成「可以是 null」。

### 推理型模型把 Ollama 的上下文用完

- **症狀**：用 Ollama 的 `qwen3-vl:2b` 批次預標，4 筆裡有 1 筆失敗，錯誤是「模型的回答太長被截斷」。
- **原因**：這個版本會先「思考」。Ollama 的 OpenAI 相容 API 把思考放在 `reasoning` 欄位，不會出現在回答裡，但它照樣佔上下文。提示約 1,400 個 token，思考和回答用掉 2,000–2,700 個，Ollama 預設的上下文只有 4,096 個，就被截斷了。
- **修法**：換成不思考的 `qwen3-vl:2b-instruct`（圖片 10 秒、文字 1–2 秒），或啟動 Ollama 時加 `OLLAMA_CONTEXT_LENGTH=8192`。兩種都實測過可以解決。錯誤訊息也改成直接寫出這兩個做法。
- **避免**：OpenAI 相容的 API 沒辦法在每個請求裡設定上下文長度，要在 Ollama 那邊設定。

### 小模型會把 JSON 包在 ```json 裡

- 不支援結構化輸出的服務（或很小的模型）常在 JSON 前後多寫幾句話、加上 Markdown 區塊，推理模型還會先輸出 `<think>…</think>`。解析時一律取第一個 `{` 到最後一個 `}`（`llm::extract_json`）。

## 登入

- **權限只寫在一個地方**：`auth::required_level` 這張表決定每個「方法＋路徑」要什麼權限，沒列到的一律要登入。新增 API 時先想它屬於哪一級，再看表裡要不要加一行。
- **巢狀路由看到的是去掉 `/api` 的路徑**：檢查權限的 middleware 掛在 `/api` 底下的 router 上，所以表裡寫的是 `/projects/1`，不是 `/api/projects/1`。
- **截圖腳本要每張用新的瀏覽器 context**：同一個瀏覽器的 cookie 是共用的，前一張登入過，下一張就看不到登入頁。

## 開發環境（Windows＋Git Bash＋Docker）

- **這台開發機沒有 Rust**：編譯和測試都在 Docker 裡跑（`rust:1-trixie` 加上 `rustup component add rustfmt clippy`）。`cargo` 的下載快取和 `target` 用 Docker volume 保存，第二次之後只要幾十秒。
- **Git Bash 會改寫路徑**：`docker run -v` 的路徑前面要加 `MSYS_NO_PATHCONV=1`。
- **用 curl 送中文 JSON 會變成亂碼**：先把 JSON 寫進檔案再用 `--data @檔案`，或改用 node 的 `fetch`。
- **pnpm 的下載快取會跑進專案資料夾**：在容器裡把 `node_modules` 掛成 volume 時，pnpm 會把快取（`.pnpm-store`，約 200 MB）放在掛載的專案資料夾裡。已經加進 `.gitignore`；在容器裡跑時也可以設 `npm_config_store_dir=/tmp/pnpm-store`。
