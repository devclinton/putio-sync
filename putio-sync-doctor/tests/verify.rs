//! The Download verification check (feature 034)

mod support;

use putio_sync_db::NewProblem;
use putio_sync_doctor::{CheckId, Status};
use support::*;

fn problem(run_id: Option<i64>, source: &str, file_id: i64, result: &str) -> NewProblem {
    NewProblem {
        run_id,
        source: source.into(),
        file_id,
        name: format!("file{file_id}.mkv"),
        local_path: Some(format!("/downloads/file{file_id}.mkv")),
        result: result.into(),
        detail: Some("expected 1a2b3c4d, got 99999999".into()),
        queued: false,
    }
}

async fn check(env: &Env) -> putio_sync_doctor::CheckResult {
    env.doctor
        .run()
        .await
        .check(CheckId::Verification)
        .cloned()
        .unwrap()
}

#[tokio::test]
async fn verification_reports_failures_mismatches_and_fixes() {
    let mut env = Env::new().await;
    env.healthy().await;

    let found = check(&env).await;
    assert_eq!(found.status, Status::Ok, "{found:?}");
    assert_eq!(
        found.summary,
        "New downloads are checked. No verify run yet."
    );

    // A download that gave up, and a run with a mismatch and a missing file
    env.db
        .verify()
        .add_problem(&problem(None, "download", 1, "failed"))
        .await
        .unwrap();
    let run = env
        .db
        .verify()
        .start_run("everything", false, 2, 0)
        .await
        .unwrap();
    env.db
        .verify()
        .finish_run(run, "finished", None)
        .await
        .unwrap();
    for (id, result) in [(2, "mismatch"), (3, "missing")] {
        env.db
            .verify()
            .add_problem(&problem(Some(run), "run", id, result))
            .await
            .unwrap();
    }
    let found = check(&env).await;
    assert_eq!(found.status, Status::Warning, "{found:?}");
    assert!(
        found.summary.starts_with(
            "1 download gave up on a CRC32 mismatch in the last 7 days; \
             the last run found 1 file not matching Put.io. The last verify run ran at "
        ),
        "{}",
        found.summary
    );
    let mut keys: Vec<_> = found.items.iter().map(|i| i.key.clone()).collect();
    keys.sort();
    assert_eq!(keys, ["1", "2"]);

    // Both pass later
    env.db.verify().mark_fixed(1).await.unwrap();
    env.db.verify().mark_fixed(2).await.unwrap();
    let found = check(&env).await;
    assert_eq!(found.status, Status::Ok, "{found:?}");
    assert!(found
        .summary
        .starts_with("New downloads are checked. The last verify run ran at "));
}

#[tokio::test]
async fn verification_off_is_a_warning() {
    let mut env = Env::new().await;
    env.healthy().await;
    let mut config = putio_sync_core::config::Config::default();
    config.download.target_directory = env.downloads();
    config.download.verify = false;
    env.doctor.configure(&config);

    let found = check(&env).await;
    assert_eq!(found.status, Status::Warning, "{found:?}");
    assert!(found.summary.contains("not checked"));
}
