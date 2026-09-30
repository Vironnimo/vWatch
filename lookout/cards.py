"""The two documents agents write: a project card and the portfolio brief.

Cards are validated leniently: unknown fields are dropped and reported as
warnings, while missing or malformed required fields fail with the field path.
Items that people rate (suggestions, patterns, synergies, app ideas, verdicts)
carry a stable `id`; new items get one, and an agent keeps an item's identity by
sending its previous id again.
"""

from __future__ import annotations

import secrets
from typing import Annotated, Any, Literal

from pydantic import BaseModel, ConfigDict, Field, ValidationError

Date = Annotated[str, Field(pattern=r"^\d{4}-\d{2}-\d{2}$")]
Short = Annotated[str, Field(min_length=1, max_length=160)]
Text = Annotated[str, Field(max_length=600)]
ItemId = Annotated[str, Field(pattern=r"^[a-z0-9][a-z0-9:_-]{2,63}$")]
Effort = Literal["15min", "1h", "half-day", "day+"]
Size = Literal["S", "M", "L"]


class _Model(BaseModel):
    model_config = ConfigDict(extra="ignore")


class Change(_Model):
    date: Date
    title: Short
    detail: Text = ""


class Milestone(_Model):
    date: Date
    title: Short


class OpenEnd(_Model):
    kind: Literal["worktree", "branch", "uncommitted", "flagged", "todo", "plan", "other"] = "other"
    title: Short
    detail: Text = ""
    source: Text = ""
    age_days: int | None = None


class Question(_Model):
    title: Short
    source: Text = ""


class Vision(_Model):
    statement: Text | None = None
    progress_note: Text | None = None
    progress_pct: Annotated[int, Field(ge=0, le=100)] | None = None


class Reentry(_Model):
    next_step: Short
    effort: Effort
    why: Text = ""


class Suggestion(_Model):
    id: ItemId | None = None
    type: Literal["next", "feature", "cleanup", "park"]
    title: Short
    why: Text
    evidence: Text = ""
    effort: Size


class Card(_Model):
    one_liner: Short
    stack: Text = ""
    phase: Literal["active", "cooling", "dormant", "maintenance", "done"]
    phase_evidence: Text = ""
    headline: Short
    personality: Text = ""
    recent_changes: list[Change] = []
    milestones: list[Milestone] = []
    features: list[Short] = []
    open_ends: list[OpenEnd] = []
    waiting_on_you: list[Question] = []
    vision: Vision = Vision()
    reentry: Reentry
    health_notes: list[Text] = []
    suggestions: list[Suggestion] = []
    cross_project_hints: list[Text] = []


class Focus(_Model):
    project: Short
    title: Short
    why: Text
    effort: Effort


class Pattern(_Model):
    id: ItemId | None = None
    title: Short
    why: Text
    projects: list[Short] = []
    evidence: Text = ""
    severity: Literal["info", "warn", "risk"] = "info"


class Synergy(_Model):
    id: ItemId | None = None
    title: Short
    why: Text
    projects: list[Short] = []
    evidence: Text = ""
    effort: Size


class AppIdea(_Model):
    id: ItemId | None = None
    title: Short
    pitch: Text
    because: Text
    effort: Size


class Verdict(_Model):
    id: ItemId | None = None
    project: Short
    verdict: Literal["park", "revive", "finish", "archive"]
    why: Text


class Portfolio(_Model):
    greeting: Text
    focus: Annotated[list[Focus], Field(max_length=5)] = []
    patterns: list[Pattern] = []
    synergies: list[Synergy] = []
    new_apps: list[AppIdea] = []
    park_or_revive: list[Verdict] = []
    attention_note: Text = ""


class CardError(Exception):
    def __init__(self, problems: list[str]):
        super().__init__("; ".join(problems))
        self.problems = problems


_RATED = {
    Card: {"suggestions": "sg"},
    Portfolio: {"patterns": "pt", "synergies": "sy", "new_apps": "ap", "park_or_revive": "vd"},
}


def _ignored(model: type[BaseModel], data: Any, prefix: str = "") -> list[str]:
    """Names of fields the model does not know, so agents learn about typos."""
    if not isinstance(data, dict):
        return []
    found = []
    for key, value in data.items():
        field = model.model_fields.get(key)
        path = f"{prefix}{key}"
        if field is None:
            found.append(path)
            continue
        inner = field.annotation
        args = getattr(inner, "__args__", ())
        item_model = next((a for a in args if isinstance(a, type) and issubclass(a, BaseModel)), None)
        if isinstance(inner, type) and issubclass(inner, BaseModel):
            found += _ignored(inner, value, f"{path}.")
        elif item_model and isinstance(value, list):
            for i, item in enumerate(value):
                found += _ignored(item_model, item, f"{path}[{i}].")
    return found


def validate(kind: type[Card] | type[Portfolio], data: Any) -> tuple[dict[str, Any], list[str]]:
    """Validate a card and assign ids to rated items. Returns (clean data, warnings)."""
    if not isinstance(data, dict):
        raise CardError(["the document must be a JSON object"])
    try:
        model = kind.model_validate(data)
    except ValidationError as exc:
        problems = []
        for err in exc.errors():
            loc = ".".join(str(p) for p in err["loc"]) or "(document)"
            problems.append(f"{loc}: {err['msg']}")
        raise CardError(problems) from None
    clean = model.model_dump()
    seen: set[str] = set()
    for field, prefix in _RATED[kind].items():
        for item in clean[field]:
            if not item.get("id") or item["id"] in seen:
                item["id"] = f"{prefix}-{secrets.token_hex(4)}"
            seen.add(item["id"])
    warnings = [f"ignored unknown field: {path}" for path in _ignored(kind, data)]
    return clean, warnings


def json_schema(kind: type[Card] | type[Portfolio]) -> dict[str, Any]:
    return kind.model_json_schema()
