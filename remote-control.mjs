// 远程桌面测试 - 使用 vue-support-remote-starter 前端
import { chromium } from 'D:/ch/project/vue-support-parent-starter/node_modules/playwright/index.js';

const agentRes = await fetch('http://127.0.0.1:3000/api/agents').then(r => r.json());
const agent = agentRes.find(a => a.ipAddress === '127.0.0.1') || agentRes[0];
if (!agent) { console.log('NO AGENT'); process.exit(1); }
console.log('Using agent:', agent.agentId, 'verifyCode=', agent.verifyCode);

const browser = await chromium.launch({ channel: 'msedge' });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
page.on('console', m => console.log('console:', m.text()));
page.on('pageerror', e => console.log('error:', e.message));

await page.goto('http://127.0.0.1:28849/');
await page.waitForLoadState('networkidle');
await page.waitForTimeout(3000);

console.log('Page title:', await page.title());
console.log('Inputs found:', await page.evaluate(() => {
  return Array.from(document.querySelectorAll('input,select,button')).map(el => ({
    id: el.id, name: el.name, tag: el.tagName, placeholder: el.placeholder,
    text: el.textContent?.trim().slice(0, 30)
  }));
}));

await page.fill('#inputTargetId', agent.agentId);
await page.fill('#inputVerifyCode', agent.verifyCode);
await page.click('#btnConnect');
await page.waitForFunction(() => document.getElementById('status')?.textContent === '已连接', { timeout: 30000 });
await page.waitForTimeout(5000);

await page.screenshot({ path: 'D:/ch/project/remote_screen.png', fullPage: false });

const status = await page.textContent('#status');
const fps = await page.textContent('#fps').catch(() => 'n/a');
console.log('status=', status, 'fps=', fps);
await browser.close();
console.log('DONE');
