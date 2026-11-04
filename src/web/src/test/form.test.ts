import { describe, expect, it } from "vitest";
import { applyArrow, applyDigit, compact, initialValues, missingRequired, snap, valuesEqual } from "../lib/form";
import { agreementLevel } from "../lib/format";
import type { Field } from "../types";
import { legacySchema } from "./fakeApi";

const [score, tag] = legacySchema.fields;
const multi: Field = { id: "m", label: "M", type: "multi", options: [{ value: "a" }, { value: "b" }], maxSelected: 1 };
const fine: Field = { id: "x", label: "X", type: "scale", min: 0, max: 1, step: 0.1 };

describe("表單邏輯", () => {
  it("起始值：用自己的舊標注，沒有就用預設值", () => {
    expect(initialValues(legacySchema)).toEqual({ score: 3, tag: "wow", type: "其它" });
    expect(initialValues(legacySchema, { score: 1 })).toEqual({ score: 1 });
  });

  it("數字鍵", () => {
    expect(applyDigit(score, {}, 4)).toEqual({ score: 4 });
    expect(applyDigit(score, {}, 7)).toBeNull(); // 超出 1–5
    expect(applyDigit(tag, {}, 2)).toEqual({ tag: "angry" });
    expect(applyDigit(tag, {}, 9)).toBeNull();
    expect(applyDigit(multi, {}, 1)).toEqual({ m: ["a"] });
    expect(applyDigit(multi, { m: ["a"] }, 1)).toEqual({ m: [] }); // 再按一次取消
    expect(applyDigit(multi, { m: ["a"] }, 2)).toBeNull(); // 最多選 1 個
  });

  it("方向鍵", () => {
    expect(applyArrow(score, { score: 5 }, 1)).toEqual({ score: 5 });
    expect(applyArrow(score, {}, -1)).toEqual({ score: 2 }); // 從預設 3 開始
    expect(applyArrow(fine, { x: 0.2 }, 1)).toEqual({ x: 0.3 }); // 不會變成 0.30000000000000004
    expect(applyArrow(tag, { tag: "wow" }, 1)).toEqual({ tag: "happy" }); // 繞回第一個
  });

  it("比較、去空值、必填", () => {
    expect(valuesEqual({ a: ["x", "y"], b: "" }, { a: ["y", "x"] })).toBe(true);
    expect(valuesEqual({ a: 1 }, { a: 2 })).toBe(false);
    expect(compact({ a: "", b: [], c: " hi " })).toEqual({ c: "hi" });
    expect(missingRequired(legacySchema, { score: 3 }).map((f) => f.id)).toEqual(["tag"]);
    expect(snap(0.36, 0, 1, 0.1)).toBe(0.4);
  });

  it("一致性的白話分級", () => {
    expect(agreementLevel(0.9)?.label).toBe("幾乎完全一致");
    expect(agreementLevel(0.5)?.label).toBe("中度一致");
    expect(agreementLevel(-0.1)?.label).toBe("比碰巧還差");
    expect(agreementLevel(null)).toBeNull();
  });
});
