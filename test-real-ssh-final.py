"""
Full E2E SSH flow now that container has SSH server
- Frontend at 127.0.0.1:7788
- Backend at 172.16.0.40:18090
- Container now has sshd on 127.0.0.1:22
- Frontend sends host=127.0.0.1 port=22 user=root pass=rootpass123
- Backend's SshBridge (in container) connects to 127.0.0.1:22 (itself) - WORKS
- WS endpoint receives SSH shell bytes
- ReSshViewer (xterm) renders terminal
"""
import asyncio, os
from playwright.async_api import async_playwright

OUT = r"D:\ch\project\e2e-results"
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=["--no-sandbox"])
        ctx = await browser.new_context(viewport={"width": 1600, "height": 1000})
        page = await ctx.new_page()

        api_calls = []
        page.on("request", lambda r: api_calls.append(("REQ", r.method, r.url[:120])) if "/api/" in r.url or "/ws/" in r.url else None)
        page.on("response", lambda r: api_calls.append(("RES", r.status, r.url[:120])) if "/api/" in r.url or "/ws/" in r.url else None)
        page.on("console", lambda m: print(f"[{m.type}] {m.text[:200]}"))
        page.on("websocket", lambda ws: print(f"[WS-OPEN] {ws.url}"))

        await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)
        await page.locator("text=自定义模式").first.click()
        await page.wait_for_timeout(800)
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(500)

        # Fill: 127.0.0.1:22 root/rootpass123
        await page.locator("input[placeholder='192.168.1.100']").first.fill("127.0.0.1")
        await page.locator("input").nth(2).fill("22")
        await page.locator("input[placeholder='admin / root']").first.fill("root")
        await page.locator("input[type='password']").first.fill("rootpass123")
        await page.wait_for_timeout(500)

        print("=== Click '连接' ===")
        btns = page.locator("button.cf-connect-btn")
        await btns.nth(1).click(force=True, timeout=5000)
        print("OK clicked")
        await page.wait_for_timeout(8000)

        # Check xterm
        body = await page.evaluate("() => document.body.innerText")
        print(f"\n=== Body after click ===\n{body[:1500]}")

        xterm = await page.evaluate("""() => ({
            canvas: !!document.querySelector('canvas'),
            xterm: !!document.querySelector('.xterm'),
            xterm_screen: !!document.querySelector('.xterm-screen'),
            rc_status: !!document.querySelector('.rc-status'),
            alert_err: !!document.querySelector('.el-alert--error'),
        })""")
        print(f"\n=== Viewer detection ===\n{xterm}")

        # Type a command if terminal is up
        if xterm.get('xterm') or xterm.get('xterm_screen'):
            print("\n=== Type 'whoami' into terminal ===")
            await page.keyboard.type("whoami")
            await page.keyboard.press("Enter")
            await page.wait_for_timeout(3000)
            body2 = await page.evaluate("() => document.body.innerText")
            print(f"After 'whoami':\n{body2[:1500]}")

        # API calls
        print(f"\n=== API calls ({len(api_calls)}) ===")
        for c in api_calls:
            print(f"  {c}")

        await page.screenshot(path=os.path.join(OUT, "step-real-ssh.png"), full_page=True)
        await browser.close()
        print("\nDONE")

asyncio.run(main())
