# Set up on macOS

**You'll need:** a Mac with Apple silicon (M1 or later), and a Put.io account.

**You'll end with:** putio-sync starting when you log in, downloading your Put.io files to a
folder on your Mac.

## 1. Install the package

1. Download `putio-sync-X.Y.Z-macos-arm64.pkg` from the
   [releases page](https://github.com/devclinton/putio-sync/releases/latest).
2. Open it. If macOS says it can't check the package, open **System Settings › Privacy &
   Security**, scroll down, and choose **Open Anyway**.
3. In the installer, choose **Customize** and leave **Start at login** ticked. Then
   **Install**.

The package installs `putio-sync` in `/usr/local/bin`, a **Put.io Sync Web UI** app in
Applications, and a login item that starts putio-sync in the background.

## 2. Find the setup code

putio-sync is already running. Its log is `~/Library/Logs/putio-sync.log`. Open it in the
Console app, or in Terminal:

```bash
tail -n 20 ~/Library/Logs/putio-sync.log
```

Look for:

```text
  putio-sync needs setting up: no config file
  Open http://localhost:15432/ and enter the setup code:

      ABCD-EFGH-JKLM
```

## 3. Run the setup wizard

Open **Put.io Sync Web UI** from Applications, or `http://localhost:15432/`. Then follow
[your first sync](first-sync.md). For the download folder, choose a folder in your home
folder, such as `/Users/<you>/Downloads/putio`, or a folder on an external disk.

## Keep it updated

When a new release is out, putio-sync shows it on its **What's new** page. Choose **Update
now** to download and install it.

## With Homebrew instead

If you manage your tools with Homebrew:

```bash
brew tap devclinton/putio-sync
brew install putio-sync
putio-sync daemon
```

The setup code is printed in that Terminal window. Homebrew doesn't start putio-sync at
login, so it stops when you close the window. The package above is simpler if you want it
running all the time. To update, run `brew update && brew upgrade putio-sync`.

## Uninstall

```bash
sudo /usr/local/share/putio-sync/uninstall.sh
```

Your settings in `~/.putiosync` and your downloads are kept.

## Next

- The Mac can serve your downloads to TVs. See [Media server](../media-server.md).
- [Web UI](../web-ui.md) explains every page.
