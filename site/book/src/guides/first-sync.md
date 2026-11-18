# Your first sync

The last part of every setup guide. putio-sync is installed and running, but has no
settings yet, so it waits in setup mode and prints a one-time **setup code**.

**You'll need:** the setup code (your platform's guide says where to find it), and a Put.io
account.

**You'll end with:** putio-sync signed in to Put.io, a download folder chosen, and your
Put.io files downloading to it.

## 1. Open the web UI

Open `http://<host>:15432/` in a browser, where `<host>` is the machine running putio-sync:
`localhost` on the same computer, or its address on your network, such as
`192.168.1.20`. The page shows **Set up putio-sync**.

Enter the setup code in **Setup code** and choose **Continue**. The code is good until setup
finishes. If you set a web UI token when installing, that works too.

## 2. Connect Put.io

Under **Connect Put.io**, paste a Put.io token:

1. Sign in at [put.io](https://put.io), and open **Settings › API**
   ([app.put.io/oauth](https://app.put.io/oauth)).
2. Create an OAuth app (any name and website will do), and copy its **OAuth token**.
3. Back in putio-sync, paste it under **Or paste a token** and choose **Use token**.

If you'd rather link with a code at put.io/link, enter the app's ID instead and choose
**Use app ID**. [Linking with a code](../using.md) explains the difference.

## 3. Choose the download folder

Under **Download folder**, enter where finished files should go and choose **Check**.
putio-sync checks it can write there and shows the free space. In a container, this is the
folder inside the container, usually `/downloads`.

## 4. Choose what to download

Under **What to download**, pick a preset (video, audio or ebooks) or leave the default,
which downloads everything except blocked file types. **Top-level folders** lists the
folders at the top of your Put.io account; set any you don't want to **Don't download**.
You can change all of this later; see [folder rules](../configuration.md).

## 5. Extras

**Extras** offers a watch folder, Prometheus metrics, and Radarr and Sonarr. They're all
optional, and you can turn them on later in Settings. Choose **Skip** to leave them for now.

## 6. Review and start

**Review** lists your choices. Choose **Dry run** to see what the first sync would
download without fetching anything. When it looks right, choose **Save and start syncing**.

putio-sync then shows your **API token** once. It signs you in to the web UI and the API.
This browser keeps it, but copy it somewhere safe for other browsers and phones. It's
saved in the config file as `[api] auth_token`.

## 7. Watch it work

The **Status** page shows the sync running, and the **Queue** page lists each file as it
downloads. Files appear in your download folder as they finish. Anything you add to Put.io
from now on is downloaded on the next sync.

## Next

- Send new torrents to Put.io from Radarr and Sonarr, or from a watch folder.
- Press <kbd>Ctrl</kbd>+<kbd>K</kbd> (<kbd>⌘</kbd>+<kbd>K</kbd> on a Mac) in the web UI to
  find any page, setting or file.
- [Web UI](../web-ui.md) explains every page, and [Configuration](../configuration.md)
  every setting.
