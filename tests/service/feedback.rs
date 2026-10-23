use super::*;

#[test]
fn owner_reactions_reach_agents_with_what_they_were_about() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.add_project(
        common::make_repo(tmp.path(), "other", &["2026-09-28T10:00:00"])
            .to_str()
            .unwrap(),
        None,
        None,
    )
    .unwrap();
    let saved = svc
        .put_card(
            "demo",
            card_with(json!({"waiting_on_you": [{"title": "Soll die App offline laufen?"}]})),
            false,
            false,
        )
        .unwrap();
    let id_of = |kind: &str| {
        saved["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == kind)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (sid, qid) = (id_of("suggestion"), id_of("question"));
    svc.put_card("other", card(), false, false).unwrap();
    // a rating keeps what it was about and why, after the agent dropped the item, and reaches every project
    svc.set_feedback(&sid, Some("down"), Some("Keine UI-Spielereien"))
        .unwrap();
    svc.put_card("demo", card_with(json!({"suggestions": [], "waiting_on_you": [{"id": qid, "title": "Soll die App offline laufen?"}]})), false, false).unwrap();
    let ctx = svc.context("other").unwrap();
    let rating = &ctx["feedback"][0];
    assert_eq!(
        rating["item"],
        json!({"kind": "suggestion", "category": "idea", "type": "feature", "title": "Dark Mode", "why": "Weil nachts.", "evidence": "", "done_when": ""})
    );
    assert_eq!(
        (rating["project"].as_str(), rating["note"].as_str()),
        (Some("demo"), Some("Keine UI-Spielereien"))
    );
    assert_eq!(
        ctx["feedback_summary"],
        json!([{"kind": "suggestion", "type": "feature", "up": 0, "later": 0, "down": 1}])
    );
    // questions take answers, which make the card due and reach only their own project
    assert_eq!(
        svc.set_feedback(&qid, Some("up"), None).unwrap_err().code,
        "invalid_feedback"
    );
    assert_eq!(
        svc.set_feedback(&qid, Some("answered"), Some(" "))
            .unwrap_err()
            .code,
        "answer_missing"
    );
    assert_eq!(
        svc.set_feedback("sg-gone", Some("up"), None)
            .unwrap_err()
            .code,
        "unknown_item"
    );
    svc.set_feedback(&qid, Some("answered"), Some("Ja, unterwegs ohne Netz"))
        .unwrap();
    assert_eq!(
        svc.due(14, true).unwrap()["projects"],
        json!([{"id": "demo", "name": "demo", "reason": "answered"}])
    );
    assert_eq!(
        svc.state(false).unwrap()["projects"][0]["card_outdated"],
        true
    );
    assert_eq!(
        svc.context("demo").unwrap()["feedback"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == qid)
            .unwrap()["note"],
        "Ja, unterwegs ohne Netz"
    );
    assert_eq!(
        svc.context("other").unwrap()["feedback"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // An unchanged reply stays handled; withdrawing or editing it needs a new card.
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.set_feedback(&qid, Some("answered"), Some("Ja, unterwegs ohne Netz"))
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    svc.set_feedback(&qid, None, None).unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "answered"
    );
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.set_feedback(&qid, Some("answered"), Some("Doch nur online"))
        .unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "answered"
    );
    // a focus item can be marked done for the next brief
    let brief = json!({"greeting": "Moin", "focus": [{"project": "demo", "title": "Offline klären", "why": "Beantwortet."}]});
    let fid = svc.put_portfolio(brief, false, false).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.set_feedback(&fid, Some("done"), None).unwrap();
    let pctx = svc.portfolio_context(&[]).unwrap();
    assert!(pctx["feedback"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == fid && f["item"]["kind"] == "focus" && f["project"] == "demo"));
    // cards written before questions had ids get them when Watchtower opens
    let stored = svc.store().latest_card("other").unwrap().unwrap();
    let basis = watchtower::store::CardBasis {
        fingerprint: &stored.fingerprint,
        head: None,
        ci: None,
        parts: &json!(null),
        owner: None,
    };
    svc.store()
        .add_card(
            "other",
            &svc.now(),
            &basis,
            &card_with(json!({"waiting_on_you": [{"title": "Alt?"}]})),
        )
        .unwrap();
    let reopened = watchtower::Watchtower::open(&tmp.path().join("watchtower.db")).unwrap();
    assert!(
        reopened.card("other").unwrap()["card"]["waiting_on_you"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("q-")
    );
}

#[test]
fn contexts_report_how_long_risks_and_items_have_lasted() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    std::fs::write(repo.join("draft.txt"), "one\n").unwrap();
    svc.refresh_facts(None, false).unwrap();
    let question = json!([{"title": "Welches Format?", "options": [{"answer": "CSV"}, {"answer": "JSON"}], "recommended": 2}]);
    svc.put_card(
        "demo",
        card_with(json!({"waiting_on_you": question})),
        false,
        false,
    )
    .unwrap();
    let id = svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["recommended"],
        2
    );
    assert_eq!(
        svc.questions(None).unwrap()["questions"][0]["id"],
        id.as_str()
    );
    clock.advance(Duration::days(3));
    std::fs::write(repo.join("draft.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(repo.join("more.txt"), "four\n").unwrap();
    svc.refresh_facts(None, false).unwrap();
    let mut kept = card_with(json!({"waiting_on_you": [{"id": id, "title": "Welches Format?"}]}));
    kept["open_ends"] = json!([{"kind": "uncommitted", "title": "Entwurf nicht committet"}]);
    svc.put_card("demo", kept, false, false).unwrap();
    let context = svc.context("demo").unwrap();
    let streak = &context["risk_streaks"][0];
    assert_eq!(streak["kind"], "uncommitted");
    assert_eq!(streak["days"], 3);
    // The clean measurement at registration bounds the run.
    assert_eq!(streak["at_least"], false);
    assert_eq!(streak["then"]["files"], 1);
    assert_eq!(streak["now"]["files"], 2);
    assert_eq!(streak["trend"], "growing");
    let items = context["item_persistence"].as_array().unwrap();
    let asked = items.iter().find(|i| i["id"] == id.as_str()).unwrap();
    assert_eq!(
        (
            asked["kind"].as_str(),
            asked["days"].as_i64(),
            asked["versions"].as_i64()
        ),
        (Some("question"), Some(3), Some(2))
    );
    // the question was reworded today, so its current wording is new
    assert_eq!(asked["unchanged_days"], 0);
    let open_end = items.iter().find(|i| i["kind"] == "open_end").unwrap();
    assert_eq!(
        (open_end["days"].as_i64(), open_end["versions"].as_i64()),
        (Some(0), Some(1))
    );
    // Committing ends the run; the brief sees long-standing items and the owner's silence.
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "draft"]);
    clock.advance(Duration::days(5));
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(svc.context("demo").unwrap()["risk_streaks"], json!([]));
    let brief = svc.portfolio_context(&[]).unwrap();
    let standing = &brief["projects"][0]["long_standing_items"];
    assert!(standing
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == id.as_str() && i["days"] == 8 && i["unchanged_days"] == 5));
    let open = svc.questions(Some("demo")).unwrap();
    assert_eq!(
        (
            open["questions"][0]["id"].as_str(),
            open["questions"][0]["days"].as_i64()
        ),
        (Some(id.as_str()), Some(8))
    );
    assert_eq!(brief["owner_response"]["open_questions"], 1);
    assert_eq!(brief["owner_response"]["oldest_open_question_days"], 8);
    assert_eq!(brief["owner_response"]["reactions_last_14_days"], 0);
}

#[test]
fn questions_can_wait_and_cards_keep_decisions_short() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let wrong =
        json!([{"title": "Welches Format?", "options": [{"answer": "CSV"}], "recommended": 2}]);
    let error = svc
        .put_card(
            "demo",
            card_with(json!({"waiting_on_you": wrong})),
            false,
            false,
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_card");
    assert!(
        error.message.contains("waiting_on_you[0].recommended"),
        "{}",
        error.message
    );
    let four = json!((0..4)
        .map(|i| json!({"title": format!("Frage {i}?")}))
        .collect::<Vec<_>>());
    let error = svc
        .put_card(
            "demo",
            card_with(json!({"waiting_on_you": four})),
            false,
            false,
        )
        .unwrap_err();
    assert!(error.message.contains("limit is 3"), "{}", error.message);
    svc.put_card(
        "demo",
        card_with(json!({"waiting_on_you": [{"title": "Welches Format?"}]})),
        false,
        false,
    )
    .unwrap();
    let id = svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.set_feedback_until(&id, Some("later"), None, Some("2026-10-07"))
        .unwrap();
    let state = svc.state(false).unwrap();
    assert_eq!(state["feedback"][&id]["snoozed_until"], "2026-10-07");
    let open = svc.questions(None).unwrap();
    assert_eq!(
        (
            open["questions"].as_array().unwrap().len(),
            open["snoozed"].as_i64()
        ),
        (0, Some(1))
    );
    // Postponing is not an answer, so the card does not become due for it.
    assert_eq!(svc.due(30, false).unwrap()["projects"], json!([]));
}
