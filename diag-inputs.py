import asyncio
from playwright.async_api import async_playwright
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=['--no-sandbox'])
        ctx = await browser.new_context(viewport={'width': 1600, 'height': 1000})
        page = await ctx.new_page()
        await page.goto('http://127.0.0.1:7788/', wait_until='networkidle', timeout=30000)
        await page.wait_for_timeout(3000)
        await page.locator('text=自定义模式').first.click()
        await page.wait_for_timeout(800)
        sel = page.locator('.el-select').first
        await sel.click()
        await page.wait_for_timeout(500)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(500)
        inputs = page.locator('input:visible')
        n = await inputs.count()
        print(f'visible: {n}')
        for i in range(n):
            el = inputs.nth(i)
            ph = await el.get_attribute('placeholder')
            v = await el.input_value()
            print(f'  [{i}] ph={ph!r} value={v!r}')
        await browser.close()
asyncio.run(main())
