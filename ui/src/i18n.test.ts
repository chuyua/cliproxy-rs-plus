import assert from "node:assert/strict";
import test from "node:test";
import { initialLanguage, translate } from "./i18n.ts";

test("language selection is independent of protocol names", () => {
  assert.equal(translate("Connect an account", "zh-CN"), "连接账号");
  assert.equal(translate("Connect an account", "en"), "Connect an account");
  for (const value of ["provider=antigravity", "refresh_token", "/oauth/callback", "x-cpa-version"]) {
    assert.equal(translate(value, "zh-CN"), value);
  }
});

test("language setting is optional, defaults to Chinese, and can select English", () => {
  assert.equal(initialLanguage(), "zh-CN");
  assert.equal(initialLanguage({ getItem: () => "en" }), "en");
  assert.equal(initialLanguage({ getItem: () => "zh-CN" }), "zh-CN");
  assert.equal(initialLanguage({ getItem: () => { throw new Error("storage denied"); } }), "zh-CN");
});
