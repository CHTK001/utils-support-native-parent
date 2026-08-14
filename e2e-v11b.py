"""Playwright E2E v11: fill custom form + click custom tab's connect button"""
import asyncio, json, sys, urllib.request
from pathlib import Path
from playwright.async_api import async_playwright

VITE = "http://127.0.0.1:7788"

async def main():
    print("=== 1. open vite ===")
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True, channel="msedge", args=["--no-sandbox"])
        context = await browser.new_context(viewport={"width": 1360, "height": 900})
        page = await context.new_page()
        page.on("console", lambda msg: print(f"  [browser:{msg.type}] {msg.text[:150]}"))
        page.on("pageerror", lambda err: print(f"  [pageerror] {err}"))

        await page.goto(VITE, wait_until="networkidle", timeout=30000)
        await page.wait_for_timeout(3000)

        # Snapshot form structure
        print("\n=== 2. switch to '自定义模式' tab ===")
        tab = page.get_by_text("自定义模式", exact=True).first
        if await tab.count():
            await tab.click()
            await page.wait_for_timeout(1000)
            print("  switched to custom tab")

        # Fill form fields (Element Plus renders el-input as <input> with specific structure)
        print("\n=== 3. fill SSH form ===")
        # Protocol select
        sel = page.locator(".el-select .el-select__wrapper").first
        if await sel.count():
            await sel.click()
            await page.wait_for_timeout(800)
            opt = page.get_by_text("SSH（远程终端）").first
            if await opt.count():
                await opt.click()
                print("   protocol selected: ssh")
            # close dropdown
            await page.keyboard.press("Escape")

        # Host input
        inputs = page.locator("form .el-input__inner, form input")
        print(f"   found {await inputs.count()} inputs")
        # host is placeholder 192.168.1.100
        host_inp = page.locator("input[placeholder='192.168.1.100']").first
        if await host_inp.count():
            await host_inp.fill("127.0.0.1")
            print("   host=127.0.0.1")

        # port el-input-number has inner input
        spinners = await page.locator(".el-input-number").count()
        print(f"   spinners: {spinners} (port default 22, skip fill)")
        print(f"   spinners: {await page.locator('.el-input-number').count()}")

        # Fill via labels: use form-item labels
        items = await page.locator(".el-form-item").all()
        print(f"   form-items: {len(items)}")
        for it in items:
            txt = await it.locator(".el-form-item__label").inner_text()
            print(f"     item label={txt!r}")

        # Instead of complex, use get_by_label
        await page.screenshot(path=str(Path("D:/ch/project/e2e-b1-form.png")), full_page=True)

        # host
        hl = page.get_by_label("主机 / IP")
        if await hl.count():
            await hl.fill("127.0.0.1")
            print("  host filled via label")

        # username
        ul = page.get_by_label("用户名")
        if await ul.count():
            await ul.fill("root")
            print("  user filled")

        # password
        pl = page.get_by_label("密码")
        if await pl.count():
            await pl.fill("rootpass123")
            print("  pass filled")

        # port (default 22, skip to avoid el-input-number flakiness)

        await page.screenshot(path=str(Path("D:/ch/project/e2e-b2-filled.png")), full_page=True)

        print("\n=== 4. click custom-tab connect button ===")
        # The connect button is under active custom tab; use text '连接' within active pane
        btns = await page.locator("button:has-text('连接')").all()
        print(f"   connect buttons: {len(btns)}")
        # Custom tab button is last (or the one enabled)
        for i, b in enumerate(btns):
            disabled = await b.is_disabled()
            print(f"   btn[{i}] disabled={disabled}")
        # Click enabled one
        for b in btns:
            if not await b.is_disabled():
                await b.click()
                print("   clicked enabled connect button")
                break

        # Also handle the 'loading' transition
        await page.wait_for_timeout(10000)

        await page.screenshot(path=str(Path("D:/ch/project/e2e-b3-connected.png")), full_page=True)

        print("\n=== 5. check guacamole status ===")
        content = await page.content()
        text = await page.locator("body").inner_text()
        print("   page text snippet:")
        print(text[:1500])
        # Find status
        for keyword in ["Guacamole", "SSH", "connected", "connecting", "错误", "error", "已连接"]:
            if keyword in text:
                print(f"   FOUND text: {keyword}")

        # canvas?
        canvas = page.locator("canvas").first
        if await canvas.count():
            box = await canvas.bounding_box()
            print(f"   canvas box: {box}")

        html = await page.content()
        Path("D:/ch/project/e2e-b-final.html").write_text(html, encoding="utf-8")
        print("   html saved")

        await browser.close()

asyncio.run(main())