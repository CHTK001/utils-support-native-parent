import asyncio
from playwright.async_api import async_playwright
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'

with open(r'D:\ch\project\test_id_ed25519', 'r') as f:
    KEY = f.read()

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=['--no-sandbox'])
        ctx = await browser.new_context(viewport={'width': 1600, 'height': 1000})
        page = await ctx.new_page()
        bodies = []
        page.on('request', lambda r: bodies.append((r.method, r.url, r.post_data)) if '/api/connections/authenticate' in r.url else None)

        await page.goto('http://127.0.0.1:7788/', wait_until='networkidle', timeout=30000)
        await page.wait_for_timeout(5000)
        await page.locator('text=自定义模式').first.click()
        await page.wait_for_timeout(1000)
        sel = page.locator('.el-select').first
        await sel.click()
        await page.wait_for_timeout(800)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(800)

        inputs = page.locator('input:visible')
        n = await inputs.count()
        print(f'visible: {n}')

        # Clear and fill carefully
        for i, (val, desc) in enumerate([('127.0.0.1', 'host'), ('22', 'port'), ('root', 'user')]):
            await inputs.nth(i+1).fill('')
            await inputs.nth(i+1).fill(val)
            print(f'filled {desc} = {val}')
        # password = key
        await inputs.nth(4).fill(KEY)
        print(f'filled password (key, {len(KEY)} bytes)')
        await page.wait_for_timeout(500)

        for i in range(n):
            el = inputs.nth(i)
            ph = await el.get_attribute('placeholder')
            v = await el.input_value()
            print(f'  [{i}] ph={ph!r} value={v[:60]!r}...')

        btns = page.locator('button.cf-connect-btn')
        await btns.nth(1).click(force=True, timeout=5000)
        await page.wait_for_timeout(15000)
        print('=== Bodies ===')
        for m, u, b in bodies:
            print(' ', b[:300])
        xterm = await page.evaluate('''() => ({
            canvas: !!document.querySelector('canvas'),
            xterm: !!document.querySelector('.xterm'),
            xterm_screen: !!document.querySelector('.xterm-screen'),
            rc_status: !!document.querySelector('.rc-status'),
            alert_err: !!document.querySelector('.el-alert--error'),
        })''')
        print('xterm:', xterm)
        body = await page.evaluate('() => document.body.innerText')
        print('Body:', body[:1500])
        await browser.close()
asyncio.run(main())
