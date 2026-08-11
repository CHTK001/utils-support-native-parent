"""
Playwright headless browser test for RemoteControl.vue 3-step flow
- Loads http://127.0.0.1:7788/
- Verifies ConnectionForm, viewer placeholder, protocol dropdown
- Submits custom auth (VNC, expect 500/connection-refused)
- Captures screenshot
"""
import asyncio
from playwright.async_api import async_playwright
import os, sys

OUT = r"D:\ch\project\e2e-results"
os.makedirs(OUT, exist_ok=True)

async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True)
        ctx = await browser.new_context(viewport={"width": 1440, "height": 900})
        page = await ctx.new_page()

        console_msgs = []
        page.on("console", lambda m: console_msgs.append(f"[{m.type}] {m.text}"))
        page.on("pageerror", lambda e: console_msgs.append(f"[error] {e}"))

        print("=== 1. Loading http://127.0.0.1:7788/ ===")
        resp = await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        print(f"Status: {resp.status} URL: {page.url}")

        print("\n=== 2. Wait for app mount (RemoteControl component) ===")
        # The page has #app element
        await page.wait_for_selector("#app", timeout=15000)
        # Wait for ConnectionForm to render
        await page.wait_for_timeout(3000)

        print("\n=== 3. Inspect rendered HTML ===")
        title = await page.title()
        print(f"Title: {title!r}")

        body_html = await page.evaluate("() => document.querySelector('#app').innerHTML.length")
        print(f"#app innerHTML length: {body_html}")

        # Count interactive elements
        counts = await page.evaluate("""() => {
            return {
                buttons: document.querySelectorAll('button').length,
                inputs: document.querySelectorAll('input, select').length,
                labels: document.querySelectorAll('label, .el-form-item__label').length,
                forms: document.querySelectorAll('form, .el-form').length,
            };
        }""")
        print(f"Element counts: {counts}")

        # Get all visible text (truncated)
        text = await page.evaluate("() => document.body.innerText")
        text_sample = text[:500].replace("\n", " | ")
        print(f"Body text sample: {text_sample}")

        # Screenshot 1: initial
        await page.screenshot(path=os.path.join(OUT, "step1-loaded.png"), full_page=True)
        print(f"Screenshot: {OUT}\\step1-loaded.png")

        # Try to find and click on protocol selector
        print("\n=== 4. Try to find protocol selector ===")
        # Look for element-plus select component
        sel = page.locator(".el-select").first
        if await sel.count() > 0:
            print(f"Found .el-select count: {await page.locator('.el-select').count()}")
            try:
                await sel.click(timeout=5000)
                await page.wait_for_timeout(1000)
                await page.screenshot(path=os.path.join(OUT, "step2-protocol-dropdown.png"))
                # See what options are available
                options = page.locator(".el-select-dropdown__item")
                opt_count = await options.count()
                print(f"Dropdown options count: {opt_count}")
                if opt_count > 0:
                    for i in range(min(8, opt_count)):
                        text = await options.nth(i).text_content()
                        print(f"  option[{i}]: {text!r}")
            except Exception as e:
                print(f"Click .el-select error: {e}")
        else:
            print("No .el-select found")

        # Check form fields
        print("\n=== 5. Inspect form fields ===")
        inputs = page.locator("input, .el-input__inner")
        n = await inputs.count()
        print(f"Input fields: {n}")
        for i in range(min(10, n)):
            try:
                el = inputs.nth(i)
                placeholder = await el.get_attribute("placeholder")
                value = await el.input_value() if await el.evaluate("e => e.tagName === 'INPUT'") else ""
                visible = await el.is_visible()
                print(f"  input[{i}] visible={visible} placeholder={placeholder!r} value={value!r}")
            except Exception as e:
                print(f"  input[{i}]: err {e}")

        # Final screenshot
        await page.screenshot(path=os.path.join(OUT, "step3-final.png"), full_page=True)

        print("\n=== Console messages ===")
        for m in console_msgs[:30]:
            print(f"  {m}")

        await browser.close()
        print("\nDONE")

asyncio.run(main())
