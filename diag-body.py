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
        await page.wait_for_timeout(500)
        sel = page.locator('.el-select').first
        await sel.click()
        await page.wait_for_timeout(300)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(500)
        # Just click connect without filling
        btns = page.locator('button.cf-connect-btn')
        await btns.nth(1).click(force=True, timeout=5000)
        await page.wait_for_timeout(5000)
        print('=== Bodies sent (no fill) ===')
        for b in bodies:
            print(b)
        await browser.close()
asyncio.run(main())
