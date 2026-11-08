"""Build NPC Spawner's release DLL without local paths in it.

Usage: python -I build.py [build|test] [extra cargo arguments, e.g. --offline]

The DLL goes to <target dir>\\x86_64-pc-windows-msvc\\release\\npc_spawner.dll,
where package.py takes it. Target dir: NPCSPAWNER_TARGET_DIR, else
%LOCALAPPDATA%\\NPCTestArena-build. Panic messages carry source paths; they are
written as cargo\\... (crates.io sources) and npc-spawner\\... (this repository)
instead of the folders of the machine that built it.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
MOD = os.path.join(ROOT, "mod")

args = sys.argv[1:]
command = args.pop(0) if args and args[0] in ("build", "test") else "build"
target_dir = os.environ.get("NPCSPAWNER_TARGET_DIR") or os.path.join(os.environ["LOCALAPPDATA"], "NPCTestArena-build")
cargo_home = os.environ.get("CARGO_HOME") or os.path.join(os.path.expanduser("~"), ".cargo")

flags = [
    f"--remap-path-prefix={cargo_home}=cargo",
    f"--remap-path-prefix={ROOT}=npc-spawner",
]
env = dict(os.environ)
env.pop("RUSTFLAGS", None)
# Paths may hold spaces: the encoded form separates flags with 0x1F.
env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)

cmd = ["cargo", command, "--release", "--target", "x86_64-pc-windows-msvc", "--target-dir", target_dir, *args]
print(" ".join(cmd), flush=True)
sys.exit(subprocess.call(cmd, cwd=MOD, env=env))
