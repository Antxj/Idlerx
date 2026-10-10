<p align="center">
  <img src="docs/logo.png" width="112" alt="Idlerx icon">
</p>

<h1 align="center">Idlerx</h1>

<p align="center"><b>English</b> | <a href="README.pt-BR.md">Português</a></p>

<p align="center">
  <a href="https://github.com/Antxj/Idlerx/releases/latest"><img src="https://img.shields.io/github/v/release/Antxj/Idlerx?label=Download&style=for-the-badge&color=14b8a6" alt="Download the latest version"></a>
</p>

Windows app that opens **several accounts of the same site or game at the
same time**, each one **truly isolated** (its own login, cookies, cache and
storage), organized in a single window: one at a time or all side by side in
a grid. Made for people who play browser idle games with more than one
account.

**Free and open source** (GPL-3.0). Written in **Rust**: a single executable,
no installer.

The interface is available in English and Brazilian Portuguese, following
the Windows language (it can be set under **Settings** ⚙ › Language).

## Features

- **Real isolation**: each account has its own browser profile folder. Log
  in to two accounts of the same game at the same time, no private window
  needed, and one never logs the other out
- **One process per account**: each account runs in its own browser
  process; if one crashes, the others keep going
- **One at a time or in a grid**: the grid adapts to the window size (or
  pick the number of columns). **Drag an account from the list onto a grid
  cell** to put it there
- **Focus mode** (F11): hides the list and the bar, only the game remains.
  F11 or Esc brings everything back
- **CPU and memory** of each account (in the list and on the grid cell) and
  the **total** (bottom of the list; hover it to see accounts × interface)
- **Sound and zoom per account**: mute one, zoom another (also with
  Ctrl+wheel inside the page). The 🔊 icon shows which one is playing sound
- **Address bar** with back, forward and reload: just paste the URL of any
  game
- **Favorite, rename, reorder** (by dragging in the list)
- **Pause** an account to close its page and free its memory; when opened
  again it is still logged in
- **Clear data** (log out) starts the account fresh without removing it
- **Reopens the accounts** that were open, at the last visited address
- **Automatic updates** through GitHub (details in [Updates](#updates))
- **Single instance**: opening the executable again just brings the window
  to the front

### Saving resources

- Pages that are off screen (another account selected, a menu on top)
  **stop drawing** and WebView2 is told to **use less memory**, while the
  game keeps running
- "Keep games running in the background" (on by default) stops the browser
  from slowing down the timers of hidden pages, so idle games keep
  progressing. Turn it off to save even more CPU
- The interface only repaints when something changes (or once a second for
  the numbers)
- **Pausing** accounts you are not using frees all their memory

## Requirements

- **Windows 10 or 11**
- **Microsoft Edge WebView2 Runtime**: ships with Windows 11 and is
  installed by Windows Update on Windows 10. If it is missing, Idlerx shows a
  notice with Microsoft's official link

Idlerx uses the Microsoft Edge (Chromium) engine, so sites behave as they do
in Edge or Chrome.

## How to use

1. Download `Idlerx.exe` from the [Releases](../../releases) page
2. Open it. On the first run, Windows SmartScreen may warn about an
   "unrecognized app": click **More info** › **Run anyway**
3. **+ New account**: paste the game address and give it a name (e.g. "Main
   char")
4. Repeat for the other accounts and pick **▣ Single** or **⊞ Grid** at the
   top right

Right-click an account for more options (edit, mute, hide from grid, clear
data, remove).

## For developers

Requirements: [Rust](https://rustup.rs) with the **MSVC** toolchain and
Visual Studio Build Tools. The GNU toolchain does not work (the WebView2
loader would become a separate DLL); the build stops with a message if it is
the active one.

```powershell
cd rust
rustup override set stable-x86_64-pc-windows-msvc   # only if MSVC is not your default
cargo test
cargo run
.\build-release.ps1      # tests + release without local paths + checks
```

Structure:

- [`main.rs`](rust/src/main.rs): startup and window
- [`app.rs`](rust/src/app.rs): user interface (egui): list, bar, grid, dialogs
- [`panels.rs`](rust/src/panels.rs): one open account (WebView2 through
  [webview2-com](https://github.com/wravery/webview2-rs), created
  asynchronously): position, mute, zoom, navigation
- [`config.rs`](rust/src/config.rs): accounts and settings (JSON)
- [`metrics.rs`](rust/src/metrics.rs): CPU and memory per process tree
- [`i18n.rs`](rust/src/i18n.rs): language and the `tr!`/`trf!` text macros
- [`update.rs`](rust/src/update.rs): checking for and installing new versions
- [`single.rs`](rust/src/single.rs): single instance

Useful variables for testing:

| Variable | Effect |
|---|---|
| `IDLERX_DATA` | keeps settings and profiles in another folder (the real accounts are not touched) |
| `IDLERX_INSTANCE` | separates a test instance from the everyday app |
| `IDLERX_LANGUAGE` | forces `pt` or `en` |
| `IDLERX_UPDATE_URL` | queries another address instead of the GitHub API |

## Where the data lives

- Settings and account list: `%APPDATA%\Idlerx\config.json`
- Browser profiles (cookies, cache, logins) of each account:
  `%LOCALAPPDATA%\Idlerx\profiles\`

Account data **never leaves the computer**.

## Updates

There is no dedicated server: the app queries
`api.github.com/repos/Antxj/Idlerx/releases/latest` 30 seconds after opening
and then once a day (it can be turned off under **Settings**). The request
sends no user data.

When there is a new version, a discreet link appears on the top bar. With
**Update now**:

1. the release's `Idlerx.exe` is downloaded and only accepted if it has
   exactly the SHA-256 that GitHub publishes for the file;
2. if the running executable is digitally signed, the new one must have a
   valid signature **from the same publisher**;
3. the current executable is renamed to `Idlerx.exe.antigo` (deleted on the
   next start) and the new one takes its place;
4. the app closes and the new version opens by itself, with the same
   accounts.

## Privacy

Idlerx does not collect or send any data. Its only connections are the
**sites you open** and the **version check** on GitHub (described above).

## Responsible use

Idlerx organizes browser sessions; it **does not automate actions** in games
or capture credentials. Respect each game's terms of service, including
about using several accounts.

## Uninstalling

Close the app and delete `Idlerx.exe`. To also remove the accounts and
profiles, delete the `%APPDATA%\Idlerx` and `%LOCALAPPDATA%\Idlerx` folders.

## Verifying the download

Every release is built exclusively by the
[release workflow](.github/workflows/release.yml) on GitHub Actions, from the
source code in this repository. Compare the SHA-256 of your copy with the one
shown on the [Releases](../../releases) page:

```powershell
Get-FileHash .\Idlerx.exe -Algorithm SHA256
```

And check the build provenance attestation (signed by GitHub) with the
[GitHub CLI](https://cli.github.com):

```powershell
gh attestation verify .\Idlerx.exe --repo Antxj/Idlerx
```

## License

[GPL-3.0-or-later](LICENSE). Third-party components and their licenses are
listed in [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
