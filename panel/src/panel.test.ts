import { describe, expect, it } from "vitest";
import { errorMessage } from "./api";
import { auditToCsv, csvField } from "./export";
import { detectLang, dictionaries } from "./i18n";
import { atLeast } from "./types";

describe("translations", () => {
  it("have the same keys in Russian and English and no empty text", () => {
    expect(Object.keys(dictionaries.en).sort()).toEqual(Object.keys(dictionaries.ru).sort());
    for (const lang of ["ru", "en"] as const) {
      for (const value of Object.values(dictionaries[lang])) expect(value.trim()).not.toBe("");
    }
  });
  it("detects the browser language", () => {
    expect(detectLang("ru-RU")).toBe("ru");
    expect(detectLang("de")).toBe("en");
    expect(detectLang(undefined)).toBe("en");
  });
});

describe("csv export", () => {
  it("quotes separators and neutralises formulas", () => {
    expect(csvField("a,b")).toBe('"a,b"');
    expect(csvField('say "hi"')).toBe('"say ""hi"""');
    expect(csvField("=1+1")).toBe("'=1+1");
    expect(csvField("plain")).toBe("plain");
  });
  it("writes a header and one line per event", () => {
    const csv = auditToCsv(
      [{ id: 7, actor_user_id: "u", action: "grant.issued", target: null, detail: { a: 1 }, created_at: "2026-10-04T00:00:00Z" }],
      () => "me@example.org",
    );
    const lines = csv.trim().split("\r\n");
    expect(lines[0]).toBe("id,time,actor,action,target,detail");
    expect(lines[1]).toBe('7,2026-10-04T00:00:00Z,me@example.org,grant.issued,,"{""a"":1}"');
  });
});

describe("api helpers", () => {
  it("reads the server error text", () => {
    expect(errorMessage({ error: "registration is closed" }, 403)).toBe("registration is closed");
    expect(errorMessage(null, 500)).toBe("HTTP 500");
  });
  it("orders roles", () => {
    expect(atLeast("owner", "admin")).toBe(true);
    expect(atLeast("member", "admin")).toBe(false);
    expect(atLeast(undefined, "member")).toBe(false);
  });
});
