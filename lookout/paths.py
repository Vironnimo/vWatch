"""Where Lookout keeps its data and which port the web app uses."""

import os
from pathlib import Path

DEFAULT_PORT = 8455


def data_dir() -> Path:
    env = os.environ.get("LOOKOUT_HOME")
    path = Path(env).expanduser() if env else Path.home() / ".lookout"
    path.mkdir(parents=True, exist_ok=True)
    return path


def db_path() -> Path:
    return data_dir() / "lookout.db"


def port() -> int:
    return int(os.environ.get("LOOKOUT_PORT", DEFAULT_PORT))


def server_url() -> str:
    return f"http://127.0.0.1:{port()}"
