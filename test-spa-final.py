"""
Full E2E: connect button is 2nd one (custom mode), click it, verify API call
"""
import asyncio, os
from playwright.async_api import async_playwright

OUT = r"D:\ch\project\e2e-results"
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=["--no-sandbox"])
        ctx = await browser.new_context(viewport={"width": 1440, "height": 900})
        page = await ctx.new_page()

        api_calls = []
        page.on("response", lambda r: api_calls.append((r.status, r.url, r.request.method)) if "/api/" in r.url else None)
        page.on("request", lambda r: api_calls.append(("REQ", r.method, r.url)) if "/api/" in r.url else None)
        page.on("console", lambda m: print(f"[{m.type}] {m.text[:200]}"))

        await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)

        # Click 自定义模式
        await page.locator("text=自定义模式").first.click()
        await page.wait_for_timeout(800)

        # Pick VNC
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        vnc = page.locator(".el-select-dropdown__item:has-text('VNC')").first
        await vnc.click()
        await page.wait_for_timeout(500)

        # Fill
        await page.locator("input[placeholder='192.168.1.100']").first.fill("172.16.0.40")
        inputs = page.locator("input")
        await inputs.nth(2).fill("5900")
        await page.locator("input[placeholder='admin / root']").first.fill("admin")
        await page.locator("input[type='password']").first.fill("test")
        await page.wait_for_timeout(500)

        # Click SECOND connect button (custom mode)
        print("=== Click custom mode connect ===")
        btns = page.locator("button.cf-connect-btn")
        n = await btns.count()
        print(f"Connect buttons count: {n}")
        for i in range(n):
            dis = await btns.nth(i).get_attribute("disabled")
            vis = await btns.nth(i).is_visible()
            print(f"  [{i}] vis={vis} disabled={dis!r}")
        # click second (custom mode)
        await btns.nth(1).click(timeout=5000, force=True)
        print("OK clicked custom connect")
        await page.wait_for_timeout(5000)

        # Check body
        body = await page.evaluate("() => document.body.innerText")
        print(f"\n=== Body after click ===\n{body[:2000]}")

        # API calls made
        print(f"\n=== API calls ({len(api_calls)}) ===")
        for c in api_calls:
            print(f"  {c}")

        # Check if viewer appeared
        has_viewer = await page.evaluate("""() => {
            return {
                guac: !!document.querySelector('.guac-client-container, .guac-screen, canvas'),
                ssh: !!document.querySelector('canvas, [class*="terminal"], [class*="xterm"]'),
                rdp: !!document.querySelector('[class*="rdp"], [class*="RDP"]'),
                status: !!document.querySelector('.rc-status'),
                error: !!document.querySelector('.el-alert--error, .el-alert'),
            };
        }""")
        print(f"\nViewer detection: {has_viewer}")

        await page.screenshot(path=os.path.join(OUT, "step-after-connect.png"), full_page=True)

        # Try SSH (172.16.0.40:22, won't work without real creds, but tests flow)
        print("\n=== Switch to SSH and try real auth ===")
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        ssh = page.locator(".el-select-dropdown__item:has-text('SSH')").first
        await ssh.click()
        await page.wait_for_timeout(500)

        # disconnect if there's a disconnect button
        disconn = page.locator("button:has-text('断开')").first
        if await disconn.count() > 0:
            try: await disconn.click(timeout=2000)
            except: pass
        await page.wait_for_timeout(500)

        await page.locator("input[placeholder='192.168.1.100']").first.fill("172.16.0.40")
        inputs = page.locator("input")
        await inputs.nth(2).fill("22")
        await page.locator("input[placeholder='admin / root']").first.fill("root")
        await page.locator("input[type='password']").first.fill("wrong_password_to_test_failure")
        await page.wait_for_timeout(500)

        btns = page.locator("button.cf-connect-btn")
        await btns.nth(1).click(timeout=5000, force=True)
        await page.wait_for_timeout(5000)

        body = await page.evaluate("() => document.body.innerText")
        print(f"\n=== Body after SSH auth ===\n{body[:2000]}")

        print(f"\n=== API calls after SSH ({len(api_calls)}) ===")
        for c in api_calls[-10:]:
            print(f"  {c}")

        await page.screenshot(path=os.path.join(OUT, "step-ssh-auth.png"), full_page=True)
        await browser.close()
        print("\nDONE")

asyncio.run(main())
