"""
Diagnose API call from browser
"""
import sys, os
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import asyncio
from playwright.async_api import async_playwright

async def run():
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True, args=['--no-sandbox'])
        page = await browser.new_page()
        # capture all network
        page.on('request', lambda r: print(f'  REQ {r.method} {r.url[:120]}'))
        page.on('response', lambda r: print(f'  RES {r.status} {r.url[:120]}'))
        page.on('console', lambda msg: print(f'  console.{msg.type}: {msg.text[:200]}') if msg.type in ('error', 'warning', 'log') else None)
        page.on('pageerror', lambda err: print(f'  pageerror: {err}'))

        await page.goto('http://127.0.0.1:7788/', timeout=30000)
        await page.wait_for_selector('input', timeout=15000)
        await page.wait_for_timeout(2000)

        # Click first .el-select
        sel = await page.query_selector('.el-select')
        if sel:
            await sel.click()
            await page.wait_for_timeout(500)
        # Pick SSH
        for opt in await page.query_selector_all('.el-select-dropdown__item'):
            if 'SSH' in (await opt.inner_text()):
                await opt.click()
                break
        await page.wait_for_timeout(500)

        # Fill host/port/user/password (assume indices 2-5)
        inputs = await page.query_selector_all('input')
        print(f'\n  inputs count: {len(inputs)}')
        for i, inp in enumerate(inputs):
            ph = (await inp.get_attribute('placeholder') or '')
            t = await inp.get_attribute('type') or 'text'
            val = await inp.get_attribute('value') or ''
            print(f'  [{i}] t={t} ph={ph!r} val={val!r}')
        # Fill
        await inputs[2].fill('127.0.0.1')
        await inputs[3].fill('22')
        await inputs[4].fill('root')
        await inputs[5].fill('rootpass123')
        await page.wait_for_timeout(500)

        # Click connect
        for b in await page.query_selector_all('button'):
            if await b.is_visible() and '连接' in await b.inner_text():
                print('  clicking 连接')
                await b.click(force=True)
                break

        # Wait
        await page.wait_for_timeout(8000)
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-after-connect.png', full_page=True)

        await browser.close()

asyncio.run(run())
