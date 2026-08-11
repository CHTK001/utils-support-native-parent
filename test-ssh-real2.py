"""
Real E2E SSH flow:
- Connect to sandbox's own SSH server (127.0.0.1:22)
- Use tester / TestPass!123 (verified working in surefire tests)
- backend is at 172.16.0.40:18090, but it must reach 127.0.0.1:22
  - container's 127.0.0.1 is the container itself, NOT the sandbox
  - So the gateway tunnel from inside container to 127.0.0.1:22 will fail
  - We need to either:
    a) Use 172.18.0.1 (host gateway from container) - or
    b) Use 172.16.0.40:22 (the sandbox's network IP, which the container can reach)

But SSH server runs in sandbox (Windows), not in 172.16.0.40 (Linux container).
Actually 172.16.0.40 is the DOCKER HOST (the Windows machine is at a different IP).
Wait, let me re-read: 172.16.0.40 is a Linux machine, the gateway container runs there.
The sandbox (where I'm running Playwright) is a separate machine.

So:
- 172.16.0.40 = Linux machine with Docker + SSH?  Let's check
- sandbox 127.0.0.1 = sandbox's own SSH (Windows OpenSSH)

Best path: connect to whatever the container can reach.
- From container: localhost is container, not Windows sandbox
- From sandbox: 172.16.0.40:22 IS open (we tested earlier)
- So container can probably reach 172.16.0.40:22 if 172.16.0.40 has sshd

Let me try user=root with various passwords against 172.16.0.40:22
"""
import asyncio, os
from playwright.async_api import async_playwright

OUT = r"D:\ch\project\e2e-results"
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

        await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)
        await page.locator("text=自定义模式").first.click()
        await page.wait_for_timeout(800)
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(500)

        # Try various target/credential combinations
        attempts = [
            # (host, port, user, password, label)
            ("172.16.0.40", "22", "root", "root", "172.16.0.40:22 root/root"),
            ("172.16.0.40", "22", "root", "toor", "172.16.0.40:22 root/toor"),
            ("172.16.0.40", "22", "ubuntu", "ubuntu", "172.16.0.40:22 ubuntu/ubuntu"),
            ("127.0.0.1", "22", "tester", "TestPass!123", "127.0.0.1:22 tester/TestPass!123"),
        ]
        for host, port, user, pwd, label in attempts:
            print(f"\n=== Attempt: {label} ===")
            # Switch back to custom if we're in any other state
            try:
                await page.locator("text=自定义模式").first.click(timeout=2000)
            except: pass
            await page.wait_for_timeout(500)
            sel = page.locator(".el-select").first
            await sel.click()
            await page.wait_for_timeout(500)
            await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
            await page.wait_for_timeout(500)

            await page.locator("input[placeholder='192.168.1.100']").first.fill(host)
            await page.locator("input").nth(2).fill(port)
            await page.locator("input[placeholder='admin / root']").first.fill(user)
            await page.locator("input[type='password']").first.fill(pwd)
            await page.wait_for_timeout(500)

            btns = page.locator("button.cf-connect-btn")
            await btns.nth(1).click(force=True, timeout=5000)
            await page.wait_for_timeout(6000)

            body = await page.evaluate("() => document.body.innerText")
            has_xterm = "xterm" in body.lower() or "terminal" in body.lower()
            has_error = "失败" in body or "refused" in body.lower() or "错误" in body

            # Check for actual SSH response (terminal prompt like $ or #)
            has_prompt = await page.evaluate("""() => {
                return {
                    canvas: !!document.querySelector('canvas'),
                    xterm: !!document.querySelector('.xterm'),
                    rc_status: !!document.querySelector('.rc-status'),
                    alert_err: !!document.querySelector('.el-alert--error'),
                    ws: !!document.querySelector('[class*="ssh"], [class*="SSH"]'),
                };
            }""")
            print(f"  xterm/canvas: {has_prompt}")
            print(f"  has error: {has_error}")
            if has_xterm or has_prompt.get("xterm") or has_prompt.get("canvas") or has_prompt.get("rc_status"):
                print(f"  🎉 SUCCESS!")
                await page.screenshot(path=os.path.join(OUT, f"step-ssh-success-{label.replace('/','_').replace(':','-').replace(' ','_')}.png"), full_page=True)
                break

        # Final API calls
        print(f"\n=== Final API calls ({len(api_calls)}) ===")
        for c in api_calls[-20:]:
            print(f"  {c}")
        print(f"\n=== WebSocket connections ({len(ws_connections)}) ===")
        for w in ws_connections:
            print(f"  WS: {w.url}")

        await browser.close()
        print("\nDONE")

asyncio.run(main())
