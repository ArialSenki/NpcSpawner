//! NPC Spawner (phase 1: core). Spawns any NPC or enemy through the game's
//! debug character creator, for testing settings and mods. Offline only.
//!
//! Hotkeys (the player's own, [Hotkeys] in the INI, the tab or the talk
//! menu; these are the defaults; only while the game window is in front):
//!  - Ctrl+F5          save the player's position as the spawn point (Placement = Saved)
//!  - Ctrl+F6          spawn what NpcSpawner.ini says (re-read every time)
//!  - Ctrl+Shift+F6    spawn the last template again (useful after a copy)
//!  - Ctrl+F7          AI of all: Active -> Paused -> Still
//!  - Ctrl+Shift+F7    research (Research = 1): dump WorldChrMan / WorldChrManDbg memory to dumps\
//!  - Ctrl+F8          team of all: as spawned <-> player's side
//!  - Ctrl+F9          reset all: heal and move back; dead ones spawn again
//!  - Ctrl+F10         remove the last one
//!  - Ctrl+Shift+F10   remove all
//! Removed characters are hidden at once and destroyed a second later (their
//! debug ChrSet slot is freed), unless DestroyRemoved = 0.
//!  - F11         talk menu with every action and setting (dialog.rs; no ERNativeUI needed).
//!                     With the NativeTweaks ERNativeUI r157+, NPC Spawner is a row of its
//!                     shared talk menu instead (talk.rs, F11) and NPC Spawner's own F11 does nothing.
//! With ERNativeUI, the same actions and every setting are in the "NPC Spawner"
//! entry of the System screen (menu.rs); its settings replace the INI's
//! until the game closes ("Save these settings" writes them to the INI).
//! Log: NpcSpawner.log next to the DLL (MO2: overwrite\DLLs).

pub mod arena;
pub mod catalog;
pub mod config;
pub mod dialog;
pub mod erui;
pub mod formation;
pub mod game;
pub mod hotkeys;
pub mod menu;
pub mod talk;
pub mod texts;

use std::{
    fs::OpenOptions,
    io::Write,
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use eldenring::{
    cs::{CSTaskGroupIndex, CSTaskImp, WorldChrMan},
    fd4::FD4TaskData,
};
use fromsoftware_shared::{FromStatic, SharedTaskImpExt};
use windows::Win32::{
    Foundation::HMODULE,
    System::{LibraryLoader::GetModuleFileNameW, Threading::GetCurrentProcessId},
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

use arena::{Arena, PlayerView, Template};
use config::Config;

pub const BUILD: &str = concat!("NPC Spawner ", env!("CARGO_PKG_VERSION"));

static DLL_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn log(msg: impl AsRef<str>) {
    let Some(dir) = DLL_DIR.get() else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("NpcSpawner.log")) {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let _ = writeln!(f, "[{ms}] {}", msg.as_ref());
    }
}

/// The settings in use: the tab's when ERNativeUI is there, else the INI's
/// (read again every time).
fn current_config() -> Config {
    menu::settings().unwrap_or_else(load_config)
}

pub fn current_config_pub() -> Config {
    current_config()
}

pub fn describe_pub(t: &Template) -> String {
    describe(t)
}

/// The settings part of a talk-menu choice (both menus): applied at once,
/// so the page shows the new value.
pub fn settings_choice(choice: dialog::Choice, hits: &[catalog::Hit], recent: &[Template]) {
    use dialog::Choice as C;
    let ini = load_config;
    match choice {
        C::SavePoint => menu::edit_settings(ini, |c| c.placement = config::Placement::Saved),
        C::Formation => menu::edit_settings(ini, |c| c.formation = dialog::next_formation(c.formation)),
        C::Distance => menu::edit_settings(ini, |c| c.distance = dialog::next_distance(c.distance)),
        C::Face => menu::edit_settings(ini, |c| c.face_player = !c.face_player),
        C::Health => menu::edit_settings(ini, |c| c.hp_percent = dialog::next_health(c.hp_percent)),
        C::InfiniteHp => menu::edit_settings(ini, |c| c.infinite_hp = !c.infinite_hp),
        C::Invulnerable => menu::edit_settings(ini, |c| c.invulnerable = !c.invulnerable),
        C::Follow => menu::edit_settings(ini, |c| c.follow_distance = dialog::next_follow(c.follow_distance)),
        C::ShowAll => menu::edit_settings(ini, |c| c.search_all = !c.search_all),
        C::HotkeysOn => {
            menu::edit_settings(ini, |c| c.hotkeys_on = !c.hotkeys_on);
            menu::show_hotkeys();
            save_hotkeys();
        }
        C::HotkeysDefault => {
            menu::edit_settings(ini, |c| c.hotkeys = hotkeys::DEFAULTS);
            menu::show_hotkeys();
            save_hotkeys();
            set_hotkey_news("every hotkey back to its default".to_string());
        }
        C::Unbind(i) => set_hotkey(i, hotkeys::Hotkey::NONE),
        C::Capture(i) => request_capture(i),
        C::PickListed(id, think) => menu::choose(id, think, &describe_id(id)),
        C::Count => menu::edit_settings(ini, |c| c.count = dialog::next_count(c.count)),
        C::Placement => menu::edit_settings(ini, |c| c.placement = dialog::next_placement(c.placement)),
        C::Ai => menu::edit_settings(ini, |c| c.ai = c.ai.next()),
        C::Team => menu::edit_settings(ini, |c| c.team = c.team.next()),
        C::PickNearest => menu::edit_settings(ini, |c| {
            c.npc_param_id = 0;
            c.chr_id = 0;
            c.think_id = 0;
        }),
        C::PickHit(i) => {
            if let Some(h) = hits.get(i) {
                menu::choose(h.id, h.think, &h.label);
            }
        }
        C::PickRecent(i) => {
            if let Some(t) = recent.get(i).copied() {
                menu::edit_settings(ini, |c| {
                    c.npc_param_id = t.npc_param_id;
                    c.chr_id = t.chr_id;
                    c.think_id = t.think_id;
                    c.chara_init_id = t.chara_init_id;
                });
            }
        }
        _ => {}
    }
}

/// Choices that act on the spawned characters or the world (frame task).
pub fn is_world_choice(choice: dialog::Choice) -> bool {
    use dialog::Choice as C;
    matches!(
        choice,
        C::Spawn | C::Repeat | C::Reset | C::RemoveAll | C::RemoveLast | C::SavePoint | C::SaveIni | C::Ai | C::Team
    )
}

pub fn load_config_pub() -> Config {
    load_config()
}

/// The INI as last read, with its file time and size (None = no file).
static INI_CACHE: Mutex<Option<(Option<(SystemTime, u64)>, Config)>> = Mutex::new(None);
/// Times the INI was read (it changed on disk).
static INI_READS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The INI's settings. Read again only when the file changes (without
/// ERNativeUI this runs every frame); warnings are logged once per change.
fn load_config() -> Config {
    let path = DLL_DIR.get().map(|d| d.join("NpcSpawner.ini"));
    let stamp = path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| (m.modified().unwrap_or(UNIX_EPOCH), m.len()));
    let mut cache = INI_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, cfg)) = cache.as_ref() {
        if *s == stamp {
            return *cfg;
        }
    }
    let cfg = match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        None => {
            log("NpcSpawner.ini not found; using defaults");
            Config::default()
        }
        Some(text) => {
            let (cfg, warnings) = Config::parse(&text);
            for w in warnings {
                log(format!("NpcSpawner.ini {w}"));
            }
            cfg
        }
    };
    *cache = Some((stamp, cfg));
    INI_READS.fetch_add(1, std::sync::atomic::Ordering::Release);
    cfg
}

/// Research: raw copies of WorldChrMan and WorldChrManDbg (to compare dumps
/// taken with different numbers of spawned characters).
fn dump(world: &WorldChrMan, spawned: usize) {
    let Some(dir) = DLL_DIR.get().map(|d| d.join("dumps")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let base = world as *const WorldChrMan as usize;
    let wcm = game::copy_memory(base, 0x20000);
    let name = format!("{ms}-s{spawned}");
    let _ = std::fs::write(dir.join(format!("{name}-WorldChrMan-{base:x}.bin")), &wcm);
    if let Ok(dbg) = unsafe { <eldenring::cs::WorldChrManDbg as FromStatic>::instance() } {
        let b = dbg as *const _ as usize;
        let _ = std::fs::write(dir.join(format!("{name}-WorldChrManDbg-{b:x}.bin")), game::copy_memory(b, 0x400));
    }
    let mut list = String::from("chr\tset\tindex\tstatus\tomission\tnpc\tdist2\n");
    for (si, set) in world.chr_sets.iter().enumerate() {
        let Some(set) = set else { continue };
        let set: &eldenring::cs::ChrSet<eldenring::cs::ChrIns> = set;
        let entries = set.entries.as_ptr() as usize;
        let size = set.capacity as usize * 16;
        if set.capacity == 0 || set.capacity > 4096 || !game::readable(entries, size) {
            continue;
        }
        for i in 0..set.capacity as usize {
            let e = unsafe { &*set.entries.as_ptr().add(i) };
            let Some(c) = e.chr_ins else { continue };
            let c = unsafe { c.as_ref() };
            let st = unsafe { *(&e.chr_load_status as *const _ as *const u8) };
            let om = unsafe { *(&c.omission_mode as *const _ as *const i32) };
            list.push_str(&format!(
                "{:x}\t{si}\t{i}\t{st}\t{om}\t{}\t{:.0}\n",
                c as *const _ as usize, c.npc_param_id, c.distance_to_player_sqr
            ));
        }
    }
    let _ = std::fs::write(dir.join(format!("{name}-chrs.tsv")), list);
    log(format!("dump {name}: {spawned} spawned; map characters {}", game::map_chr_summary(world)));
}

// ---- hotkeys

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    SavePoint,
    Spawn,
    Repeat,
    CycleAi,
    ToggleTeam,
    Reset,
    RemoveLast,
    RemoveAll,
    Dialog,
    Dump,
}

/// The action of each hotkey (hotkeys::ACTIONS order).
const KEY_ACTIONS: [Action; hotkeys::COUNT] = [
    Action::SavePoint,
    Action::Spawn,
    Action::Repeat,
    Action::CycleAi,
    Action::ToggleTeam,
    Action::Reset,
    Action::RemoveLast,
    Action::RemoveAll,
    Action::Dialog,
    Action::Dump,
];

/// Waiting for the keys of one hotkey (chosen in the talk menu).
struct Capture {
    action: usize,
    since: Instant,
    /// Every key was released since the choice (the menu's confirm key).
    armed: bool,
}

/// What a hotkey change did: the log and the tab's status.
fn set_hotkey_news(text: String) {
    log(format!("hotkeys: {text}"));
    menu::notice(text);
}

/// The talk menu asked to assign the keys of `action`: the next keys
/// pressed within 10 s (the menu closes meanwhile).
static CAPTURE_REQUEST: Mutex<Option<usize>> = Mutex::new(None);

pub fn request_capture(action: usize) {
    *CAPTURE_REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(action);
}

/// Sets one hotkey in the settings in use (and the tab's row).
pub fn set_hotkey(action: usize, key: hotkeys::Hotkey) {
    if action >= hotkeys::COUNT {
        return;
    }
    menu::edit_settings(load_config, |c| c.hotkeys[action] = key);
    menu::show_hotkeys();
    save_hotkeys();
    set_hotkey_news(format!("{}: {}", hotkeys::ACTIONS[action].1, key.name()));
}

/// Writes the hotkeys in use to the [Hotkeys] section of NpcSpawner.ini
/// (the rest of the file is kept), so they last after the game closes.
pub fn save_hotkeys() {
    let (Some(cfg), Some(dir)) = (menu::settings(), DLL_DIR.get()) else { return };
    let path = dir.join("NpcSpawner.ini");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = cfg.with_hotkeys(&old);
    if new == old {
        return;
    }
    match std::fs::write(&path, new) {
        Ok(()) => log("hotkeys saved to NpcSpawner.ini"),
        Err(e) => log(format!("could not save the hotkeys to NpcSpawner.ini: {e}")),
    }
}

fn game_in_front() -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

// ---- names

/// msgbnd file of NPC names (NpcParam nameId). Checked in game: the log says
/// whether names resolve.
const NPC_NAME_CATEGORY: u32 = 0x12;

fn npc_name(npc_param_id: i32) -> Option<String> {
    use eldenring::cs::{NpcParam, SoloParamRepository};
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    let row = repo.get::<NpcParam>(npc_param_id as u32)?;
    let name_id = row.name_id();
    if name_id <= 0 {
        return None;
    }
    texts::lookup(NPC_NAME_CATEGORY, name_id)
}

fn describe_id(id: u32) -> String {
    let id = id as i32;
    describe(&Template { chr_id: Config::resolve_chr_id(id, 0), npc_param_id: id, think_id: 0, chara_init_id: -1 })
}

fn describe(t: &Template) -> String {
    match catalog::name(t.npc_param_id).or_else(|| npc_name(t.npc_param_id)) {
        Some(n) => format!("{n} ({})", t.npc_param_id),
        None => format!("NpcParam {} (c{:04})", t.npc_param_id, t.chr_id),
    }
}

// ---- frame task

struct State {
    arena: Arena,
    keys_prev: [bool; hotkeys::COUNT],
    capture: Option<Capture>,
    /// INI_READS when the hotkeys were last taken from the INI.
    ini_reads: u32,
    /// When the INI was last checked for changes.
    ini_checked: Option<Instant>,
    dialog: dialog::Dialog,
    had_player: bool,
    /// DestroyRemoved, read when the world loads.
    destroy: bool,
}

// Only touched from the game's frame task.
unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn frame(_: &FD4TaskData) {
    if std::panic::catch_unwind(AssertUnwindSafe(frame_inner)).is_err() {
        log("panic in the frame task (caught)");
    }
}

fn frame_inner() {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let st = guard.get_or_insert_with(|| State {
        arena: Arena::default(),
        keys_prev: [false; hotkeys::COUNT],
        capture: None,
        ini_reads: 0,
        ini_checked: None,
        dialog: dialog::Dialog::default(),
        had_player: false,
        destroy: true,
    });

    let Ok(world) = (unsafe { WorldChrMan::instance_mut() }) else { return };
    let Some(player) = world.main_player.as_ref() else {
        if st.had_player {
            st.had_player = false;
            st.arena.forget_world();
        }
        st.dialog.close();
        return;
    };
    let pc = &player.chr_ins;
    let q = pc.modules.physics.orientation;
    let p = pc.modules.physics.position;
    let view = PlayerView {
        ptr: pc as *const _ as usize,
        pos: [p.0, p.1, p.2],
        forward: formation::forward_of([q.0, q.1, q.2, q.3]),
        target: pc
            .is_locked_on
            .then(|| [pc.lock_on_target_position.0, pc.lock_on_target_position.1, pc.lock_on_target_position.2]),
        team: pc.team_type,
    };
    let player_handle = pc.field_ins_handle;
    let bp = &player.block_position;
    let local_offset = [view.pos[0] - bp.x, view.pos[1] - bp.y, view.pos[2] - bp.z];

    if !st.had_player {
        st.had_player = true;
        log(format!("{BUILD}: player loaded at {:?}; debug ChrSet {:?} of {}", view.pos, game::debug_set_used(world), world.debug_chr_set.capacity));
    }
    // Game names need the message lookup: find it first, and build the
    // catalogue again if it was built without it (both at most every 5 s).
    if !texts::ready() {
        texts::install();
    }
    if catalog::needs_build(texts::ready()) {
        if let Some(dir) = DLL_DIR.get() {
            catalog::build(dir, current_config().research);
        }
    }
    if !st.arena.creator_checked {
        st.arena.creator_checked = true;
        st.arena.creator = game::find_creator(world);
        st.arena.lookup = game::find_allocator_lookup(world);
        game::ensure_limits_slot();
        if !texts::ready() {
            texts::install();
        }
        let cfg = current_config();
        st.destroy = cfg.destroy_removed;
        log(format!("map characters at load: {}", game::map_chr_summary(world)));
        game::raise_update_budgets(cfg.update_near, cfg.update_far);
        game::raise_limits(cfg.max_active);
    }

    // The INI changed on disk (checked every second): its hotkeys apply at
    // once, also over the tab's settings.
    if st.ini_checked.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
        st.ini_checked = Some(Instant::now());
        let ini = load_config();
        let reads = INI_READS.load(std::sync::atomic::Ordering::Acquire);
        if reads != st.ini_reads {
            let first = st.ini_reads == 0;
            st.ini_reads = reads;
            if !first && menu::settings().is_some_and(|c| c.hotkeys != ini.hotkeys || c.hotkeys_on != ini.hotkeys_on) {
                menu::edit_settings(load_config, |c| {
                    c.hotkeys = ini.hotkeys;
                    c.hotkeys_on = ini.hotkeys_on;
                });
                menu::show_hotkeys();
                log("hotkeys: taken from the changed NpcSpawner.ini");
            }
        }
    }

    // Hotkeys, edge-triggered; or the keys of the one being assigned.
    let in_front = game_in_front();
    if let Some(a) = CAPTURE_REQUEST.lock().unwrap_or_else(|e| e.into_inner()).take() {
        st.capture = Some(Capture { action: a, since: Instant::now(), armed: false });
        log(format!("hotkeys: press the new keys for {} within 10 s", hotkeys::ACTIONS[a.min(hotkeys::COUNT - 1)].1));
    }
    let mut actions = Vec::new();
    let mut reopen = false;
    if let Some(cap) = st.capture.as_mut() {
        let held = in_front.then(|| hotkeys::assignable().find(|vk| hotkeys::down(*vk))).flatten();
        if cap.since.elapsed() > Duration::from_secs(10) {
            let a = cap.action;
            st.capture = None;
            set_hotkey_news(format!("{}: no key pressed, unchanged", hotkeys::ACTIONS[a].1));
            reopen = true;
        } else if !cap.armed {
            cap.armed = held.is_none();
        } else if let Some(vk) = held {
            let m = hotkeys::modifiers();
            let a = cap.action;
            st.capture = None;
            set_hotkey(a, hotkeys::Hotkey { vk, ctrl: m.ctrl, shift: m.shift, alt: m.alt });
            reopen = true;
        }
        // Keys held now do not fire their action when the capture ends.
        st.keys_prev = [true; hotkeys::COUNT];
    } else {
        let cfg = current_config();
        let m = hotkeys::modifiers();
        for (i, key) in cfg.hotkeys.iter().enumerate() {
            let down = in_front && cfg.hotkeys_on && hotkeys::pressed(key, m);
            if down && !st.keys_prev[i] {
                actions.push(KEY_ACTIONS[i]);
            }
            st.keys_prev[i] = down;
        }
    }
    // Back to the hotkeys, where the changed row shows its new keys.
    if reopen {
        if talk::registered() {
            talk::open_at(dialog::Page::Hotkeys);
        } else {
            st.dialog.open_at(dialog::Page::Hotkeys);
        }
    }

    let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    let run = |st: &mut State, world: &mut WorldChrMan, action: Action| match action {
        Action::SavePoint => {
            st.arena.saved_point = Some(view.pos);
            log(format!("spawn point saved: {:?}", view.pos));
        }
        Action::Spawn => {
            let cfg = current_config();
            if cfg.update_near > 0 || cfg.update_far > 0 {
                game::raise_update_budgets(cfg.update_near, cfg.update_far);
            }
            if cfg.max_active > 0 {
                game::raise_limits(cfg.max_active);
            }
            st.arena.spawn_from_ini(cfg, world, &view, seed)
        }
        Action::Repeat => st.arena.repeat_last(&view, seed),
        Action::CycleAi => st.arena.cycle_ai(),
        Action::ToggleTeam => st.arena.toggle_team(world, current_config().side_change_respawn),
        Action::Reset => st.arena.reset_all(world),
        Action::RemoveLast => st.arena.remove_last(world),
        Action::RemoveAll => st.arena.remove_all(world),
        Action::Dump if current_config().research => dump(world, st.arena.spawned.len()),
        Action::Dump => {}
        Action::Dialog if talk::registered() => {}
        Action::Dialog => st.dialog.open(),
    };
    for a in actions {
        run(st, world, a);
    }

    // The talk menu (NPC Spawner's own; with ERNativeUI's shared one, its
    // choices come through talk::take_queue).
    let mut world_choices = Vec::new();
    if st.dialog.is_open() {
        let cfg = current_config();
        let recent = st.arena.recent.clone();
        let hits = menu::search_hits();
        let hit_labels: Vec<String> = hits.iter().map(|h| h.label.clone()).collect();
        let v = dialog::View {
            cfg: &cfg,
            spawned: st.arena.spawned.len(),
            recent: &recent,
            hits: &hit_labels,
            has_last: st.arena.has_last(),
            describe: &describe,
        };
        if let Some(choice) = st.dialog.step(player_handle, &v) {
            log(format!("dialog: {choice:?}"));
            settings_choice(choice, &hits, &recent);
            if is_world_choice(choice) {
                world_choices.push(choice);
            }
        }
    }
    world_choices.extend(talk::take_queue());
    for choice in world_choices {
        use dialog::Choice as C;
        match choice {
            C::Spawn => run(st, world, Action::Spawn),
            C::Repeat => run(st, world, Action::Repeat),
            C::Reset => run(st, world, Action::Reset),
            C::RemoveAll => run(st, world, Action::RemoveAll),
            C::RemoveLast => run(st, world, Action::RemoveLast),
            C::SavePoint => run(st, world, Action::SavePoint),
            C::SaveIni => save_ini(),
            C::Ai => st.arena.set_ai_all(current_config().ai),
            C::Team => {
                let cfg = current_config();
                st.arena.set_team_all(cfg.team, world, cfg.side_change_respawn);
            }
            _ => {}
        }
    }

    for a in menu::take_actions() {
        match a {
            menu::MenuAction::Spawn => run(st, world, Action::Spawn),
            menu::MenuAction::Repeat => run(st, world, Action::Repeat),
            menu::MenuAction::SavePoint => run(st, world, Action::SavePoint),
            menu::MenuAction::Reset => run(st, world, Action::Reset),
            menu::MenuAction::RemoveLast => run(st, world, Action::RemoveLast),
            menu::MenuAction::RemoveAll => run(st, world, Action::RemoveAll),
            menu::MenuAction::SetAi(ai) => st.arena.set_ai_all(ai),
            menu::MenuAction::SetTeam(team) => {
                st.arena.set_team_all(team, world, current_config().side_change_respawn)
            }
            menu::MenuAction::ApplyLimits => game::raise_limits(current_config().max_active),
            menu::MenuAction::SaveIni => save_ini(),
        }
    }
    if let Some(cfg) = menu::settings() {
        st.destroy = cfg.destroy_removed;
    }

    {
        let mut snap = talk::SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner());
        snap.spawned = st.arena.spawned.len();
        snap.has_last = st.arena.has_last();
        if snap.recent != st.arena.recent {
            snap.recent = st.arena.recent.clone();
        }
    }
    talk::tick();
    st.arena.probe_tick(world, local_offset);
    if !st.arena.spawned.is_empty() {
        st.arena.follow(world, &view, current_config().follow_distance);
    }
    st.arena.step_spawning(world);
    st.arena.maintain(world, st.destroy);

    let dead = st.arena.spawned.iter().filter(|s| s.dead).count();
    menu::set_status(if st.arena.spawned.is_empty() {
        "Nothing spawned".to_string()
    } else {
        format!(
            "{} spawned{}",
            st.arena.spawned.len(),
            if dead > 0 { format!(", {dead} dead") } else { String::new() }
        )
    });
}

#[unsafe(no_mangle)]
/// # Safety
/// Called by the Windows loader only.
pub unsafe extern "system" fn DllMain(hmodule: HMODULE, reason: u32, _: *mut core::ffi::c_void) -> i32 {
    if reason != 1 {
        return 1;
    }
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(hmodule), &mut buf) } as usize;
    let path = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    if let Some(dir) = path.parent() {
        let _ = DLL_DIR.set(dir.to_path_buf());
    }
    // A fresh log every launch.
    if let Some(dir) = DLL_DIR.get() {
        let _ = std::fs::write(dir.join("NpcSpawner.log"), "");
    }
    log(format!("{BUILD} loaded from {}", path.display()));
    let module = hmodule.0 as usize;
    std::thread::spawn(move || menu::register(HMODULE(module as *mut _), load_config()));
    std::thread::spawn(|| {
        texts::install();
        // The DLL can load before the game has finished starting (the
        // scheduler lookup then fails at once): keep trying for 3 minutes.
        let start = std::time::Instant::now();
        let mut last_error = String::new();
        let task = loop {
            // A zero timeout only checks once: the library's own wait spins
            // (yield_now) and kept a core busy while the game was starting.
            match CSTaskImp::wait_for_instance(Duration::ZERO) {
                Ok(t) => break t,
                Err(e) => {
                    let e = format!("{e:?}");
                    if e != last_error {
                        log(format!("waiting for the game's task scheduler ({e})"));
                        last_error = e;
                    }
                    if start.elapsed() > Duration::from_secs(180) {
                        log("the game's task scheduler never appeared; the mod is inactive");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        };
        std::mem::forget(task.run_recurring(frame as fn(&FD4TaskData), CSTaskGroupIndex::FrameBegin));
        log(format!("frame task installed; {:?}", load_config()));
    });
    1
}

fn save_ini() {
    let (Some(cfg), Some(dir)) = (menu::settings(), DLL_DIR.get()) else { return };
    match std::fs::write(dir.join("NpcSpawner.ini"), cfg.to_ini()) {
        Ok(()) => {
            log("settings saved to NpcSpawner.ini");
            menu::notice("Settings saved");
        }
        Err(e) => log(format!("could not save NpcSpawner.ini: {e}")),
    }
}
