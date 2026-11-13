//! Feature 026: where each saved sidecar came from (`subtitle_provenance`).

mod common;

use common::*;
use putio_sync_pipeline::store::Provenance;

fn row(path: &str, source: &str, bytes: i64) -> Provenance {
    Provenance {
        path: path.to_string(),
        language: "en".to_string(),
        source: source.to_string(),
        bytes,
    }
}

#[tokio::test]
async fn upsert_replaces_and_get_reads_back() {
    let env = env(vec![]).await;
    env.store
        .upsert_provenance(&row("/m/a.en.srt", "opensubtitles", 10))
        .await
        .unwrap();
    env.store
        .upsert_provenance(&row("/m/a.en.srt", "transcribed", 12))
        .await
        .unwrap();
    let got = env.store.provenance("/m/a.en.srt", None).await.unwrap();
    assert_eq!(got, Some(row("/m/a.en.srt", "transcribed", 12)));
    assert_eq!(
        env.store.provenance("/m/none.srt", None).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_size_that_differs_counts_as_unknown() {
    let env = env(vec![]).await;
    env.store
        .upsert_provenance(&row("/m/a.en.srt", "transcribed", 12))
        .await
        .unwrap();
    assert!(env
        .store
        .provenance("/m/a.en.srt", Some(12))
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .provenance("/m/a.en.srt", Some(99))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn rekey_moves_rows_under_a_folder_and_leaves_lookalikes() {
    let env = env(vec![]).await;
    for p in [
        "/stage/Show/a.en.srt",
        "/stage/Show/sub/b.en.srt",
        "/stage/Show2/c.en.srt",
    ] {
        env.store
            .upsert_provenance(&row(p, "translated", 5))
            .await
            .unwrap();
    }
    let n = env
        .store
        .rekey_provenance("/stage/Show", "/lib/Show")
        .await
        .unwrap();
    assert_eq!(n, 2);
    assert!(env
        .store
        .provenance("/lib/Show/a.en.srt", None)
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .provenance("/lib/Show/sub/b.en.srt", None)
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .provenance("/stage/Show2/c.en.srt", None)
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .provenance("/stage/Show/a.en.srt", None)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn rekey_matches_windows_backslash_paths() {
    let env = env(vec![]).await;
    for p in [
        r"C:\dl\tv\.putio-sync-staging\Show\a.en.srt",
        r"C:\dl\tv\.putio-sync-staging\Show2\c.en.srt",
    ] {
        env.store
            .upsert_provenance(&row(p, "translated", 5))
            .await
            .unwrap();
    }
    let n = env
        .store
        .rekey_provenance(r"C:\dl\tv\.putio-sync-staging\Show\", r"C:\dl\tv\Show")
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert!(env
        .store
        .provenance(r"C:\dl\tv\Show\a.en.srt", None)
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .provenance(r"C:\dl\tv\.putio-sync-staging\Show2\c.en.srt", None)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn prune_drops_rows_older_than_ninety_days() {
    let env = env(vec![]).await;
    env.store
        .upsert_provenance(&row("/m/old.srt", "translated", 1))
        .await
        .unwrap();
    env.store
        .upsert_provenance(&row("/m/new.srt", "translated", 1))
        .await
        .unwrap();
    sqlx::query("UPDATE subtitle_provenance SET created_at = strftime('%s','now') - 91*86400 WHERE path = '/m/old.srt'")
        .execute(env.db.pool())
        .await
        .unwrap();
    env.store.prune(30, 24).await.unwrap();
    assert!(env
        .store
        .provenance("/m/old.srt", None)
        .await
        .unwrap()
        .is_none());
    assert!(env
        .store
        .provenance("/m/new.srt", None)
        .await
        .unwrap()
        .is_some());
}
