import asyncio
from playwright.async_api import async_playwright
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=['--no-sandbox'])
        ctx = await browser.new_context(viewport={'width': 1600, 'height': 1000})
        page = await ctx.new_page()
        bodies = []
        page.on('request', lambda r: bodies.append(r.post_data) if '/api/connections/authenticate' in r.url else None)
        await page.goto('http://127.0.0.1:7788/', wait_until='networkidle', timeout=30000)
        await page.wait_for_timeout(3000)
        await page.locator('text=自定义模式').first.click()
        await page.wait_for_timeout(800)
        sel = page.locator('.el-select').first
        await sel.click()
        await page.wait_for_timeout(500)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(500)

        # All visible inputs
        inputs = page.locator('input:visible')
        n = await inputs.count()
        print(f'Visible inputs: {n}')
        for i in range(n):
            el = inputs.nth(i)
            ph = await el.get_attribute('placeholder')
            tp = await el.get_attribute('type')
            v = await el.input_value()
            print(f'  [{i}] type={tp} ph={ph!r} value={v!r}')

        # Fill: 0=select (skip), 1=host, 2=port, 3=user, 4=password
        print('\n=== Filling ===')
        await inputs.nth(1).fill('127.0.0.1')
        await inputs.nth(2).fill('22')
        await inputs.nth(3).fill('root')
        await inputs.nth(4).fill('rootpass123')
        await page.wait_for_timeout(500)

        # Recheck
        for i in range(await page.locator('input:visible').count()):
            el = page.locator('input:visible').nth(i)
            ph = await el.get_attribute('placeholder')
            v = await el.input_value()
            print(f'  AFTER [{i}] ph={ph!r} value={v!r}')

        btns = page.locator('button.cf-connect-btn')
        await btns.nth(1).click(force=True, timeout=5000)
        await page.wait_for_timeout(10000)

        print('\n=== Bodies sent ===')
        for b in bodies:
            print(' ', b)
        body = await page.evaluate('() => document.body.innerText')
        print('\n=== Body after click ===')
        print(body[:1500])
        # Check xterm
        xterm = await page.evaluate('''() => ({
            canvas: !!document.querySelector('canvas'),
            xterm: !!document.querySelector('.xterm'),
            rc_status: !!document.querySelector('.rc-status'),
            alert_err: !!document.querySelector('.el-alert--error'),
        })''')
        print('xterm:', xterm)
        await browser.close()
asyncio.run(main())
