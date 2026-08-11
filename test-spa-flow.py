"""
Full 3-step E2E flow:
1. Load page, verify RemoteControl renders
2. Click protocol dropdown, select VNC
3. Fill host=172.16.0.40, port=18090 (or any)
4. Click "连接" button
5. Wait for response, check for tunnelId/wsUrl in UI
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

OUT = r"D:\ch\project\e2e-results"
os.makedirs(OUT, exist_ok=True)
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(
            headless=True, executable_path=EDGE,
            args=["--no-sandbox", "--disable-dev-shm-usage"]
        )
        ctx = await browser.new_context(viewport={"width": 1440, "height": 900})
        page = await ctx.new_page()

        console = []
        page.on("console", lambda m: console.append(f"[{m.type}] {m.text[:200]}"))
        page.on("pageerror", lambda e: console.append(f"[ERR] {e}"))
        page.on("response", lambda r: console.append(f"[NET] {r.status} {r.url[:80]}") if "/api/" in r.url else None)

        print("=== 1. Load http://127.0.0.1:7788/ ===")
        resp = await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        print(f"Status: {resp.status}")
        await page.wait_for_timeout(3000)

        # Switch to custom mode (click "自定义模式" tab)
        print("\n=== 2. Click '自定义模式' tab ===")
        custom_tab = page.locator("text=自定义模式").first
        if await custom_tab.count() > 0:
            await custom_tab.click(timeout=5000)
            print("OK clicked custom mode")
            await page.wait_for_timeout(1000)
        else:
            print("custom tab not found, trying other locators")

        # Find protocol select
        print("\n=== 3. Select VNC ===")
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(1000)
        vnc = page.locator(".el-select-dropdown__item:has-text('VNC')").first
        await vnc.click()
        await page.wait_for_timeout(500)
        print("OK VNC selected")

        # Fill host
        print("\n=== 4. Fill host/port/user/pass ===")
        inputs = page.locator("input.el-input__inner")
        n = await inputs.count()
        for i in range(n):
            ph = await inputs.nth(i).get_attribute("placeholder")
            print(f"  [{i}] placeholder={ph!r}")
        # host is the input with placeholder '192.168.1.100'
        host_in = page.locator("input[placeholder='192.168.1.100']").first
        if await host_in.count() > 0:
            await host_in.fill("172.16.0.40")
            print("OK filled host=172.16.0.40")
        # port
        port_in = page.locator("input").nth(2)  # 3rd input is port
        if await port_in.count() > 0:
            await port_in.fill("5900")
            print("OK filled port=5900")
        # user
        user_in = page.locator("input[placeholder='admin / root']").first
        if await user_in.count() > 0:
            await user_in.fill("admin")
            print("OK filled user=admin")
        # pass
        pass_in = page.locator("input[type='password']").first
        if await pass_in.count() > 0:
            await pass_in.fill("test")
            print("OK filled pass=test")

        # Click connect
        print("\n=== 5. Click '连接' button ===")
        await page.screenshot(path=os.path.join(OUT, "step4-filled.png"), full_page=True)
        connect = page.locator("button:has-text('连接')").first
        if await connect.count() > 0:
            await connect.click(timeout=5000)
            print("OK clicked connect")
            # Wait for either error or success
            await page.wait_for_timeout(5000)

        # Check result
        print("\n=== 6. Check result ===")
        # Look for error message or status tag
        body_text = await page.evaluate("() => document.body.innerText")
        print(f"Body text after auth attempt:\n{body_text[:800]}")

        # Check for tunnelId
        has_tunnel = "tunnel" in body_text.lower() or "TUN-" in body_text or "tunnelId" in body_text.lower()
        has_error = "失败" in body_text or "错误" in body_text or "refused" in body_text.lower()
        print(f"\nHas tunnel info: {has_tunnel}")
        print(f"Has error message: {has_error}")

        await page.screenshot(path=os.path.join(OUT, "step5-after-auth.png"), full_page=True)

        # Try a real SSH test
        print("\n=== 7. Try real SSH to 172.16.0.40:22 ===")
        # Switch to SSH
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        ssh = page.locator(".el-select-dropdown__item:has-text('SSH')").first
        if await ssh.count() > 0:
            await ssh.click()
            await page.wait_for_timeout(500)
            await host_in.fill("172.16.0.40")
            await port_in.fill("22")
            await user_in.fill("root")
            await pass_in.fill("test")
            print("OK filled SSH form")

            connect = page.locator("button:has-text('连接')").first
            await connect.click()
            await page.wait_for_timeout(5000)
            body_text = await page.evaluate("() => document.body.innerText")
            print(f"After SSH auth body text:\n{body_text[:800]}")

        await page.screenshot(path=os.path.join(OUT, "step6-ssh-attempt.png"), full_page=True)

        print("\n=== Console & Network ===")
        for m in console[-30:]:
            print(f"  {m}")

        await browser.close()
        print("\nDONE")

asyncio.run(main())
