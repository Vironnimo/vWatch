"""The local web app: one page plus a small JSON API over the same service the CLI uses."""

from __future__ import annotations

import threading
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any

from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from lookout import __version__
from lookout.service import Lookout, LookoutError

WEB = Path(__file__).parent / "web"
REFRESH_MINUTES = 20


class FeedbackIn(BaseModel):
    item_id: str
    value: str | None = None
    project_id: str | None = None
    title: str = ""


class PrefIn(BaseModel):
    key: str
    value: Any


class StatusIn(BaseModel):
    status: str
    note: str | None = None


def create_app(service: Lookout, background_refresh: bool = True) -> FastAPI:
    stop = threading.Event()

    def refresher() -> None:
        while not stop.is_set():
            try:
                service.refresh_facts()
            except Exception:  # a failed refresh must not end the loop; the page shows the last good facts
                pass
            stop.wait(REFRESH_MINUTES * 60)

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        if background_refresh:
            threading.Thread(target=refresher, name="lookout-refresh", daemon=True).start()
        yield
        stop.set()

    app = FastAPI(title="Lookout", version=__version__, lifespan=lifespan)
    app.mount("/static", StaticFiles(directory=WEB), name="static")

    def guarded(fn, *args, **kwargs):
        try:
            return fn(*args, **kwargs)
        except LookoutError as exc:
            raise HTTPException(status_code=404 if exc.code == "project_not_found" else 400, detail=exc.as_dict()) from None

    @app.get("/")
    def index() -> FileResponse:
        return FileResponse(WEB / "index.html", headers={"Cache-Control": "no-store"})

    @app.get("/api/health")
    def health() -> dict[str, Any]:
        return {"ok": True, "app": "lookout", "version": __version__}

    @app.get("/api/state")
    def state(visit: bool = False) -> dict[str, Any]:
        return service.state(visit=visit)

    @app.post("/api/refresh")
    def refresh() -> dict[str, Any]:
        service.refresh_facts()
        return service.state()

    @app.post("/api/feedback")
    def feedback(body: FeedbackIn) -> dict[str, Any]:
        return guarded(service.set_feedback, body.item_id, body.value, body.project_id, body.title)

    @app.post("/api/prefs")
    def prefs(body: PrefIn) -> dict[str, Any]:
        guarded(service.set_pref, body.key, body.value)
        return {"ok": True}

    @app.post("/api/projects/{project_id}/status")
    def status(project_id: str, body: StatusIn) -> dict[str, Any]:
        return guarded(service.set_status, project_id, body.status, body.note)

    return app
