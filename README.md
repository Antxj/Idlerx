<p align="center">
  <img src="docs/logo.png" width="112" alt="Idlerx icon">
</p>

<h1 align="center">Idlerx</h1>

<p align="center"><b>All your accounts, one window.</b><br>
Several accounts of the same browser game open at once, each one isolated,
side by side, with the full uBlock Origin, a VPN per group and macros.</p>

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
windows, browser profiles, one account logging the other out, a sluggish
computer. Idlerx sorts it out:

- 🔒 **Every account truly isolated**: its own login, cookies, saves and
  passwords. Log in to several accounts of the same game at once, none logs
  the other out.
- 🪟 **Everything in one window**: one account at a time or all side by
  side in a grid.
- 🛡️ **The full uBlock Origin** on every account: no ads, trackers or
  pop-ups.
- 🌐 **A VPN per group**: each group of accounts goes out through its own
  VPN, with a kill switch, without touching the rest of the computer.
- 🖱️ **Macros that don't take over your mouse**: record by playing and the
  account repeats your clicks and keys while you keep using your PC.
- 🌙 **Always running**: hidden, minimized or in the tray, the games keep
  going.

## Features

### Accounts and organization

- **Firefox inside**: every account is a Firefox container, shown in its
  own place in Idlerx's window. Idlerx uses Firefox profiles of its own:
  your everyday Firefox does not change.
- **One at a time or in a grid**: an automatic grid that fits the window,
  or a fixed shape (side by side, stacked, 2×2, 3×2, 3×3… or any columns
  and rows). Drag an account from the list onto a grid cell.
- **Groups**: sort accounts by game, server or anything you like, each
  with a color if you want. Click a group to see only its accounts in the
  grid; open or pause a whole group at once; drag accounts and groups to
  rearrange them.
- **Shortcuts**: Ctrl+1…9 shows the account at that place of the list, also
  from inside a game. F11 is focus mode: just the game, no list or bar.
- **Sound and zoom per account**, address bar, favorites, rename, drag to
  reorder.
- **Saved passwords per account**: Idlerx offers to save each login and
  fills it in next time. One account never sees another's passwords. They
  stay on your computer, encrypted by Windows, never synced anywhere.
- **Pause** an account to free its memory; it is still logged in when you
  open it again.
- **InPrivate mode**, for a whole group or a single account: the account
  keeps nothing (cookies, logins, passwords, site data) and always opens on
  a blank page. Pausing it or closing Idlerx wipes everything. Note: games
  that save progress in the browser lose it.
- **Picks up where you left off**: the accounts that were open come back at
  their last address.

<p align="center"><img src="docs/en/grid_menu.png" width="420" alt="Grid shape picker"></p>

### Ad and tracker blocker

[uBlock Origin](https://github.com/gorhill/uBlock), the full extension, is
installed in every account, InPrivate included, with its default lists kept
up to date by itself. Ads, trackers, malware sites and pop-ups are blocked.
The shield 🛡 in an account's header opens uBlock Origin's own panel for
that page: turn blocking off on a site, or pick an element (a stubborn
banner) and remove it.

### A VPN per group

Choose an OpenVPN `.ovpn` file for a group and its accounts reach the
internet **only through the VPN**:

- **Kill switch**: if the VPN drops, the group's accounts stay offline until
  it is back. They never go out through your own connection.
- **No leaks**: names resolved through the VPN, WebRTC without revealing
  your IP, IPv6 blocked.
- **The rest of your PC is untouched**: only that group uses the VPN; other
  groups and your other programs keep your regular connection. Each group
  with a VPN has a Firefox of its own.
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

### Light and always running

- **Frame limit**: Firefox itself draws at most 30 frames per second by
  default (adjustable). Idle games do not need more, and it saves a lot of
  CPU and GPU.
- **Economy mode** (on by default): fewer Firefox processes for all the
  accounts.
- **Off screen, nothing wasted**: hidden accounts don't draw, but the game
  keeps going: timers at full speed and the game loop running at 5 frames
  per second.
- **System tray**: the X sends it to the tray and the games keep going. The
  icon gets a red dot if a group's VPN drops, and Windows tells you when it
  drops and when it is back.
- **Self-healing**: if an account's Firefox closes, the account opens again
  by itself.
- **CPU and RAM** of the whole app, of each Firefox and of each group with
  a VPN, always in sight.
- **Pages log**: when each account loaded, was hidden or shown, and what
  Firefox did. Handy to understand what happened while you were away.

<p align="center"><img src="docs/en/settings.png" width="420" alt="Settings"></p>

## Get started in a minute

1. **Download** `Idlerx.exe` from the [Releases](../../releases/latest)
   page.
2. **Open it**. The first time, Windows may show "unrecognized app": click
   **More info** › **Run anyway**.
3. If Firefox is not installed, click **Install Firefox** (Mozilla's
   official installer).
4. **+ New account**: paste the game address and give it a name.
5. Repeat for your other accounts and pick **Single** or **Grid** at the
   top right.

Right-clicking an account or a group shows every option.

**Requirements:** Windows 10 or 11 and Firefox 128 or newer. The interface
is in English and Brazilian Portuguese, following the Windows language.

### Coming from Idlerx 0.x

Versions up to 0.10 used the Microsoft Edge engine (WebView2), whose logins
do not come over to Firefox: log in to each account once more. Games that
keep progress only in the browser start over, so export your saves in the
previous version first, if the game has that option. The old Edge data
stays until you delete it in **Settings › Delete Edge data**.

## FAQ

**Is it really free?**
Yes, and it always will be: no ads, no paid version, no sign-up. The source
is open (GPL-3.0).

**Does it change my Firefox?**
No. Idlerx uses Firefox profiles of its own, in its own folder. Your
everyday Firefox, its tabs, add-ons and settings are not touched.

**Where do my accounts and passwords live?**
Only on your computer. Each account's logins stay in its container, and
saved passwords, VPN passwords and tokens are encrypted by Windows. Nothing
goes to any server.

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
Only Firefox, which Idlerx offers to install. OpenVPN is only needed if you
use a VPN in your groups, and Idlerx offers to install it too.

## Privacy

Idlerx does not collect or send any data. Its only connections are the
**sites you open**, the **version check** on GitHub (with none of your
data), downloading **uBlock Origin** once from Mozilla's add-ons site, and,
in groups with a VPN, **your VPN server** and one request to
`api.ipify.org` **through the VPN itself**, to show the IP the sites see.

In Idlerx's Firefox profiles, telemetry, studies, sponsored content and
suggestions are turned off. Firefox still checks its own updates,
blocklists and certificates, and uBlock Origin keeps its lists up to date.

## Security

Every version is built exclusively by the
[release workflow](.github/workflows/release.yml) on GitHub Actions, from
this repository's source. Idlerx's Firefox extension is signed by Mozilla
in the same workflow. To check your copy:

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

- Settings, accounts, saved passwords and the pages log: `%APPDATA%\Idlerx\`
- Firefox profiles (logins, cookies, cache) of the accounts:
  `%LOCALAPPDATA%\Idlerx\profiles\firefox\`

**Uninstall:** close Idlerx and delete `Idlerx.exe`. To also remove the
accounts, delete the `%APPDATA%\Idlerx` and `%LOCALAPPDATA%\Idlerx` folders
and the registry key
`HKEY_CURRENT_USER\Software\Mozilla\NativeMessagingHosts\io.github.antxj.idlerx`.

## License

Free and open source, under the [GPL-3.0-or-later](LICENSE). Third-party
components and their licenses are listed in
[THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
