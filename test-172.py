"""
E2E: try 172.16.0.40:22 via Playwright (any common password)
"""
import asyncio
from playwright.async_api import async_playwright
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'

async def try_one(page, host, port, user, pwd, label):
    print(f"\n=== {label} ===")
    # Reset form: clear and refill
    inputs = page.locator('input:visible')
    await inputs.nth(1).fill(host)
    await inputs.nth(2).fill(str(port))
    await inputs.nth(3).fill(user)
    await inputs.nth(4).fill(pwd)
    await page.wait_for_timeout(300)
    btns = page.locator('button.cf-connect-btn')
    await btns.nth(1).click(force=True, timeout=5000)
    await page.wait_for_timeout(8000)
    xterm = await page.evaluate('''() => ({
        canvas: !!document.querySelector('canvas'),
        xterm: !!document.querySelector('.xterm'),
        xterm_screen: !!document.querySelector('.xterm-screen'),
        rc_status: !!document.querySelector('.rc-status'),
        alert_err: !!document.querySelector('.el-alert--error'),
    })''')
    print(f"  xterm: {xterm}")
    return xterm

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

        # Try various combinations for 172.16.0.40
        attempts = [
            ("172.16.0.40", 22, "root", "root"),
            ("172.16.0.40", 22, "root", "toor"),
            ("172.16.0.40", 22, "root", "123456"),
            ("172.16.0.40", 22, "admin", "admin"),
        ]
        for host, port, user, pwd in attempts:
            xterm = await try_one(page, host, port, user, pwd, f"{user}@{host}:{port} pass={pwd}")
            if xterm.get('xterm') or xterm.get('rc_status'):
                print(f"  🎉 SUCCESS with {user}:{pwd}")
                # Try whoami
                if xterm.get('xterm'):
                    await page.keyboard.type('whoami')
                    await page.keyboard.press('Enter')
                    await page.wait_for_timeout(3000)
                    body2 = await page.evaluate('() => document.body.innerText')
                    print(f"  After whoami:\n{body2[:500]}")
                await page.screenshot(path=r'D:\ch\project\e2e-results\step-ssh-172.png', full_page=True)
                break

        await browser.close()

asyncio.run(main())
