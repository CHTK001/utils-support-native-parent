// 截图当前远程桌面画面
import { chromium } from 'playwright';

const agentRes = await fetch('http://127.0.0.1:3000/api/agents').then(r => r.json());
const agent = agentRes.find(a => a.id === 'my-desktop') || agentRes[0];
if (!agent) { console.log('NO AGENT'); process.exit(1); }

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
page.on('console', m => console.log('console:', m.text()));
page.on('pageerror', e => console.log('error:', e.message));

await page.goto('http://127.0.0.1:28849/');
await page.waitForLoadState('networkidle');

await page.fill('#targetId', agent.id);
await page.fill('#verifyCode', agent.verifyCode);
await page.click('#btnConnect');
await page.waitForSelector('#status.ok', { timeout: 30000 });
await page.waitForTimeout(3000);

await page.screenshot({ path: 'D:/ch/project/remote_screen.png', fullPage: false });

const status = await page.textContent('#status');
const fps = await page.textContent('#fps').catch(() => 'n/a');
console.log('status=', status, 'fps=', fps);
await browser.close();
