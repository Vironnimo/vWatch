import json

from fastapi.testclient import TestClient

from conftest import card
from lookout import cli
from lookout.server import create_app


def run_cli(capsys, *argv):
    code = cli.main(list(argv))
    return code, json.loads(capsys.readouterr().out)


def test_cli_prints_json_for_success_and_expected_failure(tmp_path, monkeypatch, capsys, make_repo):
    monkeypatch.setenv("LOOKOUT_HOME", str(tmp_path / "home"))
    repo = make_repo()

    code, out = run_cli(capsys, "projects", "add", str(repo), "--id", "demo")
    assert code == 0 and out["ok"] and out["project"]["id"] == "demo"

    code, out = run_cli(capsys, "card", "put", "demo", "--json", json.dumps(card()))
    assert code == 0 and out["saved"]

    doc = tmp_path / "patch.json"
    doc.write_text(json.dumps({"headline": "Aus Datei"}), encoding="utf-8-sig")
    code, out = run_cli(capsys, "card", "put", "demo", "--file", str(doc), "--merge")
    assert code == 0 and out["version"] == 2

    code, out = run_cli(capsys, "context", "nope")
    assert code == 1 and out == {"ok": False, "error": {"code": "project_not_found", "message": "No project with id 'nope'.", "hint": "Known project ids: demo."}}

    code, out = run_cli(capsys, "card", "put", "demo", "--json", "{broken")
    assert code == 1 and out["error"]["code"] == "invalid_json"


def test_web_api_serves_state_and_saves_owner_choices(service, make_repo):
    service.add_project(str(make_repo()))
    sid = service.put_card("demo", card())["rated_items"][0]["id"]
    client = TestClient(create_app(service, background_refresh=False))

    assert "Lookout" in client.get("/").text
    state = client.get("/api/state?visit=true").json()
    assert state["projects"][0]["card"]["headline"] == "Alles ruhig"
    assert state["projects"][0]["facts"]["is_git"] is True

    assert client.post("/api/feedback", json={"item_id": sid, "value": "later", "project_id": "demo", "title": "Dark Mode"}).status_code == 200
    assert client.post("/api/prefs", json={"key": "hidden_modules", "value": ["pulse"]}).status_code == 200
    assert client.post("/api/projects/demo/status", json={"status": "sleeping"}).status_code == 400

    state = client.get("/api/state").json()
    assert state["feedback"][sid]["value"] == "later" and state["prefs"]["hidden_modules"] == ["pulse"]
