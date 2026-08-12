"""
Full Playwright E2E test of SSH connection
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

        page.on('console', lambda msg: print(f'  console.{msg.type}: {msg.text}'))
        page.on('pageerror', lambda err: print(f'  pageerror: {err}'))

        print('=== 1. Load SPA ===')
        await page.goto('http://127.0.0.1:7788/', timeout=30000)
        await page.wait_for_load_state('networkidle', timeout=15000)
        print(f'  url: {page.url}')

        print('\n=== 2. Wait for connection form ===')
        await page.wait_for_selector('select, input, .el-form, .ant-form, [class*="connection"]', timeout=15000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-1-loaded.png', full_page=True)
        print('  ✓ form visible')

        # Find select
        selects = await page.query_selector_all('select')
        print(f'  selects: {len(selects)}')
        for i, sel in enumerate(selects):
            opts = await sel.query_selector_all('option')
            print(f'    [{i}] {[await o.inner_text() for o in opts]}')

        if selects:
            try:
                await selects[0].select_option(label='SSH')
                print('  selected SSH')
            except Exception as e:
                print(f'  label fail: {e}')
                try: await selects[0].select_option(value='SSH')
                except Exception as e2: print(f'  value fail: {e2}')

        await page.wait_for_timeout(500)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-2-ssh.png', full_page=True)

        print('\n=== 3. Fill credentials ===')
        inputs = await page.query_selector_all('input')
        print(f'  inputs: {len(inputs)}')
        for i, inp in enumerate(inputs):
            t = await inp.get_attribute('type') or 'text'
            ph = (await inp.get_attribute('placeholder') or '')[:30]
            print(f'    [{i}] type={t} placeholder={ph!r}')

        # Fill by index heuristic (after first select)
        # Try to find host input
        host_inp = None
        for inp in inputs:
            ph = (await inp.get_attribute('placeholder') or '').lower()
            if 'host' in ph or 'ip' in ph or '主机' in ph:
                host_inp = inp
                break
        if host_inp:
            await host_inp.fill('127.0.0.1')
        elif len(inputs) >= 1:
            await inputs[0].fill('127.0.0.1')

        # Port
        port_inp = None
        for inp in inputs:
            ph = (await inp.get_attribute('placeholder') or '').lower()
            if 'port' in ph or '端口' in ph:
                port_inp = inp
                break
        if port_inp:
            await port_inp.fill('22')

        # User
        user_inp = None
        for inp in inputs:
            ph = (await inp.get_attribute('placeholder') or '').lower()
            if 'user' in ph or 'username' in ph or '用户' in ph:
                user_inp = inp
                break
        if user_inp:
            await user_inp.fill('root')

        # Password
        for inp in inputs:
            t = await inp.get_attribute('type') or 'text'
            if t == 'password':
                await inp.fill('rootpass123')
                break

        await page.wait_for_timeout(500)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-3-filled.png', full_page=True)

        print('\n=== 4. Click connect ===')
        clicked = False
        for b in await page.query_selector_all('button'):
            txt = (await b.inner_text()).lower()
            if 'connect' in txt or '连接' in txt:
                await b.click()
                print(f'  clicked: {txt!r}')
                clicked = True
                break
        if not clicked:
            for b in await page.query_selector_all('button'):
                txt = (await b.inner_text()).lower()
                if 'login' in txt or '登录' in txt:
                    await b.click()
                    print(f'  clicked: {txt!r}')
                    clicked = True
                    break
        if not clicked:
            print('  no connect button found')

        print('\n=== 5. Wait for terminal (max 30s) ===')
        try:
            await page.wait_for_selector('.xterm, .xterm-screen, .xterm-rows, canvas, [class*="terminal"]', timeout=30000)
            print('  ✓ terminal visible')
        except Exception as e:
            print(f'  no terminal: {e}')

        await page.wait_for_timeout(3000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-4-terminal.png', full_page=True)

        print('\n=== 6. Check alert ===')
        alerts = await page.query_selector_all('.el-message--error, .ant-message-error, [role="alert"]')
        for a in alerts:
            print(f'  alert: {await a.inner_text()}')

        # Try typing
        print('\n=== 7. Send ls command ===')
        try:
            await page.keyboard.press('Tab')
            await page.keyboard.type('ls\n', delay=50)
            await page.wait_for_timeout(2000)
            await page.screenshot(path=r'D:\ch\project\e2e-results\step-5-ls.png', full_page=True)
        except Exception as e:
            print(f'  type failed: {e}')

        await browser.close()
        print('\n=== DONE ===')

asyncio.run(run())
