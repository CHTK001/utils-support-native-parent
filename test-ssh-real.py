"""
Playwright E2E with REAL SSH connection:
- Frontend at 127.0.0.1:7788
- Backend at 172.16.0.40:18090 (gateway-server container)
- SSH target at 172.16.0.40:22 (sandbox can reach directly)

Flow:
1. Load SPA, select SSH custom mode
2. Fill host=172.16.0.40 port=22 user=root pass=...
3. Click connect
4. Wait for tunnelId + wsUrl
5. Verify ReSshViewer (xterm canvas) rendered
"""
import asyncio, os
from playwright.async_api import async_playwright

OUT = r"D:\ch\project\e2e-results"
os.makedirs(OUT, exist_ok=True)
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(
            headless=True, executable_path=EDGE, args=["--no-sandbox"]
        )
        ctx = await browser.new_context(viewport={"width": 1600, "height": 1000})
        page = await ctx.new_page()

        api_calls = []
        ws_connections = []

        page.on("request", lambda r: api_calls.append(("REQ", r.method, r.url[:120])) if "/api/" in r.url or "/ws/" in r.url else None)
        page.on("response", lambda r: api_calls.append(("RES", r.status, r.url[:120])) if "/api/" in r.url or "/ws/" in r.url else None)
        page.on("websocket", lambda ws: ws_connections.append(ws))
        page.on("console", lambda m: print(f"[{m.type}] {m.text[:150]}"))

        print("=== 1. Load SPA ===")
        await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)

        # Custom mode
        print("\n=== 2. Switch to custom mode ===")
        await page.locator("text=自定义模式").first.click()
        await page.wait_for_timeout(800)

        # Pick SSH
        print("\n=== 3. Select SSH ===")
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        ssh = page.locator(".el-select-dropdown__item:has-text('SSH')").first
        await ssh.click()
        await page.wait_for_timeout(500)
        print("OK SSH selected")

        # Fill
        print("\n=== 4. Fill SSH credentials ===")
        await page.locator("input[placeholder='192.168.1.100']").first.fill("172.16.0.40")
        await page.locator("input").nth(2).fill("22")
        await page.locator("input[placeholder='admin / root']").first.fill("tester")
        # Need a real password - try default tester
        pass_in = page.locator("input[type='password']").first
        await pass_in.fill("testerpass")
        await page.wait_for_timeout(500)

        await page.screenshot(path=os.path.join(OUT, "step-ssh-form.png"), full_page=True)

        # Click custom connect (2nd button)
        print("\n=== 5. Click '连接' ===")
        btns = page.locator("button.cf-connect-btn")
        await btns.nth(1).click(timeout=5000, force=True)
        print("OK clicked")

        # Wait for response
        await page.wait_for_timeout(8000)

        # Check status
        body = await page.evaluate("() => document.body.innerText")
        print(f"\n=== Body after connect ===\n{body[:1500]}")

        # Check for ws/tunnel/error in body
        has_tunnel = "tunnel" in body.lower() or "ws://" in body
        has_error = "失败" in body or "错误" in body or "refused" in body.lower()
        has_xterm = await page.evaluate("""() => {
            return {
                canvas: !!document.querySelector('canvas'),
                xterm: !!document.querySelector('.xterm, [class*="xterm"]'),
                terminal: !!document.querySelector('[class*="terminal"]'),
                rc_status: !!document.querySelector('.rc-status'),
                el_alert: document.querySelectorAll('.el-alert').length,
            };
        }""")
        print(f"\nViewer detection: {has_xterm}")
        print(f"Has tunnel info: {has_tunnel}")
        print(f"Has error: {has_error}")

        print(f"\n=== API calls ({len(api_calls)}) ===")
        for c in api_calls[-15:]:
            print(f"  {c}")

        print(f"\n=== WebSocket connections ({len(ws_connections)}) ===")
        for w in ws_connections:
            print(f"  WS: {w.url}")

        await page.screenshot(path=os.path.join(OUT, "step-ssh-result.png"), full_page=True)

        # If error, try with valid creds (need to check what user is on 172.16.0.40)
        if has_error:
            print("\n=== Auth failed. Trying without password (key auth) ===")
            # Clear password
            await pass_in.fill("")
            await page.wait_for_timeout(300)
            await btns.nth(1).click(timeout=5000, force=True)
            await page.wait_for_timeout(8000)
            body = await page.evaluate("() => document.body.innerText")
            print(f"After no-pass attempt:\n{body[:1000]}")

        await browser.close()
        print("\nDONE")

asyncio.run(main())
