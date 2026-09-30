from datetime import date

from lookout import facts

TODAY = date(2026, 9, 30)


def test_facts_describe_history_backup_and_unsaved_work(make_repo):
    repo = make_repo(commits=("2026-04-01T10:00:00", "2026-09-10T10:00:00", "2026-09-29T10:00:00"))
    (repo / "f0.txt").write_text("changed\nand more\n")
    (repo / "new.txt").write_text("a\nb\nc\n")

    f = facts.collect(str(repo), TODAY, with_ci=False)

    assert f["ok"] and f["is_git"] and f["branch"] == "main"
    assert f["commits"]["total"] == 3
    assert (f["commits"]["first"], f["commits"]["last"]) == ("2026-04-01", "2026-09-29")
    assert (f["commits"]["last_7d"], f["commits"]["last_30d"], f["commits"]["last_90d"]) == (1, 2, 2)
    assert len(f["commits"]["weekly"]) == facts.WEEKS and f["commits"]["weekly"][-1] == 1 and sum(f["commits"]["weekly"]) == 2
    assert f["has_remote"] is False and f["upstream"] is None
    assert f["dirty"]["files"] == 2 and f["dirty"]["untracked"] == 1 and f["dirty"]["modified"] == 1
    assert f["dirty"]["lines_added"] == 2 + 3 and f["dirty"]["lines_removed"] == 1
    assert f["last_work"] == date.today().isoformat()  # uncommitted edits count as work


def test_fingerprint_changes_with_new_work(make_repo):
    repo = make_repo()
    before = facts.fingerprint(facts.collect(str(repo), TODAY, with_ci=False))
    assert facts.fingerprint(facts.collect(str(repo), TODAY, with_ci=False)) == before
    (repo / "f0.txt").write_text("edited\n")
    assert facts.fingerprint(facts.collect(str(repo), TODAY, with_ci=False)) != before


def test_non_repositories_report_why(tmp_path):
    plain = tmp_path / "plain"
    plain.mkdir()
    assert facts.collect(str(plain), TODAY)["error"] == "not_a_git_repository"
    assert facts.collect(str(tmp_path / "gone"), TODAY)["error"] == "path_missing"
