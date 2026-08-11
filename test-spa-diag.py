"""
Diagnose form state - why is connect button disabled?
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

        page.on("console", lambda m: print(f"[{m.type}] {m.text[:200]}"))
        page.on("response", lambda r: print(f"[NET {r.status}] {r.url[:80]}") if "/api/" in r.url else None)

        await page.goto("http://127.0.0.1:7788/", wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)

        # Find all buttons
        print("=== Buttons ===")
        btns = page.locator("button")
        n = await btns.count()
        for i in range(n):
            try:
                t = await btns.nth(i).text_content()
                dis = await btns.nth(i).get_attribute("disabled")
                cls = await btns.nth(i).get_attribute("class")
                print(f"  [{i}] text={t!r} disabled={dis!r} class={cls!r}")
            except: pass

        # Switch to custom
        await page.locator("text=自定义模式").first.click()
        await page.wait_for_timeout(1000)

        # Pick VNC
        sel = page.locator(".el-select").first
        await sel.click()
        await page.wait_for_timeout(800)
        vnc = page.locator(".el-select-dropdown__item:has-text('VNC')").first
        await vnc.click()
        await page.wait_for_timeout(500)

        # Fill
        host_in = page.locator("input[placeholder='192.168.1.100']").first
        await host_in.fill("172.16.0.40")
        port_in = page.locator("input").nth(2)
        await port_in.fill("5900")
        user_in = page.locator("input[placeholder='admin / root']").first
        await user_in.fill("admin")
        pass_in = page.locator("input[type='password']").first
        await pass_in.fill("test")
        await page.wait_for_timeout(500)

        print("\n=== After fill: ===")
        # Recheck buttons
        btns = page.locator("button")
        n = await btns.count()
        for i in range(n):
            try:
                t = await btns.nth(i).text_content()
                dis = await btns.nth(i).get_attribute("disabled")
                vis = await btns.nth(i).is_visible()
                print(f"  [{i}] vis={vis} text={t!r} disabled={dis!r}")
            except: pass

        # Check form validation messages
        print("\n=== Form errors ===")
        form_items = page.locator(".el-form-item__error")
        n = await form_items.count()
        for i in range(n):
            try:
                t = await form_items.nth(i).text_content()
                vis = await form_items.nth(i).is_visible()
                print(f"  [{i}] vis={vis} text={t!r}")
            except: pass

        # Check what fields are required
        print("\n=== Required indicators ===")
        required = page.locator(".el-form-item.is-required")
        n = await required.count()
        for i in range(n):
            try:
                lbl = await required.nth(i).text_content()
                print(f"  [{i}] {lbl!r}")
            except: pass

        # Force-click the connect via JS (bypass disabled)
        print("\n=== Force submit via JS ===")
        # Find the connect button by class
        result = await page.evaluate("""() => {
            const btn = document.querySelector('.cf-connect-btn');
            if (!btn) return 'no btn';
            return {
                disabled: btn.disabled,
                aria_disabled: btn.getAttribute('aria-disabled'),
                class: btn.className,
                text: btn.textContent.trim(),
            };
        }""")
        print(f"Button state: {result}")

        # Try removing disabled and click
        result2 = await page.evaluate("""() => {
            const btn = document.querySelector('.cf-connect-btn');
            if (!btn) return 'no btn';
            btn.disabled = false;
            btn.removeAttribute('aria-disabled');
            btn.classList.remove('is-disabled');
            return 'unlocked';
        }""")
        print(f"After unlock: {result2}")

        await page.locator(".cf-connect-btn").click(force=True, timeout=5000)
        await page.wait_for_timeout(5000)

        # Check result
        body = await page.evaluate("() => document.body.innerText")
        print(f"\n=== Body after force click ===\n{body[:1500]}")

        await page.screenshot(path=os.path.join(OUT, "step-force-click.png"), full_page=True)

        await browser.close()

asyncio.run(main())
