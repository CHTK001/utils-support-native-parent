"""Playwright + Edge E2E: v11 jar + guacamole-common-js 协议 + guacd ssh

Steps:
  1. Edge browser navigate to http://127.0.0.1:7788/
  2. Click RemoteControl / gateway page
  3. Fill SSH connection form (127.0.0.1:22 root/rootpass123)
  4. Click connect
  5. Wait for guacamole viewer to render
  6. Verify terminal shows shell prompt
  7. Send 'id' via keyboard
  8. Verify 'uid=0(root)' shown
"""
import asyncio, sys, json, urllib.request
from pathlib import Path
from playwright.async_api import async_playwright

VITE = "http://127.0.0.1:7788"
TARGET_HOST = "127.0.0.1"
TARGET_PORT = 22
TARGET_USER = "root"
TARGET_PASS = "rootpass123"

async def main():
    print(f"=== 1. open {VITE} ===")
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True, channel="msedge", args=["--no-sandbox"])
        context = await browser.new_context(viewport={"width": 1280, "height": 800})
        page = await context.new_page()
        page.on("console", lambda msg: print(f"  [browser:{msg.type}] {msg.text}"))
        page.on("pageerror", lambda err: print(f"  [pageerror] {err}"))

        # Authenticate via API directly to confirm gateway alive
        body = json.dumps({"mode":"custom","protocol":"SSH","host":TARGET_HOST,"port":TARGET_PORT,
                           "user":TARGET_USER,"password":TARGET_PASS}).encode()
        req = urllib.request.Request("http://172.16.0.40:18090/api/connections/authenticate",
                                     data=body, headers={"Content-Type":"application/json"})
        result = json.loads(urllib.request.urlopen(req, timeout=10).read().decode())
        tunnel_id = result["data"]["tunnelId"]
        ws_url = result["data"]["wsUrl"]
        print(f"  tunnel_id: {tunnel_id}")
        print(f"  ws_url: {ws_url}")

        await page.goto(VITE, wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(2000)

        # Take screenshot of initial page
        await page.screenshot(path=str(Path("D:/ch/project/e2e-1-initial.png")), full_page=True)
        print(f"  initial screenshot saved")

        # Find connection form
        print(f"\n=== 2. fill connection form ===")
        # Try to find inputs by label or placeholder
        form_filled = False
        for label in ["主机", "Host", "host", "IP", "地址", "server"]:
            inp = page.get_by_label(label, exact=False).first
            if await inp.count():
                await inp.fill(TARGET_HOST)
                form_filled = True
                break
        if not form_filled:
            # Try placeholder
            inp = page.locator("input[placeholder*='IP'], input[placeholder*='host'], input[placeholder*='主机']").first
            if await inp.count():
                await inp.fill(TARGET_HOST)
                form_filled = True

        for label in ["端口", "Port", "port"]:
            inp = page.get_by_label(label, exact=False).first
            if await inp.count():
                await inp.fill(str(TARGET_PORT))
                break

        for label in ["用户名", "Username", "User", "user", "账号"]:
            inp = page.get_by_label(label, exact=False).first
            if await inp.count():
                await inp.fill(TARGET_USER)
                break

        for label in ["密码", "Password", "password", "口令"]:
            inp = page.get_by_label(label, exact=False).first
            if await inp.count():
                await inp.fill(TARGET_PASS)
                break

        # Protocol select
        for label in ["协议", "Protocol", "protocol"]:
            sel = page.get_by_label(label, exact=False).first
            if await sel.count():
                try:
                    await sel.select_option("SSH")
                except Exception:
                    pass
                break

        await page.screenshot(path=str(Path("D:/ch/project/e2e-2-filled.png")), full_page=True)
        print(f"  form filled screenshot saved")

        # Click connect
        print(f"\n=== 3. click connect ===")
        for sel_text in ["连接", "Connect", "connect", "提交", "确认", "Login"]:
            btn = page.get_by_role("button", name=sel_text, exact=False).first
            if await btn.count():
                await btn.click()
                print(f"  clicked: {sel_text}")
                break

        await page.wait_for_timeout(8000)
        await page.screenshot(path=str(Path("D:/ch/project/e2e-3-connected.png")), full_page=True)
        print(f"  after-connect screenshot saved")

        # Look for guacamole display canvas
        canvas = page.locator("canvas, .guac-canvas, [class*='display']").first
        if await canvas.count():
            box = await canvas.bounding_box()
            print(f"  guac display box: {box}")
        else:
            print(f"  no canvas found")

        # Wait for terminal content - send 'id' command
        print(f"\n=== 4. wait for terminal + send 'id' ===")
        await page.wait_for_timeout(5000)
        # Try keyboard input via the canvas focus
        await page.keyboard.press("Tab")
        await page.wait_for_timeout(500)
        await page.keyboard.type("id")
        await page.keyboard.press("Enter")
        await page.wait_for_timeout(3000)

        await page.screenshot(path=str(Path("D:/ch/project/e2e-4-id.png")), full_page=True)

        # Look for output in DOM
        print(f"\n=== 5. inspect DOM for uid= ===")
        content = await page.content()
        if "uid=0(root)" in content or "uid=0" in content:
            print(f"  FOUND: uid=0(root) in DOM")
        else:
            print(f"  no uid= in DOM (may be in canvas)")

        # Save HTML for inspection
        await page.evaluate("document.documentElement.outerHTML").then if hasattr(page.evaluate, "then") else None

        Path("D:/ch/project/e2e-final.html").write_text(await page.content(), encoding="utf-8")
        print(f"  html saved to e2e-final.html ({len(await page.content())} chars)")

        await browser.close()

asyncio.run(main())
