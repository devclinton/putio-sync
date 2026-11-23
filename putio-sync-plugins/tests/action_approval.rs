//! Settings actions only get the hosts and folders the person approved, plus the ones in
//! the unsaved form values of the same request (E-02).

use std::path::Path;

use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::{CallError, PluginsOptions, PluginsService};
use serde_json::{json, Map, Value};

fn manifest(fixed_host: &str, fixed_dir: &str) -> String {
    format!(
        r#"
name = "probe"
version = "1"
events = ["download.finished"]

[permissions.http]
hosts = ["{fixed_host}"]
host_from_config = ["server"]

[permissions.files]
read = ["{fixed_dir}"]
read_from_config = ["folder"]

[[config]]
key = "server"
type = "url"

[[config]]
key = "folder"
type = "string"

[[actions]]
id = "get"
label = "Get"

[[actions]]
id = "read"
label = "Read"
"#
    )
}

const SCRIPT: &str = r#"
local plugin = {}
function plugin.on_event(event) end
function plugin.on_action(id, input)
  if id == "get" then
    local r, err = putio.http.request({url = input.url})
    if not r then error(err) end
    return {message = "status " .. r.status}
  end
  local t, err = putio.fs.read(input.path)
  if not t then error(err) end
  return {message = t}
end
return plugin
"#;

fn service(dir: &Path, settings: PluginSettings) -> PluginsService {
    PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        data_dir: None,
        settings: vec![settings],
        clock: None,
        download_dir: None,
        reach: Some(putio_sync_core::net_guard::Reach::Local),
    })
}

fn saved() -> PluginSettings {
    PluginSettings {
        name: "probe".to_string(),
        ..PluginSettings::default()
    }
}

fn form(pairs: &[(&str, &str)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect()
}

async fn run(
    plugins: &PluginsService,
    id: &str,
    input: Value,
    overrides: Map<String, Value>,
) -> Result<String, String> {
    match plugins.action("probe", id, input, overrides).await {
        Ok(answer) => Ok(answer.result.message.unwrap_or_default()),
        Err(CallError::Failed(run)) => Err(run.error.unwrap_or_default()),
        Err(other) => Err(other.to_string()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_action_reaches_only_approved_or_typed_hosts() {
    let mut server = mockito::Server::new_async().await;
    let hit = server
        .mock("GET", "/")
        .with_body("ok")
        .expect_at_least(1)
        .create_async()
        .await;
    let url = server.url();
    let host = reqwest::Url::parse(&url)
        .unwrap()
        .host_str()
        .unwrap()
        .to_string();
    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path().join("probe");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.toml"), manifest(&host, "/nonexistent")).unwrap();
    std::fs::write(plugin.join("main.lua"), SCRIPT).unwrap();

    // The fixed host is in the manifest but the person never approved it
    let plugins = service(dir.path(), saved());
    let input = json!({"url": url});
    let err = run(&plugins, "get", input.clone(), Map::new())
        .await
        .unwrap_err();
    assert!(err.contains("not approved"), "{err}");

    // A saved config value is not approved either
    let mut with_server = saved();
    with_server.config.insert("server".into(), json!(url));
    let plugins = service(dir.path(), with_server);
    let err = run(&plugins, "get", input.clone(), Map::new())
        .await
        .unwrap_err();
    assert!(err.contains("not approved"), "{err}");

    // The server typed in the same request is the person setting it up
    let ok = run(&plugins, "get", input.clone(), form(&[("server", &url)])).await;
    assert_eq!(ok.unwrap(), "status 200");

    // Approved hosts still work
    let mut approved = saved();
    approved.approved_hosts = vec![host];
    let plugins = service(dir.path(), approved);
    assert_eq!(
        run(&plugins, "get", input, Map::new()).await.unwrap(),
        "status 200"
    );
    hit.assert_async().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_action_reads_only_approved_or_typed_folders() {
    let secret = tempfile::tempdir().unwrap();
    std::fs::write(secret.path().join("a.txt"), "hello").unwrap();
    let folder = secret.path().canonicalize().unwrap();
    let file = folder.join("a.txt").to_string_lossy().to_string();
    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path().join("probe");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("plugin.toml"),
        manifest("example.invalid", &folder.to_string_lossy()),
    )
    .unwrap();
    std::fs::write(plugin.join("main.lua"), SCRIPT).unwrap();
    let input = json!({"path": file});

    let plugins = service(dir.path(), saved());
    assert!(run(&plugins, "read", input.clone(), Map::new())
        .await
        .is_err());

    // A folder typed into the form of this request counts
    let typed = form(&[("folder", &folder.to_string_lossy())]);
    assert_eq!(
        run(&plugins, "read", input.clone(), typed).await.unwrap(),
        "hello"
    );

    let mut approved = saved();
    approved.approved_paths = vec![format!("r:{}", folder.to_string_lossy())];
    let plugins = service(dir.path(), approved);
    assert_eq!(
        run(&plugins, "read", input, Map::new()).await.unwrap(),
        "hello"
    );
}
