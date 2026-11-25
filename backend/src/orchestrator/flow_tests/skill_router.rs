//! Library skills reach a session only through `janus-skills`
//! (docs/domains/20261116-resource_library.md#skill_router): the agent loads the router from the
//! bundle `session/new` hands it (no system-layer pointer), finds a skill in its catalog and reads
//! that skill's SKILL.md, and Janus records the read (`skill.used`). The library counts it, and the next run's catalog puts
//! the skill first.

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_skill_found_in_the_catalog_is_read_recorded_and_counted() {
    let h = harness("skill-router", &[("lead", false), ("m1", false)], json!({ "T1": ["skill:notes"] }));
    let notes = h
        .app
        .create_skill(&json!({ "name": "notes", "description": "把会议记录整理成决定和待办。在用户要整理会议记录时使用", "content": "写下来。" }))
        .await
        .unwrap();
    let alpha = h
        .app
        .create_skill(&json!({ "name": "alpha", "description": "写周报", "content": "照模板写。" }))
        .await
        .unwrap();

    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    let evs = events(&h.app, &run);
    // No system layer points at the catalog: the agent loads it natively from its bundle.
    assert!(
        !evs.iter().any(|e| e.kind.as_str() == "session.prompt" && e.data.as_ref().unwrap()["channel"] == "skills"),
        "no skills pointer"
    );
    let router = h.app.cfg.layout.run(RunKind::Goal, &run).bundle("m1").join("skills/janus-skills/SKILL.md");
    let md = std::fs::read_to_string(&router).unwrap();
    assert!(
        md.contains("description: 每项任务开始时，先读这份目录"),
        "the run's language, when to read it first: {md}"
    );
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(calls.contains("skill notes read"), "the agent found notes in the catalog: {calls}");

    // Reading the router is not using a skill; reading notes' SKILL.md is, once.
    let used: Vec<&Event> = evs.iter().filter(|e| e.kind.as_str() == "skill.used").collect();
    assert_eq!(used.len(), 1, "{used:?}");
    assert_eq!((used[0].actor.as_str(), used[0].task.as_deref()), ("m1", Some("T1")));
    assert_eq!(used[0].data.as_ref().unwrap()["item"], json!(notes.id));

    // The library counts the session, and points at what keeps a skill from being found.
    let lib = h.app.library_view().unwrap();
    let item = |id: &str| lib["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap().clone();
    assert_eq!(item(&notes.id)["uses"]["sessions"], 1);
    assert!(item(&notes.id)["uses"]["lastAt"].is_i64());
    assert_eq!(item(&notes.id)["hints"], json!([]));
    assert_eq!(item(&alpha.id)["uses"], json!({ "sessions": 0, "lastAt": null }));
    assert_eq!(item(&alpha.id)["hints"], json!([{ "kind": "short" }]));
    assert_eq!(lib["skillRouter"], json!({ "catalog": "list" }));

    // The next run's catalog lists the used skill first, though alpha comes first by name.
    let team = h.app.run(&run).unwrap().team.clone();
    let next = h.app.cfg.layout.run(RunKind::Goal, "next");
    h.app.bundle_team(&next, &team, crate::lang::Lang::Zh).unwrap();
    let md = std::fs::read_to_string(next.bundle("m1").join("skills/janus-skills/SKILL.md")).unwrap();
    assert!(md.find("- notes：").unwrap() < md.find("- alpha：").unwrap(), "{md}");
    // A run in English gets its catalog in English.
    let english = h.app.cfg.layout.run(RunKind::Goal, "english");
    h.app.bundle_team(&english, &team, crate::lang::Lang::En).unwrap();
    let md = std::fs::read_to_string(english.bundle("m1").join("skills/janus-skills/SKILL.md")).unwrap();
    assert!(md.contains("description: At the start of each task, read this catalog"), "{md}");
    assert!(md.find("- notes: ").unwrap() < md.find("- alpha: ").unwrap(), "{md}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
