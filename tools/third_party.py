"""Regenerates THIRD-PARTY-LICENSES.txt from the Rust dependency tree.

Run from the repository root after changing dependencies:
  py tools/third_party.py
"""

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

HEADER = """Idlerx - third-party components
================================

Idlerx itself is licensed under the GNU General Public License v3.0 or later
(see LICENSE). The executable includes the components below, each under its
own license. All of them are compatible with the GPL-3.0.

Mozilla Firefox
---------------
Idlerx shows the pages with the Firefox installed on the computer, which is
NOT redistributed with Idlerx (when it is missing, Idlerx offers to download
the official installer from Mozilla, checking Mozilla's signature). Firefox
is licensed under the Mozilla Public License 2.0:
https://www.mozilla.org/MPL/2.0/
"Firefox" is a trademark of the Mozilla Foundation. Idlerx is not produced,
sponsored or endorsed by Mozilla.

uBlock Origin
-------------
Idlerx installs uBlock Origin (by Raymond Hill and contributors) in its own
Firefox profiles, downloaded from Mozilla's add-ons site
(https://addons.mozilla.org/firefox/addon/ublock-origin/) and signed by
Mozilla. It is NOT embedded in the executable. uBlock Origin is licensed
under the GNU General Public License v3.0: https://github.com/gorhill/uBlock

Idlerx's Firefox extension
--------------------------
The extension that connects Idlerx to its Firefox (firefox/extension in the
repository) is part of Idlerx, under the same GPL-3.0-or-later license.

SQLite
------
Built into the executable (through the rusqlite crate) to read the previous
version's data. SQLite is in the public domain: https://sqlite.org/copyright.html

OpenVPN Community Edition
-------------------------
Idlerx.exe embeds the official OpenVPN Community installer
(OpenVPN-2.7.6-I001-amd64.msi), redistributed without any modification,
exactly as published by OpenVPN Inc. at:

    https://build.openvpn.net/downloads/releases/

The package is digitally signed by "OpenVPN Inc." and its integrity
(SHA-256) is verified on every release build. It is installed only when
the user asks for it (VPN of a group).

OpenVPN is free software, licensed under the GNU General Public License
version 2 (GPLv2): https://github.com/OpenVPN/openvpn/blob/master/COPYING
Source code of the distributed binary:
https://build.openvpn.net/downloads/releases/openvpn-2.7.6.tar.gz

Idlerx is an independent program: it does not link to or incorporate
OpenVPN code. It runs openvpn.exe as a separate process (through the
OpenVPN interactive service) and talks to it through its management
interface, a local socket.

"OpenVPN" is a registered trademark of OpenVPN Inc. Idlerx is not
produced, sponsored or endorsed by OpenVPN Inc.

Fonts
-----
The interface fonts come with egui (epaint_default_fonts): Ubuntu Light
(Ubuntu Font Licence 1.0), Noto Emoji (SIL Open Font License 1.1) and
emoji-icon-font (SIL Open Font License 1.1).

Rust crates
-----------
name version - license - repository
"""


def main() -> None:
    out = subprocess.run(
        [
            "cargo", "tree", "-e", "normal",
            "--target", "x86_64-pc-windows-msvc",
            "--prefix", "none",
            "--format", "{p}|{l}|{r}",
        ],
        cwd=ROOT / "rust",
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    rows = set()
    for line in out.splitlines():
        line = line.replace(" (*)", "").strip()
        if not line or line.startswith("idlerx "):
            continue
        pkg, lic, repo = (line.split("|") + ["", ""])[:3]
        rows.add(f"{pkg} - {lic or 'see repository'} - {repo}".rstrip(" -"))
    text = HEADER + "\n".join(sorted(rows, key=str.lower)) + "\n"
    (ROOT / "THIRD-PARTY-LICENSES.txt").write_text(text, encoding="utf-8", newline="\n")
    print(f"{len(rows)} crates")


if __name__ == "__main__":
    main()
