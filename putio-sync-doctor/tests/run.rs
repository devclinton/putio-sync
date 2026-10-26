//! Full doctor runs against a mocked Put.io (T015)

mod support;

use putio_sync_db::models::QueueStatus;
use putio_sync_doctor::{CheckId, Report, Status};
use std::time::Duration;
use support::*;

fn status(report: &Report, id: CheckId) -> Status {
    report.check(id).unwrap().status
}

#[tokio::test]
async fn a_healthy_setup_reports_no_problems() {
    let mut env = Env::new().await;
    env.healthy().await;
    let report = env.doctor.run().await;

    assert_eq!(report.checks.len(), CheckId::ALL.len());
    for id in [
        CheckId::Auth,
        CheckId::Storage,
        CheckId::Transfers,
        CheckId::EmptyFolders,
        CheckId::Duplicates,
        CheckId::OrphanedTorrents,
        CheckId::DeadQueueRows,
        CheckId::FailingDownloads,
        CheckId::OrphanPartials,
        CheckId::DownloadDir,
        CheckId::Database,
        CheckId::Reachability,
        CheckId::RateLimit,
    ] {
        assert_eq!(
            status(&report, id),
            Status::Ok,
            "{id}: {:?}",
            report.check(id)
        );
    }
    // No config file and watch off
    assert_eq!(status(&report, CheckId::Config), Status::Skipped);
    assert_eq!(status(&report, CheckId::StuckWatchFiles), Status::Skipped);
    assert_eq!(env.doctor.latest(), Some(report));
}

#[tokio::test]
async fn seeded_faults_are_reported() {
    let mut env = Env::new().await;
    env.account(200).await;
    env.transfers(&[(5, "Broken", "ERROR")]).await;
    env.folder(0, &[(78, "flaky.mkv", 2_000_000, false)]).await;
    env.gone(77).await;
    let q = env.db.queue();
    q.add(77, "gone.mkv", 10, "u", "Movies", 0).await.unwrap();
    q.add(78, "flaky.mkv", 10, "u", "Movies", 0).await.unwrap();
    q.update_status(78, QueueStatus::Failed).await.unwrap();
    partial(
        &env.downloads(),
        "Old/.left.mkv.downloading",
        Duration::from_secs(7200),
    );

    let report = env.doctor.run().await;
    assert_eq!(status(&report, CheckId::Transfers), Status::Problem);
    let dead = report.check(CheckId::DeadQueueRows).unwrap();
    assert_eq!(dead.status, Status::Warning);
    assert_eq!(dead.items[0].key, "77");
    assert_eq!(status(&report, CheckId::FailingDownloads), Status::Warning);
    assert_eq!(status(&report, CheckId::OrphanPartials), Status::Warning);
    assert_eq!(report.overall, Status::Problem);
    assert!(report.counts.problem >= 1);
}

#[tokio::test]
async fn a_rejected_token_skips_putio_checks_but_not_local_ones() {
    let mut env = Env::new().await;
    env.account(401).await;
    let report = env.doctor.run().await;
    assert_eq!(status(&report, CheckId::Auth), Status::Problem);
    for id in [
        CheckId::Storage,
        CheckId::Transfers,
        CheckId::EmptyFolders,
        CheckId::Duplicates,
        CheckId::DeadQueueRows,
    ] {
        assert_eq!(status(&report, id), Status::Skipped, "{id}");
    }
    assert_eq!(status(&report, CheckId::DownloadDir), Status::Ok);
    assert_eq!(status(&report, CheckId::Database), Status::Ok);
}

#[tokio::test]
async fn concurrent_runs_share_one_run() {
    let mut env = Env::new().await;
    let account = env
        .server
        .mock("GET", "/account/info")
        .with_body(r#"{"status":"OK","info":{"user_id":1,"username":"u","disk":{"size":1,"used":0,"avail":1}}}"#)
        .expect(1)
        .create_async()
        .await;
    env.transfers(&[]).await;
    env.folder(0, &[]).await;
    let (a, b) = tokio::join!(env.doctor.run(), env.doctor.run());
    assert_eq!(a, b);
    account.assert_async().await;
}
