"""Writes mod/THIRD-PARTY-NOTICES.txt for a release.

The Rust crates are the same as Multiple Spirit Ashes' (checked against
Cargo.lock), so its inventory and license texts are reused, with this mod's
header and data notes.
Usage: python -I tools/notices.py <version> <msa notices> <smithbox license>
"""
import re, subprocess, sys, os

version, msa, smithbox = sys.argv[1:4]
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def packages(lock):
    s = open(lock, encoding="utf-8").read()
    return set(re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', s))


ours = {p for p in packages(os.path.join(root, "mod", "Cargo.lock")) if p[0] != "npc-spawner"}
src = open(msa, encoding="utf-8-sig").read()
listed = set(re.findall(r"^- (\S+) (\S+) — declared license", src, re.M))
# In Cargo.lock but missing from MSA's list.
EXTRA = {
    "libc": "MIT OR Apache-2.0 — https://github.com/rust-lang/libc",
    "serde_derive": "MIT OR Apache-2.0 — https://github.com/serde-rs/serde",
    "winapi-i686-pc-windows-gnu": "MIT/Apache-2.0 — https://github.com/retep998/winapi-rs",
}
missing = ours - listed
assert {p[0] for p in missing} <= set(EXTRA) and not (listed - ours), (sorted(missing), sorted(listed - ours))

body = src[src.index("PACKAGE INVENTORY"):]
for name, ver in sorted(missing):
    line = f"- {name} {ver} — declared license: {EXTRA[name]}\n"
    rows = list(re.finditer(r"^- (\S+) \S+ — declared license.*\n", body, re.M))
    at = next((m.start() for m in rows if m.group(1) > name), rows[-1].end())
    body = body[:at] + line + body[at:]
rustc = subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.split()[1]
body = re.sub(r"from rustc [0-9.]+", f"from rustc {rustc}", body)
lic = open(smithbox, encoding="utf-8-sig").read().strip()

head = f"""NPC SPAWNER
THIRD-PARTY NOTICES
Version {version} — Elden Ring 1.17, 1.17.1 and 1.17.2 (2.7.0.0, 2.7.1.0, 2.7.1.1, 2.7.2.0, 2.7.2.1)

SCOPE
This file identifies third-party software dependencies and data, and reproduces the license notices selected for this release. The package inventory is based on Cargo.lock and Cargo metadata for the x86_64-pc-windows-msvc build. It includes transitive and build-time dependencies; being listed does not mean a crate is linked into the final DLL.

Where a package declares multiple license options and MIT is one of them, this file reproduces the MIT option. For licenses requiring an additional license, those additional terms are included too. The upstream license declarations remain authoritative; this notice does not change upstream terms or state permissions for original code or assets.

DATA EMBEDDED IN NpcSpawner.dll
- NpcParam row names (English): from Smithbox by Vawser, file src/Smithbox.Data/Assets/PARAM/ER/Param Row Names/English/NpcParam.json (https://github.com/vawser/Smithbox, commit 0ec83fa85875f84133d79754b19407e89002942d, 2026-03-11), used under the MIT License reproduced below.
- Map zone names: from Smithbox 2.2.6, file Assets/Aliases/ER/MapNames.json, used under the same MIT License.
- Where each NpcParam row is placed and which AI the maps give it: a list of ids made by the author from the game's own map files. No game files are included in this mod.
Elden Ring and related trademarks and rights remain with their respective owners.

SMITHBOX LICENSE
{lic}

"""
out = os.path.join(root, "mod", "THIRD-PARTY-NOTICES.txt")
open(out, "w", encoding="utf-8", newline="\r\n").write(head + body)
print(out, len(ours), "crates")
