<p align="center">
  <img src="docs/logo.png" width="112" alt="Idlerx icon">
</p>

<h1 align="center">Idlerx</h1>

<p align="center"><b>All your accounts, one window.</b><br>
Several accounts of the same browser game open at once, each one isolated,
side by side, lightweight, with a VPN per group and macros.</p>

<p align="center"><b>English</b> | <a href="README.pt-BR.md">Português</a></p>

<p align="center">
  <a href="https://github.com/Antxj/Idlerx/releases/latest"><img src="https://img.shields.io/github/v/release/Antxj/Idlerx?label=Free%20download&style=for-the-badge&color=14b8a6" alt="Download the latest version"></a>
</p>

<p align="center">
  Free · Open source · No ads · No sign-up · A single file, nothing to install
</p>

| Dark theme | Light theme |
|---|---|
| ![Four accounts in a 2×2 grid, dark theme](docs/en/home_dark.png) | ![The same grid, light theme](docs/en/home_light.png) |

<sub>Games in the pictures: [A Dark Room](https://github.com/doublespeakgames/adarkroom),
[Trimps](https://github.com/Trimps/Trimps.github.io) and
[Candy Box 2](https://github.com/candybox2/candybox2.github.io), all open
source. Two "Dark Room" accounts run side by side, each with its own save.</sub>

## Why Idlerx

Playing with several accounts in a browser is usually a mess: private
windows, Chrome profiles, one account logging the other out, a sluggish
computer. Idlerx sorts it out:

- 🔒 **Every account truly isolated**: its own login, cookies and progress.
  Log in to several accounts of the same game at once, none logs the other
  out.
- 🪟 **Everything in one window**: one account at a time or all side by
  side in a grid.
- 🌐 **A VPN per group**: each group of accounts goes out through its own
  VPN, with a kill switch, without touching the rest of the computer.
- 🖱️ **Macros that don't take over your mouse**: record by playing and the
  account repeats your clicks and keys while you keep using your PC.
- 🛡️ **No ads or trackers**: a built-in blocker with the same lists as
  uBlock Origin, on every account.
- 🍃 **Lightweight**: games don't need 60 frames a second or all your
  memory.
- 🌙 **Always running**: minimize it or send it to the tray and the games
  keep going.

## Features

### Accounts and organization

- **One at a time or in a grid**: an automatic grid that fits the window,
  or a fixed shape (side by side, stacked, 2×2, 3×2, 3×3… or any columns
  and rows). Drag an account from the list onto a grid cell.
- **Groups**: sort accounts by game, server or anything you like. Click a
  group to see only its accounts in the grid; open or pause a whole group
  at once; drag accounts and groups to rearrange them. Give a group a
  color and its accounts carry it in the list and in the grid.
- **Focus mode** (F11): just the game, no list or bar.
- **Ctrl+1…9**: jumps to that account of the list.
- **Sound and zoom per account**, address bar, favorites, rename, drag to
  reorder.
- **Saved passwords per account**, encrypted by Windows, never synced
  anywhere.
- **Pause** an account to free all its memory; it is still logged in when
  you open it again.
- **InPrivate mode**, for a whole group or a single account: the account
  keeps nothing (cookies, logins, passwords, site data) and always opens on
  a blank page. Pausing it or closing Idlerx wipes everything. Note: games
  that save progress in the browser lose it.
- **Picks up where you left off**: the accounts that were open come back at
  their last address.

<p align="center"><img src="docs/en/grid_menu.png" width="420" alt="Grid shape picker"></p>

### A VPN per group

Choose an OpenVPN `.ovpn` file for a group and its accounts reach the
internet **only through the VPN**:

- **Kill switch**: if the VPN drops, the group's accounts stay offline until
  it is back. They never go out through your own connection.
- **No leaks**: DNS through the VPN, WebRTC without revealing your IP, IPv6
  blocked. Checked on browserleaks.com.
- **The rest of your PC is untouched**: only that group uses the VPN; other
  groups and your other programs keep your regular connection.
- **One VPN per group, several at the same time.**
- Login by certificate, username and password, or a **Google
  Authenticator token** (the 6-digit code is generated for you).
- The group shows the **IP** the sites see and its **download and upload**.
  When Idlerx opens, the accounts wait for the VPN before loading.
- The official OpenVPN installer is included: if it is missing, it is one
  click away.

Right-click the group › **Edit (name, VPN)…**

### Macros

Repeat clicks and keys on an account **without using the Windows mouse or
keyboard**: you keep using your computer normally, and the macro works even
with the account hidden, minimized or in the tray.

- **Record it by playing**: click **Record a macro** and use the game
  normally. Your clicks, keys and the time between them become the macro,
  ready to adjust.
- **Or build it on a picture of the page**: click where the macro should
  click. Left or right button, single or double.
- **Keys and waits**: letters, digits, F1–F12, arrows, Space, Enter, Esc…
- **Repetition**: every so many seconds, minutes or hours ("50 min",
  "1h30"), a number of times or until you stop it. The account's line in
  the list counts down to the next click.
- **Points relative to the page**: they stay in place if the cell changes
  size.
- **Stops by itself** if the page reloads, the account closes or the VPN
  drops. And the tray icon has **Stop all macros**.

Right-click the account › **Record a macro** or **Macro…**

### Ad and tracker blocker

Built in and **on by default for every account**, InPrivate included.

- **The Brave browser's engine** ([adblock-rust](https://github.com/brave/adblock-rust))
  with **the lists uBlock Origin uses by default**: uBlock filters,
  EasyList, EasyPrivacy and Peter Lowe's list.
- **Blocks** ad scripts, trackers and malware, and **hides the spots** where
  ads would be.
- **uBlock Origin's scripts and stand-ins**: the rules that run a small
  script in the page (against pop-unders, ad timers and "turn off your
  blocker" checks) work as in uBlock Origin, and an ad library a game
  needs gets a harmless stand-in instead of breaking the game.
- **Block element**: an ad got through? The account's 🛡 button ›
  **Block element**, point at it and click: it is gone from that site for
  good (Settings › **Hidden elements** brings it back).
- **Per account**: the 🛡 button shows how much was blocked there and
  turns blocking off (or on again) for that site with one click.
- **No pop-ups**: ad windows, and any window a site tries to open on its
  own, without a click, are not opened. Windows you open by clicking (a
  login, a link) work normally.
- **Lists always up to date**: downloaded by themselves and refreshed once
  a day.
- **Allowed sites**: a list of sites where nothing is blocked, in case a
  game complains.
- Fewer ads also means less CPU, memory and bandwidth used.

Settings › **Block ads and trackers**.

### Lightweight and always running

- **Frame limit**: 60 FPS on the selected account and 30 on the others by
  default (adjustable, per account too). With four animated games in the
  grid, CPU use went from 25% to 6%.
- **Economy mode** (on by default): the accounts share one browser, using
  about 40% less memory.
- **Off screen, nothing wasted**: hidden accounts don't draw and use less
  memory, but the game keeps going: timers at full speed and the game loop
  running at 5 frames per second. Minimized, Idlerx uses about ten times
  less CPU.
- **Muted, no sound work**: new accounts start muted, and a muted account's
  sound is stopped, not just silenced, so it costs nothing (music comes back
  when you unmute).
- **System tray**: the X sends it to the tray and the games keep going. The
  icon gets a red dot if a group's VPN drops, and Windows tells you when it
  drops and when it is back.
- **Self-healing**: a page that crashes or freezes reloads by itself.
- **CPU and RAM** of each account, each group and the total, always in
  sight.
- **Pages log**: when each account loaded, reloaded (and why), was hidden or
  shown. Handy to understand what happened while you were away.

<p align="center"><img src="docs/en/settings.png" width="420" alt="Settings"></p>

## Get started in a minute

1. **Download** `Idlerx.exe` from the [Releases](../../releases/latest)
   page.
2. **Open it**. The first time, Windows may show "unrecognized app": click
   **More info** › **Run anyway**.
3. **+ New account**: paste the game address and give it a name.
4. Repeat for your other accounts and pick **Single** or **Grid** at the
   top right.

Right-clicking an account or a group shows every option.

**Requirements:** Windows 10 or 11. Idlerx uses the Microsoft Edge engine
(WebView2), which ships with Windows 11 and comes through Windows Update on
Windows 10. Sites work as they do in Edge or Chrome. The interface is in
English and Brazilian Portuguese, following the Windows language.

## FAQ

**Is it really free?**
Yes, and it always will be: no ads, no paid version, no sign-up. The source
is open (GPL-3.0).

**Where do my accounts and passwords live?**
Only on your computer. Each account's logins stay in its own profile
folder, and VPN passwords and tokens are encrypted by Windows. Nothing goes
to any server.

**Does Idlerx update itself?**
It checks for a new version right after opening and then every hour, and
shows a discreet notice. When updating, the new file is only accepted if
its checksum (SHA-256) matches the one GitHub publishes, and Idlerx reopens
by itself with the same accounts.

**Why does Windows warn me the first time?**
SmartScreen warns about programs that are not downloaded often yet. Every
version is built straight from this repository's source, and you can check
it: see [Security](#security).

**Do I need to install anything?**
No. It is a single file. OpenVPN is only needed if you use a VPN in your
groups, and Idlerx offers to install it.

## Privacy

Idlerx does not collect or send any data. Its only connections are the
**sites you open**, the **version check** on GitHub (with none of your
data), the daily download of the **ad blocker's lists** (from
`ublockorigin.github.io`, `easylist.to` and `pgl.yoyo.org`, with none of
your data; with the blocker off nothing is downloaded) and, in groups with a
VPN, **your VPN server** and one request to `api.ipify.org` **through the
VPN itself**, to show the IP the sites see.

## Security

Every version is built exclusively by the
[release workflow](.github/workflows/release.yml) on GitHub Actions, from
this repository's source. To check your copy:

```powershell
Get-FileHash .\Idlerx.exe -Algorithm SHA256
gh attestation verify .\Idlerx.exe --repo Antxj/Idlerx
```

Compare the SHA-256 with the one on the [Releases](../../releases) page;
the second command checks the build provenance attestation signed by GitHub
(it needs the [GitHub CLI](https://cli.github.com)).

## Responsible use

Idlerx organizes browser sessions and has macros that repeat clicks and
keys you set yourself; it does not capture credentials. Many games forbid
automation and using several accounts: check each game's terms of service
before using it.

## Where the data lives

- Settings, accounts and the pages log: `%APPDATA%\Idlerx\`
- Browser profiles (logins, cookies, cache) of each account:
  `%LOCALAPPDATA%\Idlerx\profiles\`

**Uninstall:** close Idlerx and delete `Idlerx.exe`. To also remove the
accounts, delete the `%APPDATA%\Idlerx` and `%LOCALAPPDATA%\Idlerx` folders.

## License

Free and open source, under the [GPL-3.0-or-later](LICENSE). Third-party
components and their licenses are listed in
[THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
