"""
Refined E2E - skip select, use input indices directly
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
        page.on('console', lambda msg: print(f'  {msg.type}: {msg.text[:200]}') if msg.type in ('error', 'warning') else None)
        page.on('pageerror', lambda err: print(f'  pageerror: {err}'))

        print('=== 1. Load SPA ===')
        await page.goto('http://127.0.0.1:7788/', timeout=30000)
        await page.wait_for_load_state('networkidle', timeout=15000)
        print(f'  url: {page.url}')

        print('\n=== 2. Wait for form ===')
        await page.wait_for_selector('input, .el-form, [class*="connection"]', timeout=15000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-1-loaded.png', full_page=True)
        print('  ✓ form visible')

        # Find protocol picker (el-select or similar)
        print('\n=== 3. Find protocol picker ===')
        el_selects = await page.query_selector_all('.el-select, .el-radio-group, [class*="protocol"]')
        print(f'  el-selects/protocol pickers: {len(el_selects)}')
        for i, sel in enumerate(el_selects):
            txt = await sel.inner_text()
            print(f'    [{i}] {txt[:80]!r}')

        # Try clicking first .el-select
        if el_selects:
            await el_selects[0].click()
            await page.wait_for_timeout(500)
            # try clicking SSH option
            opts = await page.query_selector_all('.el-select-dropdown__item, .el-option, [class*="option"]')
            for opt in opts:
                t = (await opt.inner_text()).strip()
                if t == 'SSH':
                    await opt.click()
                    print('  selected SSH')
                    break
            else:
                # click first option
                if opts:
                    await opts[0].click()
                    print(f'  selected first option: {await opts[0].inner_text()}')
        await page.wait_for_timeout(500)

        # Inspect current inputs
        print('\n=== 4. Fill credentials ===')
        inputs = await page.query_selector_all('input')
        print(f'  inputs: {len(inputs)}')
        for i, inp in enumerate(inputs):
            t = await inp.get_attribute('type') or 'text'
            ph = (await inp.get_attribute('placeholder') or '')[:30]
            vis = await inp.is_visible()
            print(f'    [{i}] type={t} placeholder={ph!r} visible={vis}')

        # index 0: key (skip)
        # index 1: empty (custom mode toggle?) - skip
        # index 2: host = 192.168.1.100
        # index 3: port (number)
        # index 4: user
        # index 5: password

        if len(inputs) >= 6:
            await inputs[2].fill('127.0.0.1')
            print('  filled host=127.0.0.1')
            await inputs[3].fill('22')
            print('  filled port=22')
            await inputs[4].fill('root')
            print('  filled user=root')
            await inputs[5].fill('rootpass123')
            print('  filled password=rootpass123')
        elif len(inputs) >= 4:
            # Fallback: find by placeholder
            for i, inp in enumerate(inputs):
                ph = (await inp.get_attribute('placeholder') or '').lower()
                if 'host' in ph or '192' in ph:
                    await inp.fill('127.0.0.1')
                elif 'port' in ph or '22' in ph:
                    await inp.fill('22')
                elif 'admin' in ph or 'root' in ph or 'user' in ph:
                    await inp.fill('root')
                elif await inp.get_attribute('type') == 'password':
                    await inp.fill('rootpass123')
        await page.wait_for_timeout(500)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-2-filled.png', full_page=True)

        print('\n=== 5. Click connect ===')
        clicked = False
        for b in await page.query_selector_all('button'):
            txt = (await b.inner_text()).lower()
            if 'connect' in txt or '连接' in txt or 'login' in txt or '登录' in txt:
                await b.click()
                print(f'  clicked: {txt!r}')
                clicked = True
                break
        if not clicked:
            print('  ⚠ no connect button')

        print('\n=== 6. Wait for terminal ===')
        try:
            await page.wait_for_selector('.xterm, .xterm-screen, .xterm-rows, canvas', timeout=30000)
            print('  ✓ terminal visible')
        except Exception as e:
            print(f'  no terminal: {e}')

        await page.wait_for_timeout(3000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-3-terminal.png', full_page=True)

        # Check alerts
        alerts = await page.query_selector_all('.el-message, .el-notification, [role="alert"]')
        for a in alerts:
            try:
                print(f'  alert: {await a.inner_text()[:200]}')
            except: pass

        # Try typing
        print('\n=== 7. Send ls ===')
        try:
            await page.keyboard.type('ls\n', delay=100)
            await page.wait_for_timeout(3000)
            await page.screenshot(path=r'D:\ch\project\e2e-results\step-4-ls.png', full_page=True)
        except Exception as e:
            print(f'  type failed: {e}')

        # Check xterm content
        xterm = await page.query_selector('.xterm-rows')
        if xterm:
            txt = await xterm.inner_text()
            print(f'  xterm text: {txt[:200]!r}')

        await browser.close()
        print('\n=== DONE ===')

asyncio.run(run())
