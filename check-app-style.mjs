import { chromium } from "playwright";

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

const errors = [];
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(`console.error: ${m.text()}`);
});

await page.goto("http://localhost:8088/", { waitUntil: "networkidle" });
await page.waitForTimeout(800);

const info = await page.evaluate(() => {
  const app = document.getElementById("app");
  const cs = app ? getComputedStyle(app) : null;
  const loader = document.getElementById("app-loader");
  return {
    appExists: !!app,
    appRect: app ? { w: app.clientWidth, h: app.clientHeight } : null,
    appDisplay: cs ? cs.display : null,
    appAlign: cs ? cs.alignItems : null,
    appJustify: cs ? cs.justifyContent : null,
    appMargin: cs ? cs.margin : null,
    appPadding: cs ? cs.padding : null,
    bodyRect: { w: document.body.clientWidth, h: document.body.clientHeight },
    htmlRect: { w: document.documentElement.clientWidth, h: document.documentElement.clientHeight },
    loaderPresent: !!loader,
    elContainer: !!document.querySelector(".el-container"),
    elHeader: !!document.querySelector(".el-header"),
    elAside: !!document.querySelector(".el-aside"),
    elMain: !!document.querySelector(".el-main"),
    appChildren: app ? Array.from(app.children).map((c) => `${c.tagName}.${c.className}`) : [],
  };
});

console.log(JSON.stringify(info, null, 2));
console.log("ERRORS:", errors.length ? errors.join("\n") : "none");

await browser.close();
