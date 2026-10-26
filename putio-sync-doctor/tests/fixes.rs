//! Every fix: preview, validation, re-checks and the follow-up check (T020)

mod support;

use mockito::Matcher;
use putio_sync_db::models::QueueStatus;
use putio_sync_doctor::{FixError, FixId, FixRequest, Status};
use std::time::Duration;
use support::*;

fn req(items: &[&str]) -> FixRequest {
    FixRequest {
        items: items.iter().map(|s| s.to_string()).collect(),
        confirm: true,
    }
}

#[tokio::test]
async fn a_destructive_fix_needs_confirm_and_calls_nothing_without_it() {
    let mut env = Env::new().await;
    let any = env
        .server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let mut r = req(&["77"]);
    r.confirm = false;
    let err = env
        .doctor
        .apply(FixId::DeadQueueRowsRemove, &r)
        .await
        .unwrap_err();
    assert!(matches!(err, FixError::Invalid(_)), "{err:?}");
    let empty = env
        .doctor
        .apply(FixId::TransfersRetry, &req(&[]))
        .await
        .unwrap_err();
    assert!(matches!(empty, FixError::Invalid(_)));
    any.assert_async().await;
}

#[tokio::test]
async fn dead_queue_rows_are_removed_and_stale_keys_skipped() {
    let mut env = Env::new().await;
    env.healthy().await;
    env.gone(77).await;
    env.db
        .queue()
        .add(77, "gone.mkv", 10, "u", "Movies", 0)
        .await
        .unwrap();

    let preview = env.doctor.preview(FixId::DeadQueueRowsRemove).await;
    assert!(preview.destructive);
    assert_eq!(preview.items.len(), 1);
    assert_eq!(preview.items[0].key, "77");

    let out = env
        .doctor
        .apply(FixId::DeadQueueRowsRemove, &req(&["77", "999"]))
        .await
        .unwrap();
    assert_eq!(out.done.len(), 1);
    assert_eq!(out.skipped.len(), 1);
    assert_eq!(out.skipped[0].item, "999");
    assert!(env.db.queue().get_by_file_id(77).await.unwrap().is_none());
    assert_eq!(out.check.status, Status::Ok);
}

#[tokio::test]
async fn failed_downloads_are_requeued() {
    let env = Env::new().await;
    let q = env.db.queue();
    q.add(78, "flaky.mkv", 10, "u", "Movies", 0).await.unwrap();
    q.update_status(78, QueueStatus::Failed).await.unwrap();

    let preview = env.doctor.preview(FixId::FailingDownloadsRequeue).await;
    assert!(!preview.destructive);
    assert_eq!(preview.items.len(), 1);

    let out = env
        .doctor
        .apply(
            FixId::FailingDownloadsRequeue,
            &FixRequest {
                items: vec!["78".into()],
                confirm: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(out.done.len(), 1);
    let row = q.get_by_file_id(78).await.unwrap().unwrap();
    assert_eq!(row.status, QueueStatus::Pending.to_string());
    assert_eq!(out.check.status, Status::Ok);
}

#[tokio::test]
async fn only_errored_transfers_are_retried() {
    let mut env = Env::new().await;
    env.account(200).await;
    env.transfers(&[(5, "Broken", "ERROR"), (6, "Fine", "COMPLETED")])
        .await;
    let retry = env
        .server
        .mock("POST", "/transfers/retry")
        .with_body(r#"{"status":"OK","transfer":{}}"#)
        .expect(1)
        .create_async()
        .await;

    let out = env
        .doctor
        .apply(FixId::TransfersRetry, &req(&["5", "6"]))
        .await
        .unwrap();
    assert_eq!(out.done.len(), 1);
    assert_eq!(out.done[0].key, "5");
    assert_eq!(out.skipped.len(), 1);
    assert_eq!(out.skipped[0].item, "6");
    retry.assert_async().await;
}

#[tokio::test]
async fn orphan_partials_are_deleted_unless_changed() {
    let env = Env::new().await;
    let root = env.downloads();
    let old = partial(&root, "A/.old.mkv.downloading", Duration::from_secs(7200));
    let touched = partial(&root, "B/.busy.mkv.downloading", Duration::from_secs(7200));

    let preview = env.doctor.preview(FixId::OrphanPartialsDelete).await;
    let keys: Vec<&str> = preview.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["A/.old.mkv.downloading", "B/.busy.mkv.downloading"]
    );

    // Written to after the preview: no longer stale
    std::fs::write(&touched, b"more bytes").unwrap();
    let out = env
        .doctor
        .apply(
            FixId::OrphanPartialsDelete,
            &req(&[
                "A/.old.mkv.downloading",
                "B/.busy.mkv.downloading",
                "../outside.downloading",
            ]),
        )
        .await
        .unwrap();
    assert_eq!(out.done.len(), 1);
    assert!(!old.exists());
    assert!(touched.exists());
    assert_eq!(out.skipped.len(), 2);
}

#[tokio::test]
async fn empty_folder_cleanup_refuses_a_changed_list() {
    let mut env = Env::new().await;
    env.account(200).await;
    env.transfers(&[]).await;
    env.folder(0, &[(50, "Empty", 0, true)]).await;
    env.folder(50, &[]).await;

    let preview = env.doctor.preview(FixId::EmptyFoldersClean).await;
    let keys: Vec<&str> = preview.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, vec!["50"]);

    let no_delete = env
        .server
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;
    let err = env
        .doctor
        .apply(FixId::EmptyFoldersClean, &req(&["51"]))
        .await
        .unwrap_err();
    assert!(matches!(err, FixError::Conflict(_)), "{err:?}");
    no_delete.assert_async().await;
    no_delete.remove_async().await;

    let delete = env
        .server
        .mock("POST", "/files/delete")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;
    let out = env
        .doctor
        .apply(FixId::EmptyFoldersClean, &req(&["50"]))
        .await
        .unwrap();
    assert_eq!(out.done.len(), 1);
    delete.assert_async().await;
}
