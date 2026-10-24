"""Save the recorded demo state into a freshly seeded instance.

demo-state/ holds what a real agent daily run wrote for the demo projects
(cards/<project-id>.json and portfolio.json, exported with `card show --json` and
`portfolio show --json`) and owner.json, the owner's reactions as CLI argument
lists. Item IDs in the documents are kept, so owner.json can name them.

Dates inside the documents are absolute while the seeded commits are relative
to today. Re-record the state with a fresh agent run when that gap shows.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

Vwatch = Callable[..., dict]


def run(vwatch: Vwatch, state: Path) -> None:
    run_id = str(vwatch("runs", "start")["run_id"])
    for card in sorted((state / "cards").glob("*.json")):
        project = card.stem
        token = vwatch("context", project)["context_token"]
        vwatch("card", "put", project, "--context-token", token, "--file", str(card))
    token = vwatch("portfolio", "context")["context_token"]
    vwatch("portfolio", "put", "--context-token", token, "--file", str(state / "portfolio.json"))
    vwatch("runs", "finish", run_id)
    owner = state / "owner.json"
    if owner.is_file():
        for call in json.loads(owner.read_text(encoding="utf-8")):
            vwatch(*call)


def export(vwatch: Vwatch, state: Path) -> None:
    """Record the cards and brief an agent wrote into a demo instance."""
    cards = state / "cards"
    cards.mkdir(parents=True, exist_ok=True)
    for project in vwatch("projects", "list")["projects"]:
        document = vwatch("card", "show", project["id"])["card"]
        write(cards / f"{project['id']}.json", document)
    write(state / "portfolio.json", vwatch("portfolio", "show")["portfolio"])


def write(path: Path, document: dict) -> None:
    path.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
