from datetime import timedelta

import pytest

from conftest import card, git
from lookout.service import LookoutError


def test_morning_run_cycle(service, make_repo, clock):
    repo = make_repo()
    service.add_project(str(repo))

    due = service.due()
    assert due["projects"] == [{"id": "demo", "name": "demo", "reason": "no_card"}]
    assert due["portfolio"]["due"] is True

    saved = service.put_card("demo", card())
    assert saved["saved"] and saved["rated_items"][0]["id"].startswith("sg-")
    assert service.due()["projects"] == []

    (repo / "f0.txt").write_text("more\n")
    git(repo, "commit", "-qam", "second")
    assert service.due()["projects"][0]["reason"] == "changed"
    ctx = service.context("demo")
    assert ctx["previous_card"]["card"]["headline"] == "Alles ruhig"
    assert ctx["since_previous_card"]["commit_count"] == 1 and ctx["since_previous_card"]["commits"][0].endswith("second")

    service.put_card("demo", card())
    clock.now += timedelta(days=15)
    assert service.due()["projects"][0]["reason"] == "stale"


def test_merge_keeps_unsent_fields_and_rated_ids(service, make_repo):
    service.add_project(str(make_repo()))
    first = service.put_card("demo", card(features=["Suche"]))
    sid = first["rated_items"][0]["id"]
    service.set_feedback(sid, "down", "demo", "Dark Mode")

    service.put_card("demo", {"headline": "Neu"}, merge=True)
    current = service.card("demo")["card"]
    assert current["headline"] == "Neu" and current["features"] == ["Suche"]
    assert current["suggestions"][0]["id"] == sid
    assert service.context("demo")["feedback"] == [{"id": sid, "title": "Dark Mode", "value": "down", "at": service.now()}]


def test_invalid_cards_are_refused_with_field_paths(service, make_repo):
    service.add_project(str(make_repo()))
    with pytest.raises(LookoutError) as exc:
        service.put_card("demo", card(phase="sleepy", reentry={"next_step": "x"}))
    assert exc.value.code == "invalid_card"
    assert "phase" in exc.value.message and "reentry.effort" in exc.value.message
    assert service.put_card("demo", card(colour="blue"))["warnings"] == ["ignored unknown field: colour"]


def test_portfolio_checks_project_ids_and_is_due_each_day(service, make_repo, clock):
    service.add_project(str(make_repo()))
    service.put_card("demo", card())
    brief = {"greeting": "Moin", "focus": [{"project": "ghost", "title": "x", "why": "y", "effort": "1h"}]}
    with pytest.raises(LookoutError) as exc:
        service.put_portfolio(brief)
    assert exc.value.code == "unknown_project" and "demo" in exc.value.hint

    brief["focus"][0]["project"] = "demo"
    service.put_portfolio(brief)
    assert service.due()["portfolio"]["due"] is False
    clock.now += timedelta(days=1)
    assert service.due()["portfolio"]["reason"] == "new_day"


def test_visits_remember_the_previous_one(service, clock):
    first = service.state(visit=True)
    assert first["previous_visit"] is None
    start = service.now()
    clock.now += timedelta(minutes=10)
    assert service.state(visit=True)["previous_visit"] is None  # a reload is not a new visit
    clock.now += timedelta(hours=5)
    assert service.state(visit=True)["previous_visit"] == service.store.pref("previous_visit")
    assert service.store.pref("previous_visit") > start


def test_status_and_registration_rules(service, make_repo, clock):
    repo = make_repo()
    service.add_project(str(repo))
    with pytest.raises(LookoutError) as exc:
        service.add_project(str(repo))
    assert exc.value.code == "already_registered"
    service.put_card("demo", card())
    clock.now += timedelta(minutes=1)
    service.set_status("demo", "parked", "wartet auf Key")
    assert service.due()["projects"][0]["reason"] == "status_changed"
    service.set_status("demo", "archived")
    assert service.due()["projects"] == [] and service.store.project("demo")["status_note"] == "wartet auf Key"
