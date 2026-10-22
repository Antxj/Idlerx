# Security policy

## Supported versions

Only the latest release published on the
[Releases](https://github.com/Antxj/Idlerx/releases) page receives fixes. The
app checks for new versions by itself and can update with one click.

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Report them
privately through GitHub instead: open the repository's **Security** tab and
click **Report a vulnerability**.

Include what you found, how to reproduce it and the app version (shown under
**Settings** ⚙). You will get an answer as soon as possible, and the fix will
be credited to you in the release notes if you wish.

## Scope

Of particular interest:

- isolation between accounts (one account's cookies, storage or login
  reaching another);
- the browser profiles in `%LOCALAPPDATA%\Idlerx\profiles` and the settings
  in `%APPDATA%\Idlerx\config.json`;
- the update mechanism (download, SHA-256 and signature checks, executable
  replacement);
- the link between Idlerx and its Firefox (the extension in
  `firefox/extension`, native messaging and the local socket with a one-time
  key);
- the saved passwords of the accounts (`%APPDATA%\Idlerx\passwords.dat`,
  encrypted by Windows).

Vulnerabilities in Firefox itself should be reported to Mozilla
(https://www.mozilla.org/security/), and in uBlock Origin to its project
(https://github.com/gorhill/uBlock).

---

**Português:** não abra issue pública para problemas de segurança. Use a aba
**Security** › **Report a vulnerability** deste repositório.
