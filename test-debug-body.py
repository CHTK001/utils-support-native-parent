"""
Debug: what exactly is sent in the body?
"""
import asyncio, os
from playwright.async_api import async_playwright

EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True, executable_path=EDGE, args=["--no-sandbox"])
        ctx = await browser.new_context(viewport={"width": 1600, "height": 1000})
        page = await ctx.new_page()

        bodies = []
        page.on("request", lambda r: bodies.append((r.method, r.url, r.post_data)) if "/api/connections/authenticate" in r.url else None)
        page.on("console", lambda m: print(f"[{m.type}] {m.text[:200]}"))

        await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)
        await page.locator("text=自定义模式").first.click()
        await page.wait_for_timeout(800)
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(500)
        await page.locator(".el-select-dropdown__item:has-text('SSH')").first.click()
        await page.wait_for_timeout(500)

        # List inputs precisely
        print("=== All inputs (in order) ===")
        inputs = page.locator("input")
        n = await inputs.count()
        for i in range(n):
            ph = await inputs.nth(i).get_attribute("placeholder")
            tp = await inputs.nth(i).get_attribute("type")
            v = await inputs.nth(i).input_value()
            vis = await inputs.nth(i).is_visible()
            print(f"  [{i}] type={tp} placeholder={ph!r} value={v!r} visible={vis}")

        # Now fill carefully
        print("\n=== Filling carefully ===")
        await page.locator("input[placeholder='192.168.1.100']").first.fill("172.16.0.40")
        await page.wait_for_timeout(300)
        await page.locator("input[placeholder='admin / root']").first.fill("root")
        await page.wait_for_timeout(300)
        # port - try to find the input whose value is currently "22" (auto-filled)
        # after SSH select, port may be auto-filled. Look for it
        # Inputs visible: [0] key, [1] host, [2] port, [3] user, [4] password
        port_inputs = page.locator("input")
        for i in range(await port_inputs.count()):
            v = await port_inputs.nth(i).input_value()
            vis = await port_inputs.nth(i).is_visible()
            if v == "22" and vis:
                print(f"  port input found at [{i}], value=22")
                # Don't fill it - keep auto-fill
                break
        else:
            # fill manually
            await page.locator("input").nth(2).fill("22")
        await page.wait_for_timeout(300)
        await page.locator("input[type='password']").first.fill("rootpass")
        await page.wait_for_timeout(500)

        # Re-inspect
        print("\n=== After fill ===")
        for i in range(await page.locator("input").count()):
            el = page.locator("input").nth(i)
            ph = await el.get_attribute("placeholder")
            v = await el.input_value()
            vis = await el.is_visible()
            print(f"  [{i}] placeholder={ph!r} value={v!r} visible={vis}")

        # Click connect
        btns = page.locator("button.cf-connect-btn")
        await btns.nth(1).click(force=True, timeout=5000)
        await page.wait_for_timeout(5000)

        # Check what body was sent
        print("\n=== Request bodies ===")
        for m, u, body in bodies:
            print(f"  {m} {u}")
            print(f"  body: {body}")
        await browser.close()
        print("DONE")

asyncio.run(main())
