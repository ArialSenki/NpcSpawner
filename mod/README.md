# NPC Spawner 1.0.0

Spawn any enemy, boss or NPC of Elden Ring wherever you are, to test builds, settings and mods.

Made and tested on **Elden Ring 1.17.2 (eldenring.exe 2.7.2.0), worldwide, PC**. Other versions are untested.
**Offline only**, with Easy Anti-Cheat disabled. It uses the game's own debug character creator.

## What it does

- **Spawn anything:** every NpcParam row of the game and the DLC, by name (the game's language or English) or by ID.
- **Ordered lists:** by type (bosses, enemies, invaders and hostile NPCs, spirit ashes and allies, other NPCs) and by the zone of the game's maps where each one is placed (114 zones in 9 areas).
- **One per character** by default: a single row per character, without unnamed rows, dummies and characters that do not fight. "Every row" shows all of them, area variants included.
- **Their own AI:** a character spawns with the AI the game's maps give it.
- **How many, where and how:** up to 20 at once, in front of you, on you, on your lock-on target, on a saved point or at random; in a line, a circle or a grid.
- **Side:** hostile, on your side, or neutral. Changing it also changes the ones already spawned.
- **Allies keep up:** an ally left more than 20 m behind for 4 seconds is brought behind you (`FollowDistance`, 0 = off). They do not walk after you on their own.
- **AI:** active, paused, or still (animated, but it neither moves nor attacks).
- **Health:** health at spawn, infinite health, invulnerable.
- **Reset all** (heal and move back; the dead spawn again), **remove the last one**, **remove all**.
- **Active character limit:** the game keeps about 60 characters active and switches map enemies off for every spawned one. Raising the limit keeps them (experimental, it costs performance).

## Two menus

- **F11:** a talk menu, like a merchant's, with every action and setting and the lists by type and by zone. It needs no other mod. With NativeTweaks r157 or later (it needs the ERNativeUI that comes with it), this talk menu is the **NPC Spawner** row of NativeTweaks' shared talk menu (F11 by default) instead, and NPC Spawner's own F11 does nothing.
- **NativeTweaks:** with [NativeTweaks](https://www.nexusmods.com/eldenring/mods/11339) r155 or later installed, the full menu is in the System menu, NativeTweaks tab, entry NPC Spawner, with the name search and its filters. It uses the ERNativeUI that comes with NativeTweaks; with the official ERNativeUI the search results do not refresh.

## Hotkeys

You choose them: any key with Ctrl, Shift and / or Alt, or none, for each action. Set them in the talk menu (Spawn options > Hotkeys: choose an action, then press the new keys), in the NativeTweaks tab (Hotkeys), or in `[Hotkeys]` of `NpcSpawner.ini`. Changes apply at once, without restarting. They work only while the game window is in front. The defaults:

| Keys | Action |
|---|---|
| F11 | Talk menu (with NativeTweaks r157+: its F11 menu, row NPC Spawner) |
| Ctrl+F6 | Spawn with the current settings |
| Ctrl+Shift+F6 | Spawn the last one again |
| Ctrl+F5 | Save your position as the spawn point |
| Ctrl+F7 | AI of all: active, paused, still |
| Ctrl+F8 | Side of all: as spawned or yours |
| Ctrl+F9 | Reset all |
| Ctrl+F10 | Remove the last one |
| Ctrl+Shift+F10 | Remove all |

## Installation

The archive has a `DLLs` folder with `NpcSpawner.dll` and `NpcSpawner.ini`. Keep them together.

- **Mod Organizer 2:** install the archive as a mod and enable it. Your MO2 needs Elden Ring support that loads DLL mods from a mod's DLLs folder.
- **Mod Engine 3:** add `DLLs/NpcSpawner.dll` to `[[natives]]` in your profile.
- **Mod Engine 2:** copy the DLLs folder into `mod` and add `mod\DLLs\NpcSpawner.dll` to `external_dlls` in `config_eldenring.toml`.

With NativeTweaks, load this DLL **after ERNativeUI.dll and before NativeTweaks.dll**: if it loads too late, the menu entry is missing (the log says "register_provider failed (8)") and the F11 menu still works.

## Settings and log

Every option is in `NpcSpawner.ini`, and each line is explained. It is read again on every spawn. "Save these settings" in the menu writes it.
Once a menu has changed a setting, the menu's settings are used instead of the INI's until the game closes (the hotkeys still follow the INI when it changes).
The active character limit and the update budgets (`MaxActive`, `UpdateNear`, `UpdateFar`) stay raised until the game closes, even after every spawned character is removed.
The log is `NpcSpawner.log`, next to the DLL (with MO2, in `overwrite\DLLs`). It starts fresh at every launch.

## Good to know

- Spawned characters are not saved: they go away at the next loading screen.
- Characters that only appear in cutscenes or as part of a bigger boss may stand still or look wrong, and some bosses depend on their arena's events.
- Up to 76 spawned characters at once (the game's debug list), 30 by default.

## Credits

- Names of NpcParam rows and of map zones: [Smithbox](https://github.com/vawser/Smithbox) by Vawser (MIT).
- fromsoftware-rs Elden Ring bindings by Vincent Swarte and contributors, pelite and Microsoft's windows-rs.

See `THIRD-PARTY-NOTICES.txt`.
