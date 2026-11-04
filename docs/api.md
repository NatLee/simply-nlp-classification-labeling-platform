# HTTP API

前端用的就是這組 API，也可以直接拿來寫腳本。所有路徑都在 `/api` 底下，回應是 JSON，欄位名稱是 camelCase。錯誤的格式固定是：

```json
{ "error": "標注有問題", "details": ["「評分」要介於 1 和 5", "「情緒」沒有「joy」這個選項"] }
```

`details` 只在有逐項問題時出現（欄位驗證、匯入時每一列的錯誤）。

預設不用登入。設了 `ADMIN_PASSWORD` 之後，除了 `/health`、`/meta`、`/auth/login`、`/auth/logout`、`/auth/me`，所有 API 都要帶 session cookie（`snl_session`），沒登入回 401，權限不夠回 403。來自別的網站的寫入請求（`Origin` 和 `PUBLIC_URL` 或請求的 Host 不同）回 403。權限表在 `src/api/src/auth.rs` 的 `required_level`。

## 登入（啟用時）

| 方法 | 路徑 | 權限 | 說明 |
|---|---|---|---|
| GET | `/auth/me` | 公開 | `{enabled, user}`；`user` 是 `{id, username, role, annotatorId}` 或 `null` |
| POST | `/auth/login` | 公開 | `{username, password}`，成功時設定 cookie。同一個帳號 15 分鐘內失敗 5 次回 429 |
| POST | `/auth/logout` | 公開 | 刪掉 session、清掉 cookie |
| GET | `/users` | 管理員 | 所有帳號 |
| POST | `/users` | 管理員 | `{username, password, role}`；`role` 是 `annotator`、`reviewer` 或 `admin`，密碼至少 8 個字元。會連到同名的標注者 |
| PUT | `/users/{id}` | 管理員 | `{role?, disabled?, password?}`；不能停用或降級自己。停用、重設密碼後那個人會被登出 |

各角色能做的事：

| 角色 | 可以 |
|---|---|
| 標注者（`annotator`） | 讀取所有資料和統計、用自己的身分標注、刪自己的標注、要 AI 建議 |
| 審核者（`reviewer`） | 再加上匯出、隱藏資料、開始或停止批次預標、刪別人的標注 |
| 管理員（`admin`） | 全部，包括建立和修改專案、匯入、管理標注者和帳號 |

登入後，`PUT /items/{id}/annotation` 和 `GET /projects/{id}/next` 的標注者 id 必須是自己的（`user.annotatorId`），否則回 403。

## 基本

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/health` | 回 `ok` |
| GET | `/meta` | 版本、AI 狀態（`{enabled, provider, model}` 或 `{enabled: false, reason}`）、上傳上限、`auth: {enabled}`。不會回 API key 或網址 |
| GET | `/templates` | 建立專案用的範本（標注結構＋標注說明） |
| GET | `/samples/{name}` | 下載內建的範例檔：`textDataExample.csv`、`labelDataExample.csv`、`sentiment-sample.jsonl`、`image-text-sample.zip` |

## 專案

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/projects` | 所有專案，附 `counts: {items, visible, labeled}` |
| POST | `/projects` | `{name, description?, templateId? \| schema?, ai?}`，`templateId` 和 `schema` 二選一 |
| GET | `/projects/{id}` | 一個專案 |
| PUT | `/projects/{id}` | `{name?, description?, schema?, ai?}`。改結構時，已經有的標注都要仍然合法，否則回 409 |
| DELETE | `/projects/{id}` | 刪除專案和它的資料、標注、AI 建議，並清掉沒人用的媒體檔 |
| GET | `/projects/{id}/items` | `?filter=all\|unlabeled\|labeled\|mine\|pending\|hidden&q=搜尋&annotator=&page=&perPage=`（`perPage` 最多 200） |
| GET | `/projects/{id}/next` | `?annotator=&exclude=1,2,3`：替這位標注者挑下一筆，回 `{itemId}`，標完了是 `null` |
| GET | `/projects/{id}/stats` | `?annotator=`：進度、每位標注者的數量、標注來源、每個欄位的分布和一致性 |

### 標注結構（schema）

```json
{
  "fields": [
    { "id": "score", "label": "評分", "type": "scale", "min": 1, "max": 5, "step": 1, "default": 3, "required": true,
      "minLabel": "很負面", "maxLabel": "很正面" },
    { "id": "tag", "label": "情緒", "type": "single", "required": true, "default": "wow",
      "options": [{ "value": "happy", "label": "happy", "emoji": "😀" }, { "value": "wow", "label": "wow", "emoji": "😲" }] },
    { "id": "topics", "label": "主題", "type": "multi", "maxSelected": 2, "options": [{ "value": "food" }, { "value": "travel" }] },
    { "id": "tag_opt", "label": "自訂標籤", "type": "tags", "suggestions": ["美食"] },
    { "id": "note", "label": "說明", "type": "text", "maxLength": 2000 }
  ]
}
```

- `id`：小寫英文字母開頭，只能有小寫英數字和底線，最多 40 字元；不能用匯出檔保留的名稱（`id`、`text`、`media`、`annotator`、`source` 等）。
- `scale`：範圍要能被 `step` 整除，最多 10,000 格。
- `single` 至少 2 個選項，`multi` 至少 1 個，最多 200 個。
- 標注的值：`scale` 是數字（會對齊到格子）、`single` 和 `text` 是字串、`multi` 和 `tags` 是字串陣列。空字串、空陣列、`null` 都視為沒填。

### AI 設定（`ai`）

```json
{ "instructions": "給模型和標注者看的標注說明", "autoAcceptThreshold": 0.9 }
```

`autoAcceptThreshold` 是 `null` 時，批次預標的建議全部留給人確認。

## 資料和標注

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/items/{id}` | `?annotator=`：`{item, annotations, mine, suggestion, tagHints}`。`tagHints` 是每個自由標籤欄位的提示：這筆以前用過的、結構裡的建議、整個專案常用的 |
| PATCH | `/items/{id}` | `{hidden}`：隱藏的資料不會出現在標注佇列 |
| PUT | `/items/{id}/annotation` | `{annotatorId, values, suggestionId?}`：新增或更新這位標注者的標注。帶 `suggestionId` 表示表單是從那筆 AI 建議帶入的，來源記成 `ai_assisted`，建議標成 `accepted`（沒改）或 `modified`（改過） |
| DELETE | `/annotations/{id}` | 刪除一筆標注 |
| GET | `/media/{id}` | 圖片或音訊，支援 Range |

標注的來源（`source`）：`human`（人工）、`ai_assisted`（看過 AI 建議後送出）、`ai_auto`（批次預標時高信心自動接受）、`import`（匯入的檔案帶的）、`legacy`（舊版 emoLabel.db）。

## AI 輔助

| 方法 | 路徑 | 說明 |
|---|---|---|
| POST | `/items/{id}/suggest` | 請模型給建議，存起來並回傳 `{values, confidence, rationale, model, status}`。AI 沒開時回 503，模型服務出錯時回 502 |
| POST | `/suggestions/{id}/reject` | 把建議標成忽略 |
| POST | `/projects/{id}/ai/batch` | `{limit?: 50, threshold?}`：批次預標還沒人標、也還沒有建議的資料（最多 5,000 筆），回 202 |
| GET | `/projects/{id}/ai/batch` | `{job, ai}`：進度（`total`、`done`、`autoAccepted`、`failed`、`errors`） |
| DELETE | `/projects/{id}/ai/batch` | 停止 |

## 標注者

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/annotators` | 所有標注者（`kind` 是 `human` 或 `ai`） |
| POST | `/annotators` | `{name}`：名字不分大小寫不能重複，`AI ·` 開頭保留給模型 |
| PUT | `/annotators/{id}` | `{name}`：改名 |
| DELETE | `/annotators/{id}` | 已經有標注的不能刪（409） |

## 匯入匯出

| 方法 | 路徑 | 說明 |
|---|---|---|
| POST | `/projects/{id}/import` | multipart，欄位名稱 `file`：CSV、JSONL、JSON 陣列或 zip。有任何一列不對就整批不匯入，`details` 列出每一列的問題 |
| GET | `/projects/{id}/import-template.csv` | 依這個專案的欄位產生的 CSV 範本 |
| GET | `/projects/{id}/export` | `?format=csv\|jsonl\|zip\|chat-jsonl&mode=annotations\|items&excludeAuto=false&includeHidden=false` |
| POST | `/import/legacy-db` | multipart：`file`（舊版的 emoLabel.db）、`name`、`utcOffsetMinutes`（舊伺服器的時區，台灣是 480）。建立一個新專案 |

格式細節見 README 的「匯入」和「匯出」。
