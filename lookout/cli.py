"""Command line for people and agents. Every command prints one JSON object.

Success: {"ok": true, ...}. Expected failure: {"ok": false, "error": {"code", "message", "hint"}}
with exit code 1. Wrong usage prints argparse help on stderr with exit code 2.
"""

from __future__ import annotations

import argparse
import json
import sys
import webbrowser
from pathlib import Path
from typing import Any

from lookout import __version__, paths
from lookout.cards import Card, Portfolio, json_schema
from lookout.service import DEFAULT_MAX_AGE_DAYS, STATUSES, Lookout, LookoutError
from lookout.store import Store


def _service() -> Lookout:
    return Lookout(Store(paths.db_path()))


def _read_document(args: argparse.Namespace) -> Any:
    if args.json is not None:
        text, origin = args.json, "--json"
    elif args.file == "-":
        text, origin = sys.stdin.buffer.read().decode("utf-8-sig"), "stdin"
    elif args.file:
        try:
            text, origin = Path(args.file).read_text(encoding="utf-8-sig"), args.file
        except OSError as exc:
            raise LookoutError("file_unreadable", f"Cannot read {args.file}: {exc.strerror}.") from None
    else:
        raise LookoutError("missing_document", "No document given.", "Pass --file <path>, --file - for stdin, or --json '<object>'.")
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise LookoutError("invalid_json", f"{origin} is not valid JSON: {exc.msg} at line {exc.lineno}, column {exc.colno}.") from None


def _add_document_args(p: argparse.ArgumentParser) -> None:
    src = p.add_mutually_exclusive_group()
    src.add_argument("--file", help="JSON file with the document; '-' reads stdin")
    src.add_argument("--json", help="the document as a JSON string")
    p.add_argument("--merge", action="store_true", help="send only changed top-level fields; the rest is kept from the previous version")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="lookout", description="Morning overview of all your agent-built projects. Every command prints JSON.")
    parser.add_argument("--version", action="version", version=__version__)
    sub = parser.add_subparsers(dest="command", required=True, metavar="<command>")

    sub.add_parser("status", help="data location, registered projects, web app state")

    projects = sub.add_parser("projects", help="register and manage the watched projects")
    psub = projects.add_subparsers(dest="action", required=True, metavar="<action>")
    psub.add_parser("list", help="all registered projects")
    add = psub.add_parser("add", help="register a project folder (usually a git repository)")
    add.add_argument("path")
    add.add_argument("--id", help="short id; defaults to the folder name")
    add.add_argument("--name", help="display name; defaults to the folder name")
    rm = psub.add_parser("remove", help="stop watching a project and delete its stored cards")
    rm.add_argument("id")
    rn = psub.add_parser("rename", help="change the display name")
    rn.add_argument("id")
    rn.add_argument("name")
    st = psub.add_parser("status", help="set what the owner intends: " + ", ".join(STATUSES))
    st.add_argument("id")
    st.add_argument("status", choices=STATUSES)
    st.add_argument("--note", help="short reason, shown to people and agents")
    disc = psub.add_parser("discover", help="list git repositories directly inside a folder that are not registered yet")
    disc.add_argument("root")

    facts = sub.add_parser("facts", help="exact git facts computed without a model")
    fsub = facts.add_subparsers(dest="action", required=True, metavar="<action>")
    fr = fsub.add_parser("refresh", help="recompute facts now (all projects when no id is given)")
    fr.add_argument("ids", nargs="*")
    fr.add_argument("--no-ci", action="store_true", help="skip the GitHub CI lookup")
    fs = fsub.add_parser("show", help="facts of one project")
    fs.add_argument("id")

    due = sub.add_parser("due", help="refresh facts and list the projects that need a new card, plus whether the portfolio brief is due")
    due.add_argument("--max-age-days", type=int, default=DEFAULT_MAX_AGE_DAYS, help=f"rewrite unchanged cards older than this (default {DEFAULT_MAX_AGE_DAYS})")
    due.add_argument("--no-refresh", action="store_true", help="use the stored facts")

    ctx = sub.add_parser("context", help="everything an agent needs before writing a project card")
    ctx.add_argument("id")

    card = sub.add_parser("card", help="read or write a project card")
    csub = card.add_subparsers(dest="action", required=True, metavar="<action>")
    cput = csub.add_parser("put", help="save a new card version")
    cput.add_argument("id")
    _add_document_args(cput)
    cshow = csub.add_parser("show", help="the current card")
    cshow.add_argument("id")

    port = sub.add_parser("portfolio", help="the cross-project brief")
    posub = port.add_subparsers(dest="action", required=True, metavar="<action>")
    posub.add_parser("context", help="all projects side by side, the previous brief and all ratings")
    pput = posub.add_parser("put", help="save a new brief")
    _add_document_args(pput)
    posub.add_parser("show", help="the current brief")

    fb = sub.add_parser("feedback", help="the owner's ratings of suggestions and ideas")
    fb.add_argument("--project", help="only ratings for this project")

    schema = sub.add_parser("schema", help="JSON schema of a document")
    schema.add_argument("kind", choices=["card", "portfolio"])

    serve = sub.add_parser("serve", help="run the web app in the foreground")
    serve.add_argument("--port", type=int, default=None)
    sub.add_parser("open", help="start the web app in the background if needed and open it in the browser")
    auto = sub.add_parser("autostart", help="start the web app when you sign in to Windows")
    auto.add_argument("action", choices=["on", "off", "status"])
    return parser


def run(args: argparse.Namespace) -> Any:
    from lookout import launcher

    cmd, action = args.command, getattr(args, "action", None)
    if cmd == "schema":
        return {"schema": json_schema(Card if args.kind == "card" else Portfolio)}
    if cmd == "serve":
        import uvicorn

        from lookout.server import create_app

        uvicorn.run(create_app(_service()), host="127.0.0.1", port=args.port or paths.port(), log_level="warning")
        return None
    if cmd == "open":
        started = launcher.start_background()
        if started:
            webbrowser.open(paths.server_url())
        return {"url": paths.server_url(), "running": started}
    if cmd == "autostart":
        return launcher.autostart(args.action)

    svc = _service()
    if cmd == "status":
        projects = svc.list_projects()
        return {"version": __version__, "data_dir": str(paths.data_dir()), "database": str(paths.db_path()),
                "web": {"url": paths.server_url(), "running": launcher.server_running()},
                "projects": len(projects), "with_card": sum(1 for p in projects if p["has_card"]),
                "hint": None if projects else "Register a project with `lookout projects add <path>`."}
    if cmd == "projects":
        if action == "list":
            return {"projects": svc.list_projects()}
        if action == "add":
            return {"project": svc.add_project(args.path, args.id, args.name)}
        if action == "remove":
            return svc.remove_project(args.id)
        if action == "rename":
            return {"project": svc.rename_project(args.id, args.name)}
        if action == "status":
            return {"project": svc.set_status(args.id, args.status, args.note)}
        if action == "discover":
            return {"candidates": svc.discover(args.root)}
    if cmd == "facts":
        if action == "refresh":
            return {"refreshed": svc.refresh_facts(args.ids or None, with_ci=not args.no_ci)}
        return {"facts": svc.facts(args.id)}
    if cmd == "due":
        return svc.due(max_age_days=args.max_age_days, refresh=not args.no_refresh)
    if cmd == "context":
        return svc.context(args.id)
    if cmd == "card":
        if action == "put":
            return svc.put_card(args.id, _read_document(args), merge=args.merge)
        return svc.card(args.id)
    if cmd == "portfolio":
        if action == "context":
            return svc.portfolio_context()
        if action == "put":
            return svc.put_portfolio(_read_document(args), merge=args.merge)
        return svc.portfolio()
    if cmd == "feedback":
        return {"feedback": svc.feedback(args.project)}
    raise AssertionError(f"unhandled command {cmd}")


def main(argv: list[str] | None = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    args = build_parser().parse_args(argv)
    try:
        result = run(args)
    except LookoutError as exc:
        print(json.dumps({"ok": False, "error": exc.as_dict()}, ensure_ascii=False, indent=2))
        return 1
    if result is not None:
        print(json.dumps({"ok": True, **result}, ensure_ascii=False, indent=2))
    return 0
