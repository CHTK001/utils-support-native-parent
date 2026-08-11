"""
Playwright + Edge browser test
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

OUT = r"D:\ch\project\e2e-results"
os.makedirs(OUT, exist_ok=True)

EDGE_PATH = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(
            headless=True,
            executable_path=EDGE_PATH,
            args=["--no-sandbox", "--disable-dev-shm-usage"]
        )
        ctx = await browser.new_context(viewport={"width": 1440, "height": 900})
        page = await ctx.new_page()

        console_msgs = []
        page.on("console", lambda m: console_msgs.append(f"[{m.type}] {m.text[:200]}"))
        page.on("pageerror", lambda e: console_msgs.append(f"[pageerror] {e}"))

        print("=== 1. Loading http://127.0.0.1:7788/ ===")
        try:
            resp = await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
            print(f"Status: {resp.status} URL: {page.url}")
        except Exception as e:
            print(f"Goto error: {e}")
            # Try domcontentloaded
            try:
                resp = await page.goto("http://127.0.0.1:7788/", wait_until="domcontentloaded", timeout=15000)
                print(f"Retry status: {resp.status if resp else 'None'}")
            except Exception as e2:
                print(f"Retry also failed: {e2}")
                await browser.close()
                return

        print("\n=== 2. Wait for #app ===")
        try:
            await page.wait_for_selector("#app", timeout=15000)
            print("OK #app found")
        except Exception as e:
            print(f"#app not found: {e}")
        await page.wait_for_timeout(3000)

        print("\n=== 3. Inspect rendered HTML ===")
        title = await page.title()
        print(f"Title: {title!r}")

        body_len = await page.evaluate("() => document.body.innerText.length")
        app_len = await page.evaluate("() => (document.querySelector('#app')?.innerHTML || '').length")
        print(f"body.innerText length: {body_len}, #app innerHTML length: {app_len}")

        counts = await page.evaluate("""() => ({
            buttons: document.querySelectorAll('button').length,
            inputs: document.querySelectorAll('input').length,
            selects: document.querySelectorAll('.el-select').length,
            el_form_items: document.querySelectorAll('.el-form-item').length,
        })""")
        print(f"Element counts: {counts}")

        # Body text
        try:
            text = await page.evaluate("() => document.body.innerText")
            text_sample = text[:600].replace("\n", " | ")
            print(f"Body text sample: {text_sample}")
        except Exception as e:
            print(f"text err: {e}")

        # Screenshot 1
        await page.screenshot(path=os.path.join(OUT, "step1-loaded.png"), full_page=True)
        print(f"Screenshot: step1-loaded.png")

        # Click first .el-select
        print("\n=== 4. Open protocol dropdown ===")
        sel = page.locator(".el-select").first
        if await sel.count() > 0:
            try:
                await sel.click(timeout=5000)
                await page.wait_for_timeout(1500)
                options = page.locator(".el-select-dropdown__item")
                opt_count = await options.count()
                print(f"Dropdown options: {opt_count}")
                for i in range(min(8, opt_count)):
                    txt = await options.nth(i).text_content()
                    print(f"  [{i}]: {txt!r}")
                await page.screenshot(path=os.path.join(OUT, "step2-protocol-dropdown.png"))

                # Try to click VNC option
                vnc_opt = page.locator(".el-select-dropdown__item:has-text('VNC')").first
                if await vnc_opt.count() > 0:
                    await vnc_opt.click(timeout=3000)
                    print("Clicked VNC")
                    await page.wait_for_timeout(1000)
            except Exception as e:
                print(f"Dropdown err: {e}")
        else:
            print("No .el-select found")

        # Inspect inputs
        print("\n=== 5. Form fields ===")
        inputs = page.locator("input.el-input__inner")
        n = await inputs.count()
        print(f"Total .el-input__inner: {n}")
        for i in range(min(8, n)):
            try:
                el = inputs.nth(i)
                ph = await el.get_attribute("placeholder")
                v = await el.input_value()
                vis = await el.is_visible()
                print(f"  [{i}] vis={vis} ph={ph!r} v={v!r}")
            except Exception as e:
                print(f"  [{i}]: err {e}")

        await page.screenshot(path=os.path.join(OUT, "step3-final.png"), full_page=True)

        # Console messages
        print("\n=== Console messages (last 20) ===")
        for m in console_msgs[-20:]:
            print(f"  {m}")

        await browser.close()
        print("\nDONE")

asyncio.run(main())
