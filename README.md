# NPC Spawner

An Elden Ring mod that spawns any enemy, boss or NPC wherever you are, to test builds, settings and mods. Offline only.

- **Players:** see [mod/README.md](mod/README.md) (English) or [mod/README.es.md](mod/README.es.md) (Spanish), the same files that ship in the release archive. Releases are on Nexus Mods.
- **Status:** beta.
- **Game versions:** Elden Ring 1.17, 1.17.1 and 1.17.2, worldwide and Japanese (eldenring.exe 2.7.0.0, 2.7.1.0, 2.7.1.1, 2.7.2.0, 2.7.2.1), PC. Tested in game on 2.7.2.0.

It spawns characters through the game's own debug character creator. Its menus: a talk menu, like a merchant's, on F11, and an entry in the System menu with [NativeTweaks](https://www.nexusmods.com/eldenring/mods/11339). With NativeTweaks r157 or later, the talk menu is a row of NativeTweaks' shared F11 menu instead.

## Building

Needs Windows, Python 3 and Rust (stable, MSVC toolchain, target `x86_64-pc-windows-msvc`).

```
python -I build.py            # the release DLL
python -I build.py test       # the unit tests
python -I package.py          # release\NpcSpawner-<version>-ER-1.17-1.17.1-1.17.2.zip
```

`build.py` runs `cargo build --release` in `mod/` and writes source paths without the folders of the machine that built it. `package.py` refuses a DLL that still holds them. The build goes to `%LOCALAPPDATA%\NPCTestArena-build`, or to `NPCSPAWNER_TARGET_DIR` if that variable is set. Extra arguments go to cargo, for example `python -I build.py build --offline`.

## Layout

| Path | What it is |
|---|---|
| `mod/` | The mod: `src/` (Rust), `data/` (embedded tables), `NpcSpawner.ini` (the defaults; a test checks it), the READMEs and THIRD-PARTY-NOTICES.txt of the release |
| `vendor/fromsoftware-rs/` | The `eldenring` and `fromsoftware-shared` crates of fromsoftware-rs, with Elden Ring RVA bundles that are not upstream (see VENDORED.md) |
| `tools/` | Rebuilding the zone data after a game update: `msbdump` (C#, reads the game's maps with Smithbox's libraries), `zones.py`, and `notices.py` (THIRD-PARTY-NOTICES.txt) |

### Source files (`mod/src/`)

| File | What it does |
|---|---|
| `lib.rs` | DLL entry, the frame task, hotkeys and the INI |
| `arena.rs` | Spawned characters: spawn, side, AI, follow, reset, remove |
| `game.rs` | Raw game access: the debug creator, freeing slots, the game's limits |
| `catalog.rs` | Every NpcParam row with names, types and map zones; search and lists |
| `dialog.rs` | NPC Spawner's own talk menu (TalkScript) |
| `talk.rs` | The row in NativeTweaks' shared talk menu (ERNativeUI fork exports) |
| `menu.rs`, `erui.rs` | The System menu entry (ERNativeUI API 1.3) |
| `texts.rs` | Game texts, and NPC Spawner's texts in its own talk menu |
| `hotkeys.rs`, `config.rs`, `formation.rs` | Hotkeys, NpcSpawner.ini, spawn positions |

## License

NPC Spawner is licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

Third-party parts keep their own licenses, listed in [mod/THIRD-PARTY-NOTICES.txt](mod/THIRD-PARTY-NOTICES.txt):
- fromsoftware-rs: MIT OR Apache-2.0.
- The NpcParam row names and map zone names from [Smithbox](https://github.com/vawser/Smithbox): MIT.

Elden Ring is a trademark of FromSoftware / Bandai Namco. This project is not affiliated with them.
