# Testing the updater

The updater is described for users in [../user/updates.md](../user/updates.md). The code is `src/update/` (`mod.rs`: the
daily rule, `install.rs`: the install, `sha256.rs`) and `src/platform/http.rs` (the requests).

## Checking it by hand

There is no way to test a real swap in CI, so before a release that changes `src/update/` or the packaging, on each
system, install the *previous* release, publish (or point `LATEST_RELEASE_API_URL` at) a newer test release and check:

- [ ] Settings opens, the sheet appears once; reopening the same day does not show it again; the next day it does.
- [ ] **Not now**, Esc and a click outside close the sheet; About still shows the update.
- [ ] **Install update**: status goes *Downloading*, then the app quits and the new version starts (About shows the new
      number). Settings, pies and the login entry are unchanged.
- [ ] With unsaved changes in Settings the app does not quit; **Restart to update** works after saving.
- [ ] After the update, with the extension exported before and loaded: `browser-extension/` holds the new version (its
      `manifest.json`), and within a minute the extension has reloaded itself (the line under the switch no longer says
      *from another version*) without pressing **Show the extension files**. Loaded from another folder: it is asked at most
      a few times and the line says what to do.
- [ ] Linux: an old `~/.local/share/applications/cursorpie.desktop` is the new one; an edited one is left alone.
- [ ] Each reason in [the table of reasons](../user/updates.md#when-the-update-cannot-be-installed) is shown by the
      copy it names: `target/release` (build folder), macOS from the disk image and from `~/Downloads` after a first
      launch (App Translocation), an unzipped Windows download, Linux `/usr/bin/cursorpie`, `/usr/local/bin` without
      `pkexec`, and a program replaced while it runs (`cp` a newer file over it, or `mv`). The sentence names the folder
      and says what to do; **Download page** is still there.
- [ ] **Check now** after the folder was made writable (or the program restarted) turns *Download page* into *Install
      update* without reopening Settings.
- [ ] Rate limit: block `api.github.com` (hosts file or firewall) and press **Check now**: the check still finds the new
      version (from the release page) and **Install update** works; the **Details** line names the API failure.
- [ ] A download that is cut half way (kill `curl` once, or unplug and replug the network) is tried again by itself;
      three failures end with **Details** naming `curl exit ...` and **Copy details** works.
- [ ] A release whose files are still uploading (publish it as a draft with the macOS image only, then public) says *still
      being published*, not *no package for this system*.
- [ ] Offline: **Check now** says *Could not check*, nothing else changes. A tampered file (change one byte of the
      downloaded package while it sits in `updates/`) is refused as *not the file the release published*.
- [ ] A copy run from `target/release` (or an unzipped Windows download) shows **Download page**, not *Install update*.
- [ ] macOS: `/Applications` owned by an admin and by a standard user; Apple silicon and Intel. Windows: from the zip
      self-install and from Setup. Linux: `/usr/local/bin` (not writable as a normal user: **Download page**) and
      `~/.local/bin`; Ubuntu and Fedora.
