//! The "NPC Spawner" tab in the System screen (ERNativeUI API 1.3).
//! Optional: without ERNativeUI the mod keeps its hotkeys and INI.
//!
//! ERNativeUI calls buttons on the game's UI thread and value changes on its
//! own worker. Callbacks only update the shared settings and queue actions;
//! the frame task (lib.rs) runs the actions on the game's frame.

use std::{
    ffi::c_void,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::HMODULE,
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    },
    core::{PCSTR, w},
};

use crate::{
    config::{AiMode, Config, Formation, Placement, Team},
    erui::{self, Api, Utf16View},
    log,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Spawn,
    Repeat,
    SavePoint,
    Reset,
    RemoveLast,
    RemoveAll,
    SetAi(AiMode),
    SetTeam(Team),
    ApplyLimits,
    SaveIni,
}

struct Menu {
    api: Api,
    provider: erui::ProviderHandle,
    status_row: erui::RowHandle,
    npc_row: erui::RowHandle,
    think_row: erui::RowHandle,
    result_row: erui::RowHandle,
    /// Button under Result showing the selected name (button labels repaint
    /// live; slider rows do not) and spawning it when pressed.
    selected_row: erui::RowHandle,
    zone_row: erui::RowHandle,
    zone_name_row: erui::RowHandle,
    set_row_text: Option<erui::SetRowTextFn>,
    set_slider_values: Option<erui::SetCustomSliderValuesFn>,
    hotkeys_toggle: erui::RowHandle,
    hotkey_rows: [erui::RowHandle; crate::hotkeys::COUNT],
}
// Handles and function pointers only.
unsafe impl Send for Menu {}
unsafe impl Sync for Menu {}

static MENU: OnceLock<Menu> = OnceLock::new();
/// The menu's settings (None until the tab is committed).
static SETTINGS: Mutex<Option<Config>> = Mutex::new(None);
static ACTIONS: Mutex<Vec<MenuAction>> = Mutex::new(Vec::new());
static LAST_STATUS: Mutex<Option<(Instant, String)>> = Mutex::new(None);
static NOTICE: Mutex<Option<(Instant, String)>> = Mutex::new(None);
/// Last value written to the read-only Selected row.
static SELECTED_TEXT: Mutex<String> = Mutex::new(String::new());
/// Hits of the last name search (menu), also shown by the dialog.
static HITS: Mutex<Vec<crate::catalog::Hit>> = Mutex::new(Vec::new());
/// Text of the last name search (searched again when the filter changes).
static QUERY: Mutex<String> = Mutex::new(String::new());
const RESULT_SLOTS: usize = 64;
/// Type, area and zone chosen in Search filters (`all` comes from the settings).
static FILTER: Mutex<crate::catalog::Filter> =
    Mutex::new(crate::catalog::Filter { all: false, kind: None, area: None, zone: None });
/// Zone slider positions: 0 = every zone shown, then these zones.
static ZONE_SLOTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// The search and lists filter, as chosen in the menu.
pub fn filter() -> crate::catalog::Filter {
    let mut f = *FILTER.lock().unwrap_or_else(|e| e.into_inner());
    f.all = SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|c| c.search_all);
    f
}

pub fn search_hits() -> Vec<crate::catalog::Hit> {
    HITS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Chooses an NpcParam id for spawning (search result or dialog pick), with
/// its AI (0 = the id's own), and shows them in the tab's Advanced rows.
pub fn choose(id: u32, think: u32, label: &str) {
    edit_settings(crate::load_config_pub, |c| {
        c.npc_param_id = id as i32;
        c.chr_id = 0;
        c.think_id = think as i32;
        c.chara_init_id = -1;
    });
    if let Some(m) = MENU.get() {
        if let Some(set) = m.api.set_text_input_value {
            for (row, v) in [(m.npc_row, id.to_string()), (m.think_row, if think > 0 { think.to_string() } else { String::new() })] {
                let t = erui::w(&v);
                let v = Utf16View::new(&t);
                set(m.provider, row, &v);
            }
        }
    }
    log(format!("chosen: {label}{}", if think > 0 { format!(", AI {think}") } else { String::new() }));
}

/// Runs the name search and shows its matches (the first one is chosen).
fn run_search(text: &str) {
    *QUERY.lock().unwrap_or_else(|e| e.into_inner()) = text.to_string();
    let f = filter();
    if text.trim().is_empty() && !f.narrows() {
        return;
    }
    let (hits, total) = crate::catalog::search(text, RESULT_SLOTS, f);
    show_hits(&hits);
    if total > hits.len() {
        notice(format!("First {} of {total}: narrow it", hits.len()));
    }
    match hits.first() {
        Some(h) => {
            let (id, think, label) = (h.id, h.think, h.label.clone());
            *HITS.lock().unwrap_or_else(|e| e.into_inner()) = hits;
            choose(id, think, &label);
        }
        None => {
            HITS.lock().unwrap_or_else(|e| e.into_inner()).clear();
            notice(if text.trim().is_empty() {
                "Nothing with these filters".to_string()
            } else {
                format!("Nothing found for \"{}\"", text.trim())
            });
        }
    }
}

fn show_hits(hits: &[crate::catalog::Hit]) {
    let Some(m) = MENU.get() else { return };
    let Some(set) = m.set_slider_values else { return };
    // The value box is narrow: position and id there, the full text in the
    // row's help (result_help), updated whenever the selection changes.
    let n = hits.len();
    let texts: Vec<Vec<u16>> = (0..RESULT_SLOTS)
        .map(|i| match hits.get(i) {
            Some(_) => erui::w(&format!("{}/{n}", i + 1)),
            None if i == 0 => erui::w("no match"),
            None => erui::w("-"),
        })
        .collect();
    let views: Vec<Utf16View> = texts.iter().map(|t| Utf16View::new(t)).collect();
    set(m.provider, m.result_row, views.as_ptr(), views.len() as u32);
    if let Some(idx) = m.api.set_custom_slider_index {
        idx(m.provider, m.result_row, 0);
    }
    result_help(hits.first(), n);
}

/// The Result row's help: the full text of the selected match.
fn result_help(hit: Option<&crate::catalog::Hit>, n: usize) {
    let Some(m) = MENU.get() else { return };
    let Some(set) = m.set_row_text else { return };
    let text = match hit {
        Some(h) => format!("{}  -  {n} match{}; the selected one is spawned.", h.label, if n == 1 { "" } else { "es" }),
        None => "No match. Try part of a name, in the game's language or in English.".to_string(),
    };
    let t = erui::w(&text);
    let tv = Utf16View::new(&t);
    // Row help only repaints when the page is reopened (fork r156): keep it fixed.
    let _ = (&set, &tv);
    let r = erui::OK;
    // Shown as a text-input value, which repaints at once.
    let value: String = match hit {
        Some(h) => h.short.chars().take(erui::TEXT_INPUT_MAX_LENGTH as usize).collect(),
        None => "no match".to_string(),
    };
    *SELECTED_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = value.clone();
    let r2 = match m.api.set_text_input_value {
        Some(f) => {
            let v = erui::w(&value);
            let vv = Utf16View::new(&v);
            f(m.provider, m.selected_row, &vv)
        }
        None => erui::OK,
    };
    if r != erui::OK || r2 != erui::OK {
        log(format!("menu: updating the Result rows failed ({r} / {r2})"));
    }
}

/// Zones the Zone slider offers for an area (every zone without one).
fn zones_for(area: Option<usize>) -> Vec<u16> {
    match area {
        Some(a) => crate::catalog::zones_of_area(a),
        None => (0..crate::catalog::zones().len() as u16).collect(),
    }
}

fn zone_values(zones: &[u16]) -> Vec<String> {
    let n = zones.len();
    std::iter::once("all".to_string()).chain((1..=n).map(|i| format!("{i}/{n}"))).collect()
}

fn set_zone_name(text: &str) {
    let Some(m) = MENU.get() else { return };
    if let Some(set) = m.api.set_text_input_value {
        let t = erui::w(text);
        let v = Utf16View::new(&t);
        set(m.provider, m.zone_name_row, &v);
    }
}

/// The area changed: the Zone slider offers its zones, starting at "all".
fn area_changed(area: Option<usize>) {
    let zones = zones_for(area);
    if let Some(m) = MENU.get() {
        if let Some(set) = m.set_slider_values {
            let texts: Vec<Vec<u16>> = zone_values(&zones).iter().map(|t| erui::w(t)).collect();
            let views: Vec<Utf16View> = texts.iter().map(|t| Utf16View::new(t)).collect();
            set(m.provider, m.zone_row, views.as_ptr(), views.len() as u32);
        }
        if let Some(idx) = m.api.set_custom_slider_index {
            idx(m.provider, m.zone_row, 0);
        }
    }
    *ZONE_SLOTS.lock().unwrap_or_else(|e| e.into_inner()) = zones;
    set_zone_name("All zones");
}

/// A short message shown in the status row for 4 s (feedback for buttons).
pub fn notice(text: impl Into<String>) {
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), text.into()));
}

/// Settings chosen in the tab, or None when there is no tab.
pub fn settings() -> Option<Config> {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn take_actions() -> Vec<MenuAction> {
    std::mem::take(&mut *ACTIONS.lock().unwrap_or_else(|e| e.into_inner()))
}

fn push(a: MenuAction) {
    ACTIONS.lock().unwrap_or_else(|e| e.into_inner()).push(a);
}

/// Edits the shared settings from the dialog; without ERNativeUI they start
/// from `init` (the INI) the first time.
pub fn edit_settings(init: impl FnOnce() -> Config, f: impl FnOnce(&mut Config)) {
    let mut s = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(init));
}

fn edit(f: impl FnOnce(&mut Config)) {
    if let Some(c) = SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        f(c);
    }
}

/// Live status row text (NativeTweaks fork only; at most every 500 ms).
pub fn set_status(text: String) {
    let text = match NOTICE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some((t, n)) if t.elapsed() < Duration::from_secs(4) => n.clone(),
        _ => text,
    };
    // The status box holds at most 35 characters.
    const STATUS_MAX: usize = erui::TEXT_INPUT_MAX_LENGTH as usize;
    let text = if text.chars().count() > STATUS_MAX {
        let mut t: String = text.chars().take(STATUS_MAX - 1).collect();
        t.push('…');
        t
    } else {
        text
    };
    let Some(m) = MENU.get() else { return };
    let Some(set) = m.api.set_text_input_value else { return };
    let mut last = LAST_STATUS.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref().is_some_and(|(t, s)| *s == text || t.elapsed() < Duration::from_millis(250)) {
        return;
    }
    let label = erui::w(&text);
    let view = Utf16View::new(&label);
    set(m.provider, m.status_row, &view);
    *last = Some((Instant::now(), text));
}

// ---- callbacks (user_data = row id)

const PLACEMENTS: [Placement; 5] =
    [Placement::Front, Placement::Player, Placement::Target, Placement::Saved, Placement::Random];
const FORMATIONS: [Formation; 3] = [Formation::Line, Formation::Circle, Formation::Grid];
const AIS: [AiMode; 3] = [AiMode::Active, AiMode::Paused, AiMode::Still];
const TEAMS: [Team; 3] = [Team::AsSpawned, Team::Ally, Team::Neutral];

const C_PLACEMENT: usize = 1;
const C_FORMATION: usize = 2;
const C_AI: usize = 3;
const C_TEAM: usize = 4;
const C_SHOW: usize = 5;
const C_TYPE: usize = 6;
const C_AREA: usize = 7;
const S_ZONE: usize = 38;
/// Read-only name of the chosen zone.
const X_ZONE_NAME: usize = 39;
const T_FACE: usize = 10;
const T_INFINITE: usize = 11;
const T_INVULNERABLE: usize = 12;
const T_DESTROY: usize = 13;
const N_COUNT: usize = 20;
const N_DISTANCE: usize = 21;
const N_HP: usize = 22;
const N_MAX_SPAWNED: usize = 23;
const N_MAX_ACTIVE: usize = 24;
const N_FOLLOW: usize = 25;
const X_NPC: usize = 30;
const X_THINK: usize = 31;
const X_CHR: usize = 32;
const X_INIT: usize = 33;
const X_SEARCH: usize = 34;
const S_RESULT: usize = 35;
/// Read-only display of the selected match (changes are ignored).
const X_SELECTED: usize = 36;
/// Status display (changes are replaced by the status).
const X_STATUS: usize = 37;
const T_HOTKEYS: usize = 14;
/// Text boxes of the hotkeys: X_HOTKEY + hotkeys::ACTIONS index.
const X_HOTKEY: usize = 60;

extern "C" fn on_button(user_data: *mut c_void) {
    let action = match user_data as usize {
        40 => MenuAction::Spawn,
        41 => MenuAction::Repeat,
        42 => MenuAction::SavePoint,
        43 => MenuAction::Reset,
        44 => MenuAction::RemoveLast,
        45 => MenuAction::RemoveAll,
        46 => MenuAction::SaveIni,
        _ => return,
    };
    push(action);
}

extern "C" fn on_choice(user_data: *mut c_void, value: u8) {
    let i = value as usize;
    match user_data as usize {
        C_PLACEMENT => edit(|c| c.placement = PLACEMENTS[i.min(PLACEMENTS.len() - 1)]),
        C_FORMATION => edit(|c| c.formation = FORMATIONS[i.min(FORMATIONS.len() - 1)]),
        C_AI => {
            let ai = AIS[i.min(AIS.len() - 1)];
            edit(|c| c.ai = ai);
            push(MenuAction::SetAi(ai));
        }
        C_SHOW | C_TYPE | C_AREA => {
            match user_data as usize {
                C_SHOW => edit(|c| c.search_all = i == 1),
                C_TYPE => FILTER.lock().unwrap_or_else(|e| e.into_inner()).kind = i.checked_sub(1),
                _ => {
                    let area = i.checked_sub(1);
                    {
                        let mut f = FILTER.lock().unwrap_or_else(|e| e.into_inner());
                        f.area = area;
                        f.zone = None;
                    }
                    area_changed(area);
                }
            }
            let q = QUERY.lock().unwrap_or_else(|e| e.into_inner()).clone();
            run_search(&q);
        }
        C_TEAM => {
            let team = TEAMS[i.min(TEAMS.len() - 1)];
            edit(|c| c.team = team);
            push(MenuAction::SetTeam(team));
        }
        _ => {}
    }
}

extern "C" fn on_toggle(user_data: *mut c_void, value: u8) {
    let on = value != 0;
    match user_data as usize {
        T_FACE => edit(|c| c.face_player = on),
        T_INFINITE => edit(|c| c.infinite_hp = on),
        T_INVULNERABLE => edit(|c| c.invulnerable = on),
        T_DESTROY => edit(|c| c.destroy_removed = on),
        T_HOTKEYS => {
            edit(|c| c.hotkeys_on = on);
            crate::save_hotkeys();
        }
        _ => {}
    }
}

extern "C" fn on_numeric(user_data: *mut c_void, ctx: *const erui::NumericSliderChangeContext) {
    let Some(ctx) = (unsafe { ctx.as_ref() }) else { return };
    let v = ctx.value.scaled_value;
    match user_data as usize {
        N_COUNT => edit(|c| c.count = v.clamp(1, 20) as u32),
        N_DISTANCE => edit(|c| c.distance = v.clamp(0, 30) as f32),
        N_HP => edit(|c| c.hp_percent = v.clamp(1, 100) as u32),
        N_MAX_SPAWNED => edit(|c| c.max_spawned = v.clamp(1, 76) as u32),
        N_FOLLOW => edit(|c| c.follow_distance = v.clamp(0, 100) as f32),
        N_MAX_ACTIVE => {
            edit(|c| c.max_active = v.clamp(0, 500) as i32);
            push(MenuAction::ApplyLimits);
        }
        _ => {}
    }
}

extern "C" fn on_text(user_data: *mut c_void, ctx: *const erui::TextInputChangeContext) {
    let Some(ctx) = (unsafe { ctx.as_ref() }) else { return };
    let text = if ctx.value.data.is_null() {
        String::new()
    } else {
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ctx.value.data, ctx.value.length as usize) })
    };
    let n = text.trim().parse::<i32>();
    // Status and Selected are displays: ERNativeUI has no read-only text box,
    // so whatever is typed there is replaced by the value at once.
    if user_data as usize == X_ZONE_NAME {
        let name = FILTER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .zone
            .and_then(|z| crate::catalog::zones().get(z as usize))
            .map_or("All zones", |z| z.name);
        set_zone_name(name);
        return;
    }
    if user_data as usize == X_SELECTED || user_data as usize == X_STATUS {
        if let Some(m) = MENU.get() {
            if let Some(set) = m.api.set_text_input_value {
                let (row, value) = if user_data as usize == X_SELECTED {
                    (m.selected_row, SELECTED_TEXT.lock().unwrap_or_else(|e| e.into_inner()).clone())
                } else {
                    let v = LAST_STATUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|l| l.1.clone());
                    (m.status_row, v.unwrap_or_else(|| "Nothing spawned".to_string()))
                };
                let w = erui::w(&value);
                let v = Utf16View::new(&w);
                set(m.provider, row, &v);
            }
        }
        return;
    }
    if (X_HOTKEY..X_HOTKEY + crate::hotkeys::COUNT).contains(&(user_data as usize)) {
        let i = user_data as usize - X_HOTKEY;
        match crate::hotkeys::Hotkey::parse(&text) {
            Some(h) => {
                edit(|c| c.hotkeys[i] = h);
                crate::save_hotkeys();
                notice(format!("{}: {}", crate::hotkeys::ACTIONS[i].0, h.name()));
            }
            None => notice(format!("Unknown keys \"{}\"", text.trim())),
        }
        return;
    }
    if user_data as usize == X_SEARCH {
        if !crate::catalog::is_built() {
            notice("Load a game first: the catalogue is built then");
            return;
        }
        run_search(&text);
        return;
    }
    match user_data as usize {
        X_NPC => edit(|c| c.npc_param_id = n.unwrap_or(0).max(0)),
        X_THINK => edit(|c| c.think_id = n.unwrap_or(0).max(0)),
        X_CHR => edit(|c| c.chr_id = n.unwrap_or(0).max(0)),
        X_INIT => edit(|c| c.chara_init_id = n.unwrap_or(-1)),
        _ => {}
    }
}

extern "C" fn on_custom(user_data: *mut c_void, ctx: *const erui::CustomSliderChangeContext) {
    let Some(ctx) = (unsafe { ctx.as_ref() }) else { return };
    if user_data as usize == S_ZONE {
        let i = ctx.index as usize;
        let zone = if i == 0 { None } else { ZONE_SLOTS.lock().unwrap_or_else(|e| e.into_inner()).get(i - 1).copied() };
        FILTER.lock().unwrap_or_else(|e| e.into_inner()).zone = zone;
        set_zone_name(match zone.and_then(|z| crate::catalog::zones().get(z as usize)) {
            Some(z) => z.name,
            None => "All zones",
        });
        let q = QUERY.lock().unwrap_or_else(|e| e.into_inner()).clone();
        run_search(&q);
        return;
    }
    if user_data as usize != S_RESULT {
        return;
    }
    let (hit, n) = {
        let hits = HITS.lock().unwrap_or_else(|e| e.into_inner());
        (hits.get(ctx.index as usize).cloned(), hits.len())
    };
    if let Some(h) = hit {
        result_help(Some(&h), n);
        choose(h.id, h.think, &h.label);
    }
}

// ---- registration

/// A target in the game's gold on a transparent background.
fn icon() -> Vec<u8> {
    let mut px = vec![0u8; erui::TAB_ICON_BYTES];
    for y in 0..32 {
        for x in 0..32 {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            let ring = (11.0..14.0).contains(&d) || (5.0..7.5).contains(&d) || d < 2.2;
            let cross = (dx.abs() < 1.0 || dy.abs() < 1.0) && d < 15.0 && d > 8.5;
            if ring || cross {
                let o = (y * 32 + x) * 4;
                px[o..o + 4].copy_from_slice(&[200, 170, 110, 255]);
            }
        }
    }
    px
}

fn connect() -> Option<Api> {
    let start = Instant::now();
    let host = loop {
        match unsafe { GetModuleHandleW(w!("ERNativeUI.dll")) } {
            Ok(h) => break h,
            Err(_) if start.elapsed() < Duration::from_secs(60) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => {
                log("menu: ERNativeUI.dll not loaded; hotkeys and INI only");
                return None;
            }
        }
    };
    let Some(sym) = (unsafe { GetProcAddress(host, PCSTR(b"ERUI_GetApi\0".as_ptr())) }) else {
        log("menu: ERNativeUI.dll has no ERUI_GetApi");
        return None;
    };
    let get_api: erui::GetApiFn = unsafe { std::mem::transmute(sym) };
    loop {
        let mut api = Api::zeroed();
        api.size = erui::API_V1_3_SIZE;
        let r = get_api(erui::API_VERSION_1_3, &mut api);
        if r == erui::OK {
            if api.api_version != erui::API_VERSION_1_3 || api.size != erui::API_V1_3_SIZE {
                log(format!("menu: unexpected API {:#x} / size {}", api.api_version, api.size));
                return None;
            }
            return Some(api);
        }
        if r != erui::HOST_NOT_READY || start.elapsed() > Duration::from_secs(90) {
            log(format!("menu: ERUI_GetApi failed ({r})"));
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct Builder<'a> {
    api: &'a Api,
    provider: erui::ProviderHandle,
    page: erui::PageHandle,
    failed: Option<String>,
}

impl Builder<'_> {
    fn check(&mut self, what: &str, r: erui::Result) {
        if r != erui::OK && self.failed.is_none() {
            self.failed = Some(format!("{what}: {r}"));
        }
    }

    fn button(&mut self, label: &str, help: &str, id: usize) -> erui::RowHandle {
        let (l, h) = (erui::w(label), erui::w(help));
        let d = erui::ButtonDesc {
            size: size_of::<erui::ButtonDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            callback: Some(on_button),
            user_data: id as *mut c_void,
            enabled: 1,
            reserved: 0,
        };
        let mut row = 0;
        let r = (self.api.add_button.unwrap())(self.provider, self.page, &d, &mut row);
        self.check(label, r);
        row
    }

    fn toggle(&mut self, label: &str, help: &str, id: usize, on: bool) -> erui::RowHandle {
        let (l, h) = (erui::w(label), erui::w(help));
        let d = erui::ToggleDesc {
            size: size_of::<erui::ToggleDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            changed_callback: Some(on_toggle),
            user_data: id as *mut c_void,
            initial_value: on as u8,
            reserved8: [0; 3],
            enabled: 1,
        };
        let mut row = 0;
        let r = (self.api.add_toggle.unwrap())(self.provider, self.page, &d, &mut row);
        self.check(label, r);
        row
    }

    fn choice(&mut self, label: &str, help: &str, id: usize, options: &[&str], initial: usize) {
        let (l, h) = (erui::w(label), erui::w(help));
        let texts: Vec<Vec<u16>> = options.iter().map(|o| erui::w(o)).collect();
        let views: Vec<Utf16View> = texts.iter().map(|t| Utf16View::new(t)).collect();
        let d = erui::ChoiceDesc {
            size: size_of::<erui::ChoiceDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            changed_callback: Some(on_choice),
            user_data: id as *mut c_void,
            options: views.as_ptr(),
            option_count: views.len() as u32,
            initial_index: initial as u8,
            reserved8: [0; 3],
        };
        let mut row = 0;
        let r = (self.api.add_inline_choice.unwrap())(self.provider, self.page, &d, &mut row);
        self.check(label, r);
    }

    #[allow(clippy::too_many_arguments)]
    fn numeric(&mut self, label: &str, help: &str, id: usize, min: i64, max: i64, step: i64, initial: i64, suffix: &str) {
        let (l, h, s) = (erui::w(label), erui::w(help), erui::w(suffix));
        let d = erui::NumericSliderDesc {
            size: size_of::<erui::NumericSliderDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            changed_callback: Some(on_numeric),
            user_data: id as *mut c_void,
            minimum_scaled_value: min,
            maximum_scaled_value: max,
            step_scaled_value: step,
            initial_scaled_value: (initial.clamp(min, max) - min) / step * step + min,
            decimal_places: 0,
            enabled: 1,
            prefix: Utf16View::EMPTY,
            suffix: if suffix.is_empty() { Utf16View::EMPTY } else { Utf16View::new(&s) },
            reserved: [0; 2],
        };
        let mut row = 0;
        let r = (self.api.add_numeric_slider.unwrap())(self.provider, self.page, &d, &mut row);
        self.check(label, r);
    }

    fn custom(&mut self, label: &str, help: &str, id: usize, values: &[&str]) -> erui::RowHandle {
        self.custom_ex(label, help, id, values, true)
    }

    fn custom_ex(&mut self, label: &str, help: &str, id: usize, values: &[&str], enabled: bool) -> erui::RowHandle {
        let (l, h) = (erui::w(label), erui::w(help));
        let texts: Vec<Vec<u16>> = values.iter().map(|v| erui::w(v)).collect();
        let views: Vec<Utf16View> = texts.iter().map(|t| Utf16View::new(t)).collect();
        let d = erui::CustomSliderDesc {
            size: size_of::<erui::CustomSliderDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            changed_callback: Some(on_custom),
            user_data: id as *mut c_void,
            values: views.as_ptr(),
            value_count: views.len() as u32,
            initial_index: 0,
            enabled: enabled as u32,
            reserved: 0,
        };
        let mut row = 0;
        let r = (self.api.add_custom_slider.unwrap())(self.provider, self.page, &d, &mut row);
        self.check(label, r);
        row
    }

    fn text(&mut self, label: &str, help: &str, id: usize, initial: &str, placeholder: &str) -> erui::RowHandle {
        self.text_len(label, help, id, initial, placeholder, 12)
    }

    fn text_len(
        &mut self,
        label: &str,
        help: &str,
        id: usize,
        initial: &str,
        placeholder: &str,
        max_len: u32,
    ) -> erui::RowHandle {
        let (l, h, i, p) = (erui::w(label), erui::w(help), erui::w(initial), erui::w(placeholder));
        let d = erui::TextInputDesc {
            size: size_of::<erui::TextInputDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            initial_value: if initial.is_empty() { Utf16View::EMPTY } else { Utf16View::new(&i) },
            placeholder: if placeholder.is_empty() { Utf16View::EMPTY } else { Utf16View::new(&p) },
            changed_callback: Some(on_text),
            user_data: id as *mut c_void,
            maximum_length: max_len,
            reserved: 0,
        };
        let mut row = 0;
        let r = (self.api.add_text_input.unwrap())(self.provider, self.page, &d, &mut row);
        self.check(label, r);
        row
    }

    fn submenu(&mut self, label: &str, help: &str, title: &str) -> erui::PageHandle {
        let (l, h, t) = (erui::w(label), erui::w(help), erui::w(title));
        let d = erui::SubmenuDesc {
            size: size_of::<erui::SubmenuDesc>() as u32,
            flags: 0,
            label: Utf16View::new(&l),
            help: Utf16View::new(&h),
            page_title: Utf16View::new(&t),
            page_help: Utf16View::new(&h),
            enabled: 1,
            reserved: 0,
        };
        let (mut page, mut row) = (0, 0);
        let r = (self.api.add_submenu.unwrap())(self.provider, self.page, &d, &mut page, &mut row);
        self.check(label, r);
        page
    }
}

fn id_text(v: i32, none: i32) -> String {
    if v == none { String::new() } else { v.to_string() }
}

/// Connects to ERNativeUI and registers the tab (from a worker thread).
pub fn register(module: HMODULE, initial: Config) {
    let Some(api) = connect() else { return };
    crate::talk::register();
    let caps = api.capabilities;
    let own_tab = initial.own_tab;
    let needed = (if own_tab { erui::CAP_CUSTOM_TABS } else { 0 })
        | erui::CAP_NUMERIC_SLIDER
        | erui::CAP_CUSTOM_SLIDER
        | erui::CAP_TEXT_INPUT;
    let fns_ok = api.register_provider.is_some()
        && api.add_button.is_some()
        && api.add_toggle.is_some()
        && api.add_inline_choice.is_some()
        && api.add_submenu.is_some()
        && api.commit_provider.is_some()
        && api.abort_provider.is_some()
        && api.add_text_input.is_some()
        && api.add_numeric_slider.is_some()
        && api.add_custom_slider.is_some()
        && (!own_tab || api.add_tab.is_some());
    if caps & needed != needed || !fns_ok {
        log(format!("menu: ERNativeUI lacks custom tabs / numeric sliders / text input (caps {caps:#x})"));
        return;
    }

    let name = erui::w("NPC Spawner");
    let pd = erui::ProviderDesc {
        size: size_of::<erui::ProviderDesc>() as u32,
        api_version: api.api_version,
        flags: 0,
        root_priority: 100,
        owner_module: module.0,
        provider_id: erui::StringView::new("npc-spawner"),
        display_name: Utf16View::new(&name),
    };
    let (mut provider, mut root) = (0, 0);
    let r = (api.register_provider.unwrap())(&pd, &mut provider, &mut root);
    if r != erui::OK {
        log(format!(
            "menu: register_provider failed ({r}){}",
            if r == 8 {
                ": ERNativeUI had already closed menu registration (this DLL loaded too late; \
                 load it right after ERNativeUI.dll and NativeTweaks.dll)"
            } else {
                ""
            }
        ));
        return;
    }

    let tab = if own_tab {
        let pixels = icon();
        let icon = erui::TabIconDesc {
            size: size_of::<erui::TabIconDesc>() as u32,
            flags: 0,
            format: erui::TAB_ICON_FORMAT_RGBA8,
            byte_count: pixels.len() as u32,
            pixels: pixels.as_ptr(),
            reserved: [0; 2],
        };
        let title = erui::w("NPC Spawner");
        let td = erui::TabDesc {
            size: size_of::<erui::TabDesc>() as u32,
            flags: 0,
            tab_id: erui::StringView::new("spawner"),
            title: Utf16View::new(&title),
            preferred_order: 200,
            reserved0: 0,
            icon: &icon,
            reserved: [0; 2],
        };
        let mut tab = 0;
        let r = (api.add_tab.unwrap())(provider, &td, &mut tab);
        if r != erui::OK {
            log(format!("menu: add_tab failed ({r})"));
            (api.abort_provider.unwrap())(provider);
            return;
        }
        tab
    } else {
        // One entry in Mod Settings that opens the whole menu.
        let mut b = Builder { api: &api, provider, page: root, failed: None };
        let page = b.submenu(
            "NPC Spawner",
            "Spawn any NPC or enemy to test settings and mods. Offline only.",
            "NPC Spawner",
        );
        if let Some(e) = b.failed {
            log(format!("menu: entry failed ({e})"));
            (api.abort_provider.unwrap())(provider);
            return;
        }
        page
    };

    let c = initial;
    let mut b = Builder { api: &api, provider, page: tab, failed: None };
    // A read-only text box: its value repaints at once (row labels only when
    // the page is reopened, with the NativeTweaks fork r156).
    // A text box used as a display: its value repaints at once (row labels
    // only when the page is reopened, fork r156). ERNativeUI has no read-only
    // text box, so anything typed there is replaced by the status at once.
    let status_row = b.text_len(
        "Status",
        "Read only: characters spawned and what the last button did. Spawning uses the game's debug character creator; offline only.",
        X_STATUS,
        "Nothing spawned",
        "",
        erui::TEXT_INPUT_MAX_LENGTH,
    );
    b.text(
        "Search by name",
        "Type part of a name (any language of the game, or English): the first match is chosen; pick another one below. Empty: everything the Search filters allow.",
        X_SEARCH,
        "",
        "e.g. Margit",
    );

    let mut placeholders = vec!["no search yet"];
    placeholders.extend(std::iter::repeat_n("-", RESULT_SLOTS - 1));
    let result_row = b.custom(
        "Result",
        "Matches of the search: the chosen one is spawned. Names from the game's texts and Smithbox (MIT).",
        S_RESULT,
        &placeholders,
    );
    let selected_row = b.text_len(
        "Selected",
        "Read only: the match chosen in Result. Spawn below spawns it.",
        X_SELECTED,
        "none yet",
        "",
        erui::TEXT_INPUT_MAX_LENGTH,
    );
    // Main page: what every test uses. The rest is in three submenus.
    b.button("Spawn", "Spawn the selected character with the current settings.", 40);
    b.numeric("How many", "Characters per spawn.", N_COUNT, 1, 20, 1, c.count as i64, "");
    b.choice(
        "Side",
        "Hostile, on your side (the spirit-ash team) or Neutral (does not fight you; team from NeutralTeam in the INI). For a dummy you can hit, Hostile with AI Still is the sure way. Changing it also changes the characters already spawned: they appear again in place, so they forget you as a target.",
        C_TEAM,
        &["Hostile", "Ally", "Neutral"],
        TEAMS.iter().position(|t| *t == c.team).unwrap_or(0),
    );
    b.choice(
        "AI",
        "Active, Paused (frozen entirely) or Still (animated, but does not move or attack). Also applies to the characters already spawned.",
        C_AI,
        &["Active", "Paused", "Still"],
        AIS.iter().position(|a| *a == c.ai).unwrap_or(0),
    );
    b.button("Reset all", "Heal the living characters and move them back; the dead ones spawn again.", 43);
    b.button("Remove all", "Removes every spawned character.", 45);

    let root = b.page;
    let filters = b.submenu(
        "Search filters",
        "Narrow the search by type of character and by the zone of the game's maps where it is placed. With the name empty, Result lists everything they allow.",
        "Search filters",
    );
    b.page = filters;
    let mut types = vec!["All types"];
    types.extend(crate::catalog::TYPES.iter().map(|t| t.0));
    b.choice("Type", "Bosses, enemies, invaders and hostile NPCs, spirit ashes and allies, or other NPCs.", C_TYPE, &types, 0);
    let mut areas = vec!["Every area"];
    areas.extend(crate::catalog::areas().iter().copied());
    b.choice(
        "Area",
        "Where in the game's maps: the Lands Between, the Realm of Shadow, legacy dungeons, catacombs, caves... Zone below picks one zone of it.",
        C_AREA,
        &areas,
        0,
    );
    let zones = zones_for(None);
    *ZONE_SLOTS.lock().unwrap_or_else(|e| e.into_inner()) = zones.clone();
    let zone_texts = zone_values(&zones);
    let zone_refs: Vec<&str> = zone_texts.iter().map(|s| s.as_str()).collect();
    let zone_row = b.custom(
        "Zone",
        "One zone of the area (its name is below). Zones and placements come from the game's maps; zone names from Smithbox (MIT).",
        S_ZONE,
        &zone_refs,
    );
    let zone_name_row = b.text_len(
        "Zone name",
        "Read only: the zone chosen above.",
        X_ZONE_NAME,
        "All zones",
        "",
        erui::TEXT_INPUT_MAX_LENGTH,
    );
    b.choice(
        "Show",
        "One per character: a row per name and model (the one placed in the maps first), without unnamed rows, dummies, corpses and characters that do not fight. Every row: all 7000 NpcParam rows, area variants included.",
        C_SHOW,
        &["One per character", "Every row"],
        c.search_all as usize,
    );

    b.page = root;
    let spawn = b.submenu(
        "Spawn options",
        "Where and how characters appear, their health, and the last one.",
        "Spawn options",
    );
    b.page = spawn;
    b.choice(
        "Where",
        "In front of you, on you, on your lock-on target, on the saved point, or at random points around you.",
        C_PLACEMENT,
        &["In front", "On me", "Lock-on target", "Saved point", "Random"],
        PLACEMENTS.iter().position(|p| *p == c.placement).unwrap_or(0),
    );
    b.numeric("Distance", "Metres in front of you (radius for Random).", N_DISTANCE, 1, 30, 1, c.distance as i64, " m");
    b.choice(
        "Formation",
        "How several characters are laid out.",
        C_FORMATION,
        &["Line", "Circle", "Grid"],
        FORMATIONS.iter().position(|f| *f == c.formation).unwrap_or(0),
    );
    b.toggle("Face me", "Spawned characters turn towards you.", T_FACE, c.face_player);
    b.numeric("Health at spawn", "Percentage of the character's maximum health.", N_HP, 10, 100, 10, c.hp_percent as i64, " %");
    b.toggle("Infinite health", "Health is refilled when it drops to 10 %.", T_INFINITE, c.infinite_hp);
    b.toggle("Invulnerable", "Spawned characters take no damage.", T_INVULNERABLE, c.invulnerable);
    b.numeric(
        "Allies follow me",
        "An ally (Side: Ally, AI: Active) left farther than this for 4 s is brought behind you. They do not walk after you. 0 = off: allies stay where they are.",
        N_FOLLOW,
        0,
        60,
        5,
        c.follow_distance as i64,
        " m",
    );
    b.button("Save the spawn point here", "Your position becomes the Saved point (Where: Saved point).", 42);
    b.button("Spawn the last one again", "Same character and settings as the last spawn, at your current position.", 41);
    b.button("Remove the last one", "Removes the most recent character.", 44);

    b.page = root;
    let adv = b.submenu(
        "Advanced and limits",
        "The NpcParam ID by hand, model, AI and equipment overrides, the game's character limits, and saving the settings.",
        "Advanced and limits",
    );
    b.page = adv;
    let npc_row = b.text(
        "NpcParam ID",
        "Which character to spawn (the name search fills it in). Empty or 0 copies the nearest living enemy within 30 m.",
        X_NPC,
        &id_text(c.npc_param_id, 0),
        "nearest enemy",
    );
    let think_row = b.text(
        "NpcThinkParam ID (AI)",
        "Empty or 0: the copied character's AI, or the NpcParam ID's own. The search fills it in when an area variant has no AI of its own (its base row's AI).",
        X_THINK,
        &id_text(c.think_id, 0),
        "automatic",
    );
    b.text("Model (cXXXX)", "Empty or 0: from the NpcParam ID.", X_CHR, &id_text(c.chr_id, 0), "automatic");
    b.text("CharaInitParam ID", "Equipment and look of human NPCs. Empty: none.", X_INIT, &id_text(c.chara_init_id, -1), "none");
    b.numeric("Most spawned at once", "Spawning stops at this number (the debug character set holds 76).", N_MAX_SPAWNED, 1, 76, 1, c.max_spawned as i64, "");
    b.numeric(
        "Active character limit",
        "Experimental. The game keeps about 60 characters active and switches map enemies off for every spawned one. Raising it keeps them, at a performance cost. 0 = the game's own; lowering needs a restart.",
        N_MAX_ACTIVE,
        0,
        300,
        10,
        c.max_active as i64,
        "",
    );
    b.toggle(
        "Free the slots of removed characters",
        "Removed characters are destroyed a second later. Off: they are only hidden until the next loading screen.",
        T_DESTROY,
        c.destroy_removed,
    );
    b.button("Save these settings", "Writes the current settings to NpcSpawner.ini as the defaults.", 46);

    b.page = root;
    let keys = b.submenu(
        "Hotkeys",
        "Your own keys for each action, with Ctrl, Shift and / or Alt or none: Ctrl+F6, Shift+K, Alt+Num5, F9, Mouse4. None = no hotkey. They apply at once; Save these settings keeps them.",
        "Hotkeys",
    );
    b.page = keys;
    let hotkeys_toggle = b.toggle("Hotkeys on", "Off: no hotkey does anything (the menus still work).", T_HOTKEYS, c.hotkeys_on);
    let mut hotkey_rows = [0; crate::hotkeys::COUNT];
    for (i, (_, label)) in crate::hotkeys::ACTIONS.iter().enumerate() {
        let help = if i == crate::hotkeys::TALK_MENU {
            "Opens the talk menu. With NativeTweaks r157 or later it does nothing: NPC Spawner is in NativeTweaks' talk menu (F11)."
        } else {
            "Type the keys, e.g. Ctrl+F6, Shift+K, F9, Mouse4, or None. The talk menu can also assign them by pressing them."
        };
        hotkey_rows[i] = b.text_len(label, help, X_HOTKEY + i, &c.hotkeys[i].name(), "None", 24);
    }

    // Display rows: read-only with a fork that has it (else typed text is
    // replaced by the value at once, in on_text).
    let read_only = unsafe { GetModuleHandleW(w!("ERNativeUI.dll")) }
        .ok()
        .and_then(|h| unsafe { GetProcAddress(h, PCSTR(b"ERNativeUIFork_SetTextInputReadOnly\0".as_ptr())) })
        .map(|f| unsafe { std::mem::transmute::<_, erui::SetTextInputReadOnlyFn>(f) });
    match read_only {
        Some(f) if b.failed.is_none() => {
            let r1 = f(provider, status_row, 1);
            let r2 = f(provider, selected_row, 1);
            let r3 = f(provider, zone_name_row, 1);
            log(format!("menu: Status / Selected / Zone name read-only ({r1} / {r2} / {r3})"));
        }
        Some(_) => {}
        None => log("menu: ERNativeUI has no read-only text rows; Status / Selected restore their value instead"),
    }

    if let Some(e) = b.failed {
        log(format!("menu: building the tab failed ({e})"));
        (api.abort_provider.unwrap())(provider);
        return;
    }
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(initial);
    let r = (api.commit_provider.unwrap())(provider);
    if r != erui::OK {
        *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        log(format!("menu: commit failed ({r})"));
        return;
    }
    let set_row_text = unsafe { GetModuleHandleW(w!("ERNativeUI.dll")) }
        .ok()
        .and_then(|h| unsafe { GetProcAddress(h, PCSTR(b"ERNativeUIFork_SetRowText\0".as_ptr())) })
        .map(|f| unsafe { std::mem::transmute::<_, erui::SetRowTextFn>(f) });
    log(format!(
        "menu: registered as {} (live status {})",
        if own_tab { "its own System tab" } else { "an entry in Mod Settings" },
        if set_row_text.is_some() { "on" } else { "off: official ERNativeUI" }
    ));
    let set_slider_values = unsafe { GetModuleHandleW(w!("ERNativeUI.dll")) }
        .ok()
        .and_then(|h| unsafe { GetProcAddress(h, PCSTR(b"ERNativeUIFork_SetCustomSliderValues\0".as_ptr())) })
        .map(|f| unsafe { std::mem::transmute::<_, erui::SetCustomSliderValuesFn>(f) });
    let _ = MENU.set(Menu {
        api,
        provider,
        status_row,
        npc_row,
        think_row,
        result_row,
        selected_row,
        zone_row,
        zone_name_row,
        set_row_text,
        set_slider_values,
        hotkeys_toggle,
        hotkey_rows,
    });
}

/// Shows the hotkeys in use in the tab's rows (after a change from the
/// talk menu or the INI).
pub fn show_hotkeys() {
    let (Some(m), Some(c)) = (MENU.get(), settings()) else { return };
    if let Some(set) = m.api.set_text_input_value {
        for (i, row) in m.hotkey_rows.iter().enumerate() {
            let t = erui::w(&c.hotkeys[i].name());
            let v = Utf16View::new(&t);
            set(m.provider, *row, &v);
        }
    }
    if let Some(set) = m.api.set_row_value {
        set(m.provider, m.hotkeys_toggle, c.hotkeys_on as u8);
    }
}
