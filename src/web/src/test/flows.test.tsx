// 主要流程：用鍵盤標注、接受 AI 建議、必填檢查、改標注結構、匯入檔案。
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it } from "vitest";
import { AppRoutes } from "../App";
import { SessionProvider } from "../lib/session";
import type { Schema } from "../types";
import { annotators, detail, error, fakeApi, legacySchema, metaOff, metaOn, project, stats, type Call } from "./fakeApi";

function renderAt(path: string) {
  return render(
    <SessionProvider>
      <MemoryRouter initialEntries={[path]}>
        <AppRoutes />
      </MemoryRouter>
    </SessionProvider>,
  );
}

const puts = (calls: Call[]) => calls.filter((c) => c.method === "PUT");

beforeEach(() => {
  localStorage.setItem("snl.annotator", "7");
});

describe("標注頁", () => {
  it("用鍵盤選分數和情緒，Enter 送出後自動換下一筆", async () => {
    let next = 0;
    const { calls } = fakeApi({
      "GET /meta": metaOff,
      "GET /annotators": annotators,
      "GET /projects": [project()],
      "GET /projects/1": project(),
      "GET /projects/1/stats": stats,
      "GET /projects/1/next": () => ({ itemId: [11, 12][next++] ?? null }),
      "GET /items/11": detail(11, "第一句：今天好開心"),
      "GET /items/12": detail(12, "第二句：排隊好久"),
      "PUT /items/11/annotation": (c: Call) => ({ id: 1, itemId: 11, values: (c.body as { values: unknown }).values }),
    });
    const user = userEvent.setup();
    renderAt("/p/1");
    expect(await screen.findByText("第一句：今天好開心")).toBeInTheDocument();
    // 第一個欄位是分數：按 4；↓ 換到情緒，按 1 是 happy
    await user.keyboard("4");
    await user.keyboard("{ArrowDown}1");
    expect(screen.getByRole("radio", { name: /happy/ })).toHaveAttribute("aria-checked", "true");
    // 打一個自訂標籤
    await user.click(screen.getByRole("textbox", { name: /自訂標籤/ }));
    await user.keyboard("城市{Enter}");
    await user.keyboard("{Escape}");
    await user.keyboard("{Enter}");
    expect(await screen.findByText("第二句：排隊好久")).toBeInTheDocument();
    const saved = puts(calls)[0];
    expect(saved.body).toEqual({
      annotatorId: 7,
      values: { score: 4, tag: "happy", type: "其它", tag_opt: ["城市"] },
      suggestionId: null,
    });
    // 跳過的資料會帶在 exclude 裡
    await user.keyboard("s");
    await waitFor(() => expect(calls.some((c) => c.path.includes("exclude=12"))).toBe(true));
  });

  it("必填沒填就不送出", async () => {
    const schema: Schema = { fields: [{ ...legacySchema.fields[1], default: null } as Schema["fields"][number]] };
    const { calls } = fakeApi({
      "GET /meta": metaOff,
      "GET /annotators": annotators,
      "GET /projects": [project(schema)],
      "GET /projects/1": project(schema),
      "GET /projects/1/stats": stats,
      "GET /projects/1/next": { itemId: 11 },
      "GET /items/11": detail(11, "沒選情緒"),
    });
    const user = userEvent.setup();
    renderAt("/p/1");
    await screen.findByText("沒選情緒");
    await user.keyboard("{Enter}");
    expect(await screen.findByText(/還沒填：情緒/)).toBeInTheDocument();
    expect(puts(calls)).toHaveLength(0);
  });

  it("顯示 AI 建議，接受後記下是哪一筆建議", async () => {
    const suggestion = {
      id: 5,
      itemId: 11,
      provider: "anthropic",
      model: "claude-opus-5-5",
      values: { score: 2, tag: "angry", type: "閒聊" },
      confidence: 0.83,
      rationale: "語氣很不耐煩。",
      status: "pending",
      createdAt: 0,
    };
    let next = 0;
    const { calls } = fakeApi({
      "GET /meta": metaOn,
      "GET /annotators": annotators,
      "GET /projects": [project()],
      "GET /projects/1": project(),
      "GET /projects/1/stats": stats,
      "GET /projects/1/next": () => ({ itemId: [11, 12][next++] ?? null }),
      "GET /items/11": detail(11, "排了一小時還沒進去", { suggestion }),
      "GET /items/12": detail(12, "下一句"),
      "PUT /items/11/annotation": { id: 1 },
      "POST /items/12/suggest": { ...suggestion, id: 6, itemId: 12, rationale: "新的建議" },
    });
    const user = userEvent.setup();
    renderAt("/p/1");
    expect(await screen.findByText("語氣很不耐煩。")).toBeInTheDocument();
    expect(screen.getByText("83%")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /接受並下一筆/ }));
    await screen.findByText("下一句");
    expect(puts(calls)[0].body).toEqual({ annotatorId: 7, values: { score: 2, tag: "angry", type: "閒聊" }, suggestionId: 5 });
    // G 產生新的建議
    await user.keyboard("g");
    expect(await screen.findByText("新的建議")).toBeInTheDocument();
  });

  it("沒有選標注者時先請人選", async () => {
    localStorage.removeItem("snl.annotator");
    fakeApi({
      "GET /meta": metaOff,
      "GET /annotators": annotators,
      "GET /projects": [project()],
      "GET /projects/1": project(),
    });
    const user = userEvent.setup();
    renderAt("/p/1");
    expect(await screen.findByText("先選一位標注者")).toBeInTheDocument();
    const main = screen.getByRole("main");
    await user.click(within(main).getByRole("button", { name: /Amy/ }));
    expect(localStorage.getItem("snl.annotator")).toBe("7");
  });
});

describe("專案設定", () => {
  it("新增欄位後儲存；後端擋下時顯示原因", async () => {
    let attempt = 0;
    const { calls } = fakeApi({
      "GET /meta": metaOff,
      "GET /annotators": annotators,
      "GET /projects": [project()],
      "GET /projects/1": project(),
      "PUT /projects/1": (c: Call) =>
        attempt++ === 0 ? error(409, "這樣改會讓已經有的標注不合法") : project((c.body as { schema: Schema }).schema),
    });
    const user = userEvent.setup();
    renderAt("/p/1/settings");
    await screen.findByText("標注結構");
    await user.click(screen.getByRole("button", { name: "多選" }));
    await user.click(screen.getAllByRole("button", { name: /儲存/ })[0]);
    expect(await screen.findByText("這樣改會讓已經有的標注不合法")).toBeInTheDocument();
    const body = puts(calls)[0].body as { schema: Schema };
    expect(body.schema.fields.map((f) => f.type)).toEqual(["scale", "single", "single", "tags", "multi"]);
    await user.click(screen.getAllByRole("button", { name: /儲存/ })[0]);
    expect(await screen.findByText("已儲存")).toBeInTheDocument();
  });
});

describe("匯入", () => {
  it("上傳檔案並顯示結果", async () => {
    const { calls } = fakeApi({
      "GET /meta": metaOff,
      "GET /annotators": annotators,
      "GET /projects": [project()],
      "GET /projects/1": project(),
      "GET /projects/1/ai/batch": { job: null, ai: metaOff.ai },
      "POST /projects/1/import": { format: "csv", rows: 3, created: 2, duplicates: 1, annotations: 0, media: 0 },
    });
    const user = userEvent.setup();
    renderAt("/p/1/data");
    const input = await screen.findByLabelText("選擇要匯入的檔案");
    await user.upload(input, new File(["Text\na\nb\na\n"], "textDataExample.csv", { type: "text/csv" }));
    expect(await screen.findByText("匯入完成：新增 2 筆")).toBeInTheDocument();
    const sent = calls.find((c) => c.method === "POST")!.body as FormData;
    expect((sent.get("file") as File).name).toBe("textDataExample.csv");
  });
});

describe("內建登入", () => {
  it("登入、以自己的身分標注、登出", async () => {
    const amy = { id: 1, username: "amy", role: "annotator", annotatorId: 7 };
    let signedIn = false;
    const { calls } = fakeApi({
      "GET /meta": { ...metaOff, auth: { enabled: true } },
      "GET /auth/me": () => ({ enabled: true, user: signedIn ? amy : null }),
      "POST /auth/login": (c: Call) => {
        const b = c.body as { username: string; password: string };
        if (b.password !== "amy-password") return error(401, "帳號或密碼不對，或帳號已停用");
        signedIn = true;
        return { enabled: true, user: amy };
      },
      "POST /auth/logout": () => {
        signedIn = false;
        return undefined;
      },
      "GET /annotators": annotators,
      "GET /projects": [project()],
      "GET /projects/1": project(),
      "GET /projects/1/stats": stats,
      "GET /projects/1/next": { itemId: 11 },
      "GET /items/11": detail(11, "登入後的第一句"),
      "PUT /items/11/annotation": { id: 1 },
    });
    // 啟用登入時，存在瀏覽器的標注者選擇不算數
    localStorage.setItem("snl.annotator", "99");
    const user = userEvent.setup();
    renderAt("/p/1");
    await user.type(await screen.findByLabelText("帳號"), "amy");
    await user.type(screen.getByLabelText("密碼"), "wrong-password");
    await user.click(screen.getByRole("button", { name: "登入" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("帳號或密碼不對");
    await user.type(screen.getByLabelText("密碼"), "amy-password");
    await user.click(screen.getByRole("button", { name: "登入" }));
    expect(await screen.findByText("登入後的第一句")).toBeInTheDocument();
    // 管理員才看得到的「帳號管理」不會出現
    expect(screen.queryByText("帳號管理")).not.toBeInTheDocument();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(puts(calls)).toHaveLength(1));
    expect((puts(calls)[0].body as { annotatorId: number }).annotatorId).toBe(7);
    await user.click(screen.getAllByRole("button", { name: "登出" })[0]);
    expect(await screen.findByLabelText("帳號")).toBeInTheDocument();
    expect(calls.some((c) => c.method === "POST" && c.path === "/auth/logout")).toBe(true);
  });
});
