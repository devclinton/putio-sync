# Set up on Windows

**You'll need:** Windows 10 or 11 (x64 or Arm), and a Put.io account.

**You'll end with:** putio-sync running in the background from startup, downloading your
Put.io files to a folder on the PC.

## 1. Run the installer

1. Download `putio-sync-X.Y.Z-windows-x64-setup.exe` (or `-arm64-setup.exe` on an Arm PC)
   from the [releases page](https://github.com/devclinton/putio-sync/releases/latest).
2. Open it. If Windows SmartScreen says it protected your PC, choose **More info**, then
   **Run anyway**.
3. Under **Run putio-sync**, choose one:

| Option | When to choose it | Config and log |
|--------|-------------------|----------------|
| **In the background at startup** (default) | It runs even when nobody is signed in | `C:\ProgramData\putio-sync` |
| **When you sign in** | It runs in a minimized window while you're signed in | `%USERPROFILE%\.putiosync` |
| **Manually** | You'll run `putio-sync daemon` yourself | `%USERPROFILE%\.putiosync` |

4. Finish the installer. It opens the web UI when it's done.

## 2. Find the setup code

- **In the background at startup:** open `C:\ProgramData\putio-sync\putio-sync.log` in
  Notepad.
- **When you sign in** or **Manually:** the code is in the putio-sync window.

Look for:

```text
  putio-sync needs setting up: no config file
  Open http://localhost:15432/ and enter the setup code:

      ABCD-EFGH-JKLM
```

## 3. Run the setup wizard

Open **Put.io Sync Web UI** from the Start menu, or `http://localhost:15432/`. Then follow
[your first sync](first-sync.md). For the download folder, enter a full path such as
`D:\Media\putio`.

In the background mode, putio-sync runs as the SYSTEM account, which can't see mapped
network drives such as `Z:`. Use a local disk, or a UNC path like `\\nas\media\putio` that
the PC can reach without signing in.

## Keep it updated

When a new release is out, putio-sync shows it on its **What's new** page. Choose **Update
now** to download and run the new installer. Your settings are kept.

## Uninstall

Use **Uninstall Put.io Sync** in the Start menu, or **Settings › Apps**. Your config folder
and downloads are kept.

## Next

- The PC can serve your downloads to TVs. See [Media server](../media-server.md). Windows
  Firewall asks to allow putio-sync the first time; allow it on private networks.
- [Web UI](../web-ui.md) explains every page.
