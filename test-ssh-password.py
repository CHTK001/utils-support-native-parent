import asyncio
from playwright.async_api import async_playwright
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=['--no-sandbox'])
        ctx = await browser.new_context(viewport={'width': 1600, 'height': 1000})
        page = await ctx.new_page()
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
        await inputs.nth(1).fill('127.0.0.1')
        await inputs.nth(2).fill('22')
        await inputs.nth(3).fill('root')
        await inputs.nth(4).fill('rootpass123')
        await page.wait_for_timeout(500)
        btns = page.locator('button.cf-connect-btn')
        await btns.nth(1).click(force=True, timeout=5000)
        await page.wait_for_timeout(15000)
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
        if xterm.get('xterm') or xterm.get('xterm_screen'):
            print('\n=== Type "whoami" ===')
            await page.keyboard.type('whoami')
            await page.keyboard.press('Enter')
            await page.wait_for_timeout(5000)
            body2 = await page.evaluate('() => document.body.innerText')
            print('After whoami:', body2[:1500])
        await page.screenshot(path=r'D:\ch\project\e2e-results\step-ssh-PASSWORD.png', full_page=True)
        await browser.close()
asyncio.run(main())
