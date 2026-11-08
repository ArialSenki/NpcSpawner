"""Build release\\NpcSpawner-<version>-ER-1.17-1.17.1-1.17.2.zip.

Usage: python -I package.py [target_dir]
Layout (as NativeTweaks): DLLs/NpcSpawner.dll, DLLs/NpcSpawner.ini,
README.md, README.es.md, THIRD-PARTY-NOTICES.txt
"""
import hashlib
import os
import re
import sys
import zipfile

ROOT = os.path.dirname(os.path.abspath(__file__))
MOD = os.path.join(ROOT, "mod")
target = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.environ["LOCALAPPDATA"], "NPCTestArena-build")
dll = os.path.join(target, "x86_64-pc-windows-msvc", "release", "npc_spawner.dll")

version = re.search(r'^version = "([^"]+)"', open(os.path.join(MOD, "Cargo.toml")).read(), re.M).group(1)
dll_bytes = open(dll, "rb").read()
assert f"NPC Spawner {version}".encode() in dll_bytes, "the DLL is not this version"
# Built with build.py: no folder of this machine inside (panic messages).
for local in {ROOT, os.path.expanduser("~"), os.environ.get("CARGO_HOME", "")} - {""}:
    assert local.encode() not in dll_bytes, f"the DLL holds the local path {local}: build it with build.py"
for f in ("README.md", "README.es.md"):
    assert f"NPC Spawner {version}" in open(os.path.join(MOD, f), encoding="utf-8").read(), f
files = [
    ("DLLs/NpcSpawner.dll", dll),
    ("DLLs/NpcSpawner.ini", os.path.join(MOD, "NpcSpawner.ini")),
    ("README.md", os.path.join(MOD, "README.md")),
    ("README.es.md", os.path.join(MOD, "README.es.md")),
    ("THIRD-PARTY-NOTICES.txt", os.path.join(MOD, "THIRD-PARTY-NOTICES.txt")),
]
out_dir = os.path.join(ROOT, "release")
os.makedirs(out_dir, exist_ok=True)
out = os.path.join(out_dir, f"NpcSpawner-{version}-ER-1.17-1.17.1-1.17.2.zip")
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for name, src in files:
        z.write(src, name)


def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest().upper()


print(out)
print(f"size={os.path.getsize(out)} sha256={sha(out)}")
for name, src in files:
    print(f"  {name}: {os.path.getsize(src)} {sha(src)}")
