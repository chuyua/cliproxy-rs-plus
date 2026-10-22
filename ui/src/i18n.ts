/**
 * Dashboard presentation only. Never translate API paths, OAuth parameters,
 * provider IDs, credentials, configuration keys or values, or request payloads.
 */
export type Language = "zh-CN" | "en";

const zh: Record<string, string> = {
  "Language": "语言",
  "Overview": "总览",
  "Use with tools": "客户端接入",
  "Credentials": "账号凭据",
  "Provider keys": "供应商密钥",
  "Client keys": "客户端密钥",
  "Models": "模型",
  "Payload rules": "请求规则",
  "Quotas": "配额",
  "Usage": "使用统计",
  "Logs": "日志",
  "Configuration": "配置",
  "Plugins": "插件",
  "System": "系统",
  "Connect an account": "连接账号",
  "Sign in": "登录",
  "Sign out": "退出登录",
  "Discard unsaved changes?": "放弃未保存的修改？",
  "Management key": "管理密钥",
  "Connecting…": "正在连接…",
  "Connect": "连接",
  "Connection lost": "连接已断开",
  "Connected to": "已连接至",
  "Pages": "页面",
  "Use light theme": "使用浅色主题",
  "Use dark theme": "使用深色主题",
  "Dismiss": "关闭",
  "Use management.secret-key from the server’s config.yaml. It stays in this tab’s memory only; reloading signs you out.": "请使用服务器 config.yaml 中的 management.secret-key。密钥仅保存在当前标签页内存中，刷新页面需要重新登录。",
  "Sign in with a provider": "使用供应商账号登录",
  "You approve access on the provider’s own page (OAuth). The proxy stores the resulting token and renews it; it never sees your password.": "请在供应商的官方授权页面确认 OAuth 授权。代理仅保存并续期令牌，不会获取你的密码。",
  "provider sign-in": "供应商登录",
  "Vertex import": "Vertex 导入",
  "sign-in is not available on this server.": "此服务器尚不支持该账号登录。",
  "The server returned an unsafe sign-in URL.": "服务器返回的登录链接不安全。",
  "Sign-in not cancelled:": "取消登录失败：",
  "account connected.": "账号已连接。",
  "Vertex import is not available on this server.": "此服务器尚不支持导入 Vertex。",
  "Vertex service account imported.": "Vertex 服务账号已导入。",
  "Waiting for approval": "等待授权",
  "Connected": "已连接",
  "Failed": "失败",
  "Cancelled": "已取消",
  "Open the provider’s page and approve access": "打开供应商授权页面并确认授权",
  "with this code": "，并使用此代码",
  "Open sign-in page": "打开登录页面",
  "Copy link": "复制链接",
  "Link copied.": "链接已复制。",
  "If the browser ends on a page that fails to load, paste its full address here.": "如果浏览器最终跳转到无法访问的 localhost 页面，请复制完整地址并粘贴到这里。无需在服务器安装浏览器。",
  "Callback address": "回调地址",
  "Send": "发送",
  "Callback sent. Finishing sign-in…": "回调已发送，正在完成登录…",
  "Cancel sign-in": "取消登录",
  "Saved on the server and in rotation.": "账号凭据已保存到服务器并加入轮换。",
  "View credentials": "查看凭据",
  "Start again": "重新登录",
  "Other ways": "其他方式",
  "Use a Google Cloud service-account JSON for Vertex AI.": "为 Vertex AI 使用 Google Cloud 服务账号 JSON 文件。",
  "Import service account": "导入服务账号",
  "Use an API key instead of an account.": "也可以使用 API 密钥，而不是账号登录。",
  "Providers": "供应商",
};

export function translate(text: string, lang: Language): string {
  return lang === "zh-CN" ? zh[text] ?? text : text;
}

export function initialLanguage(storage?: Pick<Storage, "getItem">): Language {
  try {
    return (storage ?? globalThis.localStorage)?.getItem("cliproxy-language") === "en" ? "en" : "zh-CN";
  } catch {
    return "zh-CN";
  }
}
