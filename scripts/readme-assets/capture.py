#!/usr/bin/env python
"""Capture the README screenshots from the real vWatch window.

Starts the GUI against the demo instance from seed.py with WebView2 remote
debugging on a loopback port, drives it with Playwright over CDP, and writes
WebP stills plus the animated tour to .github/assets/, in the window's
dark theme. Needs the Python
`playwright` and `Pillow` packages; it installs nothing.

    python scripts/readme-assets/seed.py --replay
    python scripts/readme-assets/capture.py
"""

from __future__ import annotations

import argparse
import io
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from PIL import Image
from playwright.sync_api import Page, sync_playwright

HERE = Path(__file__).resolve().parent
CHECKOUT = HERE.parents[1]
sys.path.insert(0, str(CHECKOUT / "scripts"))
import check  # noqa: E402

PORT = 9333
VIEWPORT = {"width": 1440, "height": 900}
WIDTH = 1760  # shown at width="880" in the README


def wait_for_debugger(deadline: float) -> None:
    while True:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=1).close()
            return
        except OSError:
            if time.monotonic() > deadline:
                raise SystemExit("the window did not open its debugging port")
            time.sleep(0.25)


def shot(page: Page, clip: dict | None = None) -> Image.Image:
    """Screenshot at twice the CSS size, independent of the monitor's scaling."""
    session = page.context.new_cdp_session(page)
    area = clip or {"x": 0, "y": 0, **VIEWPORT}
    data = session.send("Page.captureScreenshot", {
        "format": "png", "captureBeyondViewport": True,
        "clip": {**area, "scale": 2 / page.evaluate("devicePixelRatio")},
    })["data"]
    session.detach()
    import base64

    image = Image.open(io.BytesIO(base64.b64decode(data))).convert("RGB")
    return image.resize((WIDTH, round(image.height * WIDTH / image.width)), Image.LANCZOS)


def area(page: Page, name: str) -> None:
    page.click(f"nav.switch button[data-style={name}]")
    page.wait_for_timeout(700)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--home", type=Path, default=CHECKOUT / ".tmp" / "readme-demo" / "home")
    parser.add_argument("--gui", type=Path,
                        default=Path(check.git(CHECKOUT, "rev-parse", "--path-format=absolute",
                                               "--git-common-dir").stdout.strip()).parent
                        / "target" / "debug" / "vWatch-UI.exe")
    parser.add_argument("--out", type=Path, default=CHECKOUT / ".github" / "assets")
    args = parser.parse_args()
    env = dict(os.environ, VWATCH_HOME=str(args.home.resolve()),
               WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f"--remote-debugging-port={PORT} "
                                                     "--remote-debugging-address=127.0.0.1")
    # The single-instance plugin would hand the start to an open window instead.
    running = subprocess.run(["tasklist", "/FI", "IMAGENAME eq vWatch-UI.exe", "/NH"],
                             capture_output=True, text=True, encoding="utf-8", errors="replace",
                             check=False).stdout
    if "vWatch-UI.exe" in running:
        raise SystemExit("close every vWatch window first; a new one would only focus it")
    window = subprocess.Popen([str(args.gui)], env=env)
    try:
        wait_for_debugger(time.monotonic() + 30)
        with sync_playwright() as playwright:
            browser = playwright.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
            page = browser.contexts[0].pages[0]
            session = page.context.new_cdp_session(page)
            session.send("Emulation.setDeviceMetricsOverride",
                         {**VIEWPORT, "deviceScaleFactor": 0, "mobile": False})
            # The README shows the window's dark theme, whatever the system uses.
            session.send("Emulation.setEmulatedMedia",
                         {"features": [{"name": "prefers-color-scheme", "value": "dark"}]})
            page.wait_for_selector("nav.switch")
            page.wait_for_timeout(1500)
            frames = scenes(page)
            browser.close()
    finally:
        window.terminate()
        window.wait(10)
    args.out.mkdir(parents=True, exist_ok=True)
    for name, image in frames.items():
        image.save(args.out / f"{name}.webp", "WEBP", quality=82, method=6)
    tour = [frames[name] for name in TOUR]
    tour[0].save(args.out / "tour.webp", "WEBP", save_all=True, append_images=tour[1:],
                 duration=3500, loop=0, quality=80, method=6)
    social(args.out / "social-preview.png")
    for path in sorted(args.out.glob("*.*")):
        print(f"{path.relative_to(CHECKOUT).as_posix()}: {path.stat().st_size // 1024} KiB")
    return 0


TOUR = ("today", "projects", "project", "ideas")


def social(target: Path) -> None:
    """The 1280x640 image for GitHub's social preview, uploaded by hand."""
    with sync_playwright() as playwright:
        # Edge ships with Windows, so no Playwright browser download is needed.
        browser = playwright.chromium.launch(channel="msedge")
        page = browser.new_page(viewport={"width": 1280, "height": 640}, color_scheme="dark")
        page.goto((HERE / "social.html").as_uri())
        page.wait_for_load_state("networkidle")
        page.screenshot(path=str(target))
        browser.close()


def scenes(page: Page) -> dict[str, Image.Image]:
    frames = {}
    area(page, "handover")
    frames["today"] = shot(page)
    area(page, "board")
    frames["projects"] = shot(page)
    page.click("button.tile[data-open=tidewatch]")
    page.wait_for_timeout(900)
    frames["project"] = shot(page)
    area(page, "ideas")
    frames["ideas"] = shot(page)
    return frames


if __name__ == "__main__":
    sys.exit(main())
