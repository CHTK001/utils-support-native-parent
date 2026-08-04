import { chromium } from "file:///D:/ch/project/vue-support-parent-starter/node_modules/playwright/index.mjs";

const browser = await chromium.launch({ headless: true, executablePath: "C:\\Program Files\\Google\\Chrome Dev\\Application\\chrome.exe" });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await ctx.newPage();
const msgs = [];
page.on("console", (m) => msgs.push(`[${m.type()}] ${m.text()}`));
page.on("pageerror", (e) => msgs.push(`[pageerror] ${e.message}`));

await page.goto("http://localhost:8088/#/chat", { waitUntil: "networkidle", timeout: 30000 });
await page.waitForTimeout(2000);

const info = await page.evaluate(() => {
  const q = (s) => document.querySelector(s);
  const rect = (s) => {
    const el = q(s);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return { w: Math.round(r.width), h: Math.round(r.height), x: Math.round(r.x), y: Math.round(r.y), disp: cs.display, font: cs.fontSize, zoom: cs.zoom };
  };
  return {
    viewport: { w: window.innerWidth, h: window.innerHeight },
    body: rect("body"),
    app: rect("#app"),
    appLoader: (() => {
      const el = document.getElementById("app-loader") || document.querySelector(".sys-loader-shell, [id*=loader], [class*=loader]");
      return el ? rect(el) : null;
    })(),
    appShell: rect(".app-shell"),
    appHeader: rect(".app-header"),
    headerLeft: rect(".header-left"),
    headerCenter: rect(".header-center"),
    headerMenuItems: Array.from(document.querySelectorAll(".header-menu .el-menu-item")).map(x => ({ t: x.textContent.trim(), w: Math.round(x.getBoundingClientRect().width) })),
    appBody: rect(".app-body"),
    appAside: rect(".app-aside"),
    asideItems: Array.from(document.querySelectorAll(".side-menu .el-menu-item")).map(x => ({ t: x.textContent.trim(), w: Math.round(x.getBoundingClientRect().width), h: Math.round(x.getBoundingClientRect().height) })),
    appMain: rect(".app-main"),
    pageContainer: rect(".page-container"),
    reChat: rect(".re-chat-container"),
    chatHeader: rect(".chat-header"),
    chatMessages: rect(".chat-messages"),
    chatInput: rect(".chat-input"),
    htmlFontSize: getComputedStyle(document.documentElement).fontSize,
  };
});

console.log(JSON.stringify(info, null, 2));
console.log("\n=== console ===");
console.log(msgs.join("\n"));
await browser.close();
