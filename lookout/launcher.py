"""Starting the web app in the background, opening it, and the Windows autostart entry."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from lookout import paths

_STARTUP_NAME = "Lookout.lnk"


def server_running() -> bool:
    try:
        with urllib.request.urlopen(f"{paths.server_url()}/api/health", timeout=1.5) as resp:
            return json.load(resp).get("app") == "lookout"
    except (OSError, ValueError):
        return False


def _windowless_python() -> str:
    exe = Path(sys.executable)
    candidate = exe.with_name("pythonw.exe")
    return str(candidate if sys.platform == "win32" and candidate.exists() else exe)


def start_background() -> bool:
    """Start `lookout serve` detached. Returns True once it answers."""
    if server_running():
        return True
    flags = 0
    if sys.platform == "win32":
        flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.CREATE_NO_WINDOW
    subprocess.Popen(
        [_windowless_python(), "-m", "lookout", "serve"],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        creationflags=flags, start_new_session=sys.platform != "win32", env=os.environ.copy(),
    )
    for _ in range(40):
        time.sleep(0.25)
        if server_running():
            return True
    return False


def _startup_file() -> Path:
    return Path(os.environ["APPDATA"]) / "Microsoft" / "Windows" / "Start Menu" / "Programs" / "Startup" / _STARTUP_NAME


def autostart(action: str) -> dict:
    """Manage a Startup-folder shortcut that launches the web app without a console window."""
    if sys.platform != "win32":
        return {"supported": False, "hint": "Autostart is only built in for Windows. Start `lookout serve` from your session startup instead."}
    target = _startup_file()
    if action == "on":
        script = (
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:LOOKOUT_LNK); "
            "$s.TargetPath = $env:LOOKOUT_EXE; $s.Arguments = '-m lookout serve'; "
            "$s.WorkingDirectory = $env:USERPROFILE; $s.WindowStyle = 7; $s.Save()"
        )
        env = {**os.environ, "LOOKOUT_LNK": str(target), "LOOKOUT_EXE": _windowless_python()}
        subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script], env=env,
                       capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
    elif action == "off" and target.exists():
        target.unlink()
    return {"supported": True, "enabled": target.exists(), "file": str(target)}
