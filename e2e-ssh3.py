"""
Force-click connect
"""
import sys, os
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import asyncio
from playwright.async_api import async_playwright

os.makedirs(r'D:\ch\project\e2e-results', exist_ok=True)

async def run():
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True, args=['--no-sandbox'])
        page = await browser.new_page()
        page.on('pageerror', lambda err: print(f'  pageerror: {err}'))

        await page.goto('http://127.0.0.1:7788/', timeout=30000)
        await page.wait_for_selector('input, .el-form, [class*="connection"]', timeout=15000)
        await page.wait_for_timeout(1000)

        # Find all visible buttons
        print('=== buttons ===')
        btns = await page.query_selector_all('button')
        for i, b in enumerate(btns):
            vis = await b.is_visible()
            txt = (await b.inner_text()).strip()[:30]
            print(f'  [{i}] vis={vis} text={txt!r}')

        # Pick first visible button
        visible_btn = None
        for b in btns:
            if await b.is_visible():
                visible_btn = b
                break

        if not visible_btn:
            # try force click on any
            for b in btns:
                txt = (await b.inner_text()).strip().lower()
                if 'connect' in txt or '连接' in txt or 'login' in txt:
                    print(f'  force-click: {txt!r}')
                    await b.click(force=True, timeout=5000)
                    break
        else:
            txt = (await visible_btn.inner_text()).strip()
            print(f'  click visible: {txt!r}')
            await visible_btn.click(force=True)

        print('\n=== wait terminal ===')
        try:
            await page.wait_for_selector('.xterm, .xterm-screen, .xterm-rows, canvas', timeout=20000)
            print('  ✓ terminal')
        except Exception as e:
            print(f'  no terminal: {e}')

        await page.wait_for_timeout(2000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-3-terminal.png', full_page=True)

        # alerts
        alerts = await page.query_selector_all('.el-message, .el-notification, [role="alert"]')
        for a in alerts:
            try: print(f'  alert: {await a.inner_text()[:200]}')
            except: pass

        await page.keyboard.type('id\n', delay=100)
        await page.wait_for_timeout(2000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-4-id.png', full_page=True)

        xterm = await page.query_selector('.xterm-rows')
        if xterm:
            print(f'  xterm: {(await xterm.inner_text())[:200]!r}')

        await browser.close()

asyncio.run(run())
