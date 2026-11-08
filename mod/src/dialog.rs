//! The talk menu (F11): the game's own talk-list dialog (as a
//! merchant's), driven from Rust through TalkScript (fromsoftware-rs
//! examples/invoke-esd) with our own texts (texts.rs). It works without
//! ERNativeUI: every setting of a spawn, and lists of every character by
//! type or by the zone of the maps where it is placed, a page at a time.
//! Every choice reopens a page, except Leave.

use eldenring::{
    cs::{BlockId, EzStateInvokeError, FieldInsHandle, MenuType, TalkScript},
    ez_state::EzStateValue,
};

use crate::{
    arena::Template,
    catalog,
    config::{AiMode, Config, Formation, Placement},
    hotkeys, log, texts,
};

const SHOW_SHOP_MESSAGE: i32 = 10;
const ADD_TALK_LIST_DATA: i32 = 19;
const CLEAR_TALK_LIST_DATA: i32 = 20;
const GET_TALK_LIST_MENU_RESULT: i32 = 23;
const CHECK_SPECIFIC_PERSON_GENERIC_DIALOG_IS_OPEN: i32 = 58;
const CHECK_SPECIFIC_PERSON_MENU_IS_OPEN: i32 = 59;
/// The game's "Leave" text, used when our texts are unavailable.
const MSG_LEAVE: i32 = 20000009;
/// Characters per page of a group list.
const PER_PAGE: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Main,
    Options,
    Pick,
    /// Types of character of a place (bosses, enemies...), with counts.
    Kinds(Scope),
    Areas,
    /// The zones of an area.
    Zones(usize),
    /// A list, page n.
    List(ListKind, usize),
    /// Every hotkey.
    Hotkeys,
    /// One hotkey (hotkeys::ACTIONS index).
    Hotkey(usize),
}

/// Where a list looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// The whole game.
    All,
    /// Every zone of an area.
    Area(usize),
    /// One zone.
    Zone(u16),
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Scope::All => "the game",
            Scope::Area(a) => catalog::areas().get(a).copied().unwrap_or(""),
            Scope::Zone(z) => catalog::zones().get(z as usize).map_or("", |z| z.name),
        }
    }

    /// The page Back returns to from its types.
    fn parent(self) -> Page {
        match self {
            Scope::All => Page::Pick,
            Scope::Area(a) => Page::Zones(a),
            Scope::Zone(z) => Page::Zones(area_of(z)),
        }
    }
}

/// What a list shows: a place and a type of catalog::TYPES (None = all,
/// sorted by type).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListKind {
    pub scope: Scope,
    pub kind: Option<usize>,
}

impl ListKind {
    fn filter(self, all: bool) -> catalog::Filter {
        let mut f = catalog::Filter { all, kind: self.kind, ..Default::default() };
        match self.scope {
            Scope::All => {}
            Scope::Area(a) => f.area = Some(a),
            Scope::Zone(z) => f.zone = Some(z),
        }
        f
    }
}

fn area_of(zone: u16) -> usize {
    let name = catalog::zones().get(zone as usize).map_or("", |z| z.area);
    catalog::areas().iter().position(|a| *a == name).unwrap_or(0)
}

/// What a dialog choice asks for; lib.rs runs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Spawn,
    Count,
    Placement,
    Ai,
    Team,
    Repeat,
    RemoveLast,
    Reset,
    RemoveAll,
    Formation,
    Distance,
    Face,
    Health,
    InfiniteHp,
    Invulnerable,
    SavePoint,
    Follow,
    ShowAll,
    SaveIni,
    PickNearest,
    PickRecent(usize),
    PickHit(usize),
    /// NpcParam id and AI (0 = the id) from a group list.
    PickListed(u32, u32),
    OpenOptions,
    OpenPick,
    OpenKinds(Scope),
    OpenAreas,
    OpenZones(usize),
    /// A list, page to show.
    Turn(ListKind, usize),
    OpenHotkeys,
    OpenHotkey(usize),
    /// The menu closes and the next keys pressed become this hotkey.
    Capture(usize),
    /// This hotkey: none.
    Unbind(usize),
    HotkeysOn,
    HotkeysDefault,
    Back,
    Leave,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Enter(Page),
    Open(Page),
}

pub struct Dialog {
    talk: Option<Box<TalkScript>>,
    state: State,
    entries: Vec<Choice>,
}

unsafe impl Send for Dialog {}

impl Default for Dialog {
    fn default() -> Self {
        Dialog { talk: None, state: State::Idle, entries: Vec::new() }
    }
}

pub struct View<'a> {
    pub cfg: &'a Config,
    pub spawned: usize,
    pub recent: &'a [Template],
    /// Results of the last name search in the menu.
    pub hits: &'a [String],
    pub has_last: bool,
    pub describe: &'a dyn Fn(&Template) -> String,
}

fn placement_name(p: Placement) -> &'static str {
    match p {
        Placement::Front => "in front of me",
        Placement::Player => "on me",
        Placement::Target => "on my target",
        Placement::Saved => "on the saved point",
        Placement::Random => "at random around me",
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

pub fn next_count(n: u32) -> u32 {
    match n {
        0..=1 => 2,
        2 => 3,
        3..=4 => 5,
        5..=9 => 10,
        10..=19 => 20,
        _ => 1,
    }
}

pub fn next_placement(p: Placement) -> Placement {
    match p {
        Placement::Front => Placement::Player,
        Placement::Player => Placement::Target,
        Placement::Target => Placement::Saved,
        Placement::Saved => Placement::Random,
        Placement::Random => Placement::Front,
    }
}

pub fn next_formation(f: Formation) -> Formation {
    match f {
        Formation::Line => Formation::Circle,
        Formation::Circle => Formation::Grid,
        Formation::Grid => Formation::Line,
    }
}

pub fn next_distance(d: f32) -> f32 {
    [2.0, 5.0, 10.0, 15.0, 20.0, 30.0].into_iter().find(|x| *x > d + 0.01).unwrap_or(1.0)
}

pub fn next_follow(d: f32) -> f32 {
    [10.0, 20.0, 30.0, 50.0].into_iter().find(|x| *x > d + 0.01).unwrap_or(0.0)
}

pub fn next_health(p: u32) -> u32 {
    match p {
        100.. => 10,
        75.. => 100,
        50.. => 75,
        25.. => 50,
        _ => 25,
    }
}

impl Dialog {
    pub fn is_open(&self) -> bool {
        self.state != State::Idle
    }

    pub fn open(&mut self) {
        self.open_at(Page::Main);
    }

    pub fn open_at(&mut self, page: Page) {
        if self.state == State::Idle {
            self.state = State::Enter(page);
        }
    }

    pub fn close(&mut self) {
        self.state = State::Idle;
        texts::unhook();
    }

    pub fn rows(page: Page, v: &View) -> Vec<(Choice, String)> {
        let c = v.cfg;
        match page {
            Page::Main => {
                let what = if c.npc_param_id > 0 {
                    (v.describe)(&Template {
                        chr_id: Config::resolve_chr_id(c.npc_param_id, c.chr_id),
                        npc_param_id: c.npc_param_id,
                        think_id: c.think_id,
                        chara_init_id: c.chara_init_id,
                    })
                } else {
                    "a copy of the nearest enemy".to_string()
                };
                let mut r = Vec::new();
                r.extend([
                    (Choice::Spawn, format!("Spawn {}{what}", if c.count > 1 { format!("{} x ", c.count) } else { String::new() })),
                    (Choice::OpenPick, "Choose what to spawn...".to_string()),
                    (Choice::Count, format!("How many: {}", c.count)),
                    (Choice::Placement, format!("Where: {}", placement_name(c.placement))),
                    (Choice::Team, format!("Side: {}", c.team.name())),
                    (
                        Choice::Ai,
                        format!(
                            "AI: {}",
                            match c.ai {
                                AiMode::Active => "active",
                                AiMode::Paused => "paused",
                                AiMode::Still => "still",
                            }
                        ),
                    ),
                    (Choice::OpenOptions, "Spawn options...".to_string()),
                ]);
                if v.has_last {
                    r.push((Choice::Repeat, "Spawn the last one again".to_string()));
                }
                if v.spawned > 0 {
                    r.push((Choice::RemoveLast, "Remove the last one".to_string()));
                    r.push((Choice::Reset, format!("Reset all ({} spawned)", v.spawned)));
                    r.push((Choice::RemoveAll, "Remove all".to_string()));
                }
                r.push((Choice::Leave, "Leave".to_string()));
                r
            }
            Page::Options => vec![
                (
                    Choice::Formation,
                    format!(
                        "Formation: {}",
                        match c.formation {
                            Formation::Line => "line",
                            Formation::Circle => "circle",
                            Formation::Grid => "grid",
                        }
                    ),
                ),
                (Choice::Distance, format!("Distance: {} m", c.distance)),
                (Choice::Face, format!("Face me: {}", on_off(c.face_player))),
                (Choice::Health, format!("Health at spawn: {} %", c.hp_percent)),
                (Choice::InfiniteHp, format!("Infinite health: {}", on_off(c.infinite_hp))),
                (Choice::Invulnerable, format!("Invulnerable: {}", on_off(c.invulnerable))),
                (
                    Choice::Follow,
                    if c.follow_distance > 0.0 {
                        format!("Allies follow me: beyond {} m", c.follow_distance)
                    } else {
                        "Allies follow me: off".to_string()
                    },
                ),
                (Choice::SavePoint, "Save the spawn point here".to_string()),
                (
                    Choice::ShowAll,
                    format!("Lists and search: {}", if c.search_all { "every row" } else { "one per character" }),
                ),
                (Choice::OpenHotkeys, "Hotkeys...".to_string()),
                (Choice::SaveIni, "Save these settings".to_string()),
                (Choice::Back, "Back".to_string()),
            ],
            Page::Hotkeys => {
                let mut r = Vec::new();
                r.push((Choice::HotkeysOn, format!("Hotkeys: {}", on_off(c.hotkeys_on))));
                for (i, (_, label)) in hotkeys::ACTIONS.iter().enumerate() {
                    if i == hotkeys::DUMP && !c.research {
                        continue;
                    }
                    let note = if i == hotkeys::TALK_MENU && crate::talk::registered() {
                        " (off: F11 menu)"
                    } else {
                        ""
                    };
                    r.push((Choice::OpenHotkey(i), format!("{label}: {}{note}", c.hotkeys[i].name())));
                }
                r.push((Choice::HotkeysDefault, "All back to the defaults".to_string()));
                r.push((Choice::Back, "Back".to_string()));
                r
            }
            Page::Hotkey(i) => {
                let i = i.min(hotkeys::COUNT - 1);
                vec![
                    (
                        Choice::Capture(i),
                        format!(
                            "New keys (now {}): menu closes, press them in 10 s",
                            c.hotkeys[i].name()
                        ),
                    ),
                    (Choice::Unbind(i), "No hotkey".to_string()),
                    (Choice::Back, "Back".to_string()),
                ]
            }
            Page::Pick => {
                let mut r = vec![
                    (Choice::PickNearest, "A copy of the nearest enemy".to_string()),
                    (Choice::OpenKinds(Scope::All), "By type...".to_string()),
                    (Choice::OpenAreas, "By zone of the game...".to_string()),
                ];
                for (i, h) in v.hits.iter().enumerate().take(10) {
                    r.push((Choice::PickHit(i), format!("Search: {h}")));
                }
                for (i, t) in v.recent.iter().enumerate().take(8) {
                    r.push((Choice::PickRecent(i), format!("Recent: {}", (v.describe)(t))));
                }
                r.push((Choice::Back, "Back".to_string()));
                r
            }
            Page::Kinds(scope) => {
                let mut r = Vec::new();
                if scope != Scope::All {
                    let k = ListKind { scope, kind: None };
                    let n = catalog::count(k.filter(c.search_all));
                    r.push((Choice::Turn(k, 0), format!("Everything in {} ({n})", scope.name())));
                }
                for (i, (name, _)) in catalog::TYPES.iter().enumerate() {
                    let k = ListKind { scope, kind: Some(i) };
                    let n = catalog::count(k.filter(c.search_all));
                    if n > 0 {
                        r.push((Choice::Turn(k, 0), format!("{name} ({n})")));
                    }
                }
                r.push((Choice::Back, "Back".to_string()));
                r
            }
            Page::Areas => {
                let mut r = Vec::new();
                for (a, name) in catalog::areas().iter().enumerate() {
                    let n = catalog::count(ListKind { scope: Scope::Area(a), kind: None }.filter(c.search_all));
                    if n > 0 {
                        r.push((Choice::OpenZones(a), format!("{name} ({n})...")));
                    }
                }
                r.push((Choice::Back, "Back".to_string()));
                r
            }
            Page::Zones(a) => {
                let area = catalog::areas().get(a).copied().unwrap_or("");
                let all = catalog::count(ListKind { scope: Scope::Area(a), kind: None }.filter(c.search_all));
                let mut r = vec![(Choice::OpenKinds(Scope::Area(a)), format!("All of {area} ({all})..."))];
                for z in catalog::zones_of_area(a) {
                    let n = catalog::count(ListKind { scope: Scope::Zone(z), kind: None }.filter(c.search_all));
                    if n > 0 {
                        let name = catalog::zones()[z as usize].name;
                        r.push((Choice::OpenKinds(Scope::Zone(z)), format!("{name} ({n})...")));
                    }
                }
                r.push((Choice::Back, "Back".to_string()));
                r
            }
            Page::List(kind, p) => {
                let all = catalog::list(kind.filter(c.search_all));
                let pages = all.len().div_ceil(PER_PAGE).max(1);
                let p = p.min(pages - 1);
                let mut r = Vec::new();
                if p + 1 < pages {
                    r.push((Choice::Turn(kind, p + 1), format!("Next page ({}/{pages})", p + 2)));
                }
                if p > 0 {
                    r.push((Choice::Turn(kind, p - 1), format!("Previous page ({p}/{pages})")));
                }
                if pages > 10 {
                    let jump = (p + 10) % pages;
                    r.push((Choice::Turn(kind, jump), format!("Ten pages on ({}/{pages})", jump + 1)));
                }
                for h in all.iter().skip(p * PER_PAGE).take(PER_PAGE) {
                    r.push((Choice::PickListed(h.id, h.think), h.label.clone()));
                }
                r.push((Choice::Back, "Back".to_string()));
                r
            }
        }
    }

    /// Page shown after a choice on `page` (None = the menu closes).
    pub fn next_page(page: Page, choice: Choice) -> Option<Page> {
        Some(match choice {
            Choice::Leave => return None,
            Choice::OpenPick => Page::Pick,
            Choice::OpenOptions => Page::Options,
            Choice::OpenKinds(scope) => Page::Kinds(scope),
            Choice::OpenAreas => Page::Areas,
            Choice::OpenZones(a) => Page::Zones(a),
            Choice::Turn(k, p) => Page::List(k, p),
            Choice::OpenHotkeys => Page::Hotkeys,
            Choice::OpenHotkey(i) => Page::Hotkey(i),
            Choice::Capture(_) => return None,
            Choice::Unbind(_) => Page::Hotkeys,
            Choice::HotkeysOn | Choice::HotkeysDefault => page,
            Choice::Back => match page {
                Page::List(k, _) => Page::Kinds(k.scope),
                Page::Kinds(scope) => scope.parent(),
                Page::Zones(_) => Page::Areas,
                Page::Areas => Page::Pick,
                Page::Hotkeys => Page::Options,
                Page::Hotkey(_) => Page::Hotkeys,
                _ => Page::Main,
            },
            // Settings of the options page keep it open.
            _ if page == Page::Options => Page::Options,
            _ => Page::Main,
        })
    }

    /// One step per frame. Returns the player's choice when the menu closes.
    pub fn step(&mut self, player: FieldInsHandle, view: &View) -> Option<Choice> {
        if self.state == State::Idle {
            return None;
        }
        let talk = self.talk.get_or_insert_with(|| Box::new(TalkScript::new(BlockId::none(), 1000, player)));
        talk.npc_talk.base.field_ins_handle = player;
        let result: Result<Option<Choice>, EzStateInvokeError> = (|| match self.state {
            State::Enter(page) => {
                talk.event(CLEAR_TALK_LIST_DATA)?;
                let rows = Self::rows(page, view);
                self.entries.clear();
                let hooked = texts::hook_for_dialog();
                for (i, (choice, text)) in rows.into_iter().enumerate() {
                    let msg = if hooked && i < texts::SLOTS {
                        texts::set(i, &text);
                        texts::id(i)
                    } else if choice == Choice::Leave {
                        MSG_LEAVE
                    } else {
                        continue;
                    };
                    self.entries.push(choice);
                    let index = self.entries.len() as i32;
                    talk.event((
                        ADD_TALK_LIST_DATA,
                        [EzStateValue::Int32(index), EzStateValue::Int32(msg), EzStateValue::Int32(-1)],
                    ))?;
                }
                talk.event((SHOW_SHOP_MESSAGE, [EzStateValue::Int32(1)]))?;
                self.state = State::Open(page);
                Ok(None)
            }
            State::Open(page) => {
                let menu: i32 = talk
                    .env((
                        CHECK_SPECIFIC_PERSON_MENU_IS_OPEN,
                        [EzStateValue::Int32(MenuType::TalkList as i32), EzStateValue::Int32(0)],
                    ))?
                    .into();
                let generic: i32 = talk
                    .env((CHECK_SPECIFIC_PERSON_GENERIC_DIALOG_IS_OPEN, [EzStateValue::Int32(0)]))?
                    .into();
                if menu == 1 && generic == 0 {
                    return Ok(None);
                }
                let r: i32 = talk.env(GET_TALK_LIST_MENU_RESULT)?.into();
                let choice = (r >= 1).then(|| self.entries.get(r as usize - 1).copied()).flatten();
                // Closed without a choice (Q): back a page, as in NativeTweaks'
                // talk menu; on the first page it closes.
                if choice.is_none() && page != Page::Main {
                    if let Some(p) = Self::next_page(page, Choice::Back) {
                        self.state = State::Enter(p);
                        return Ok(None);
                    }
                }
                self.state = match choice.and_then(|c| Self::next_page(page, c)) {
                    Some(p) => State::Enter(p),
                    None => {
                        texts::unhook();
                        State::Idle
                    }
                };
                Ok(choice)
            }
            State::Idle => Ok(None),
        })();
        match result {
            Ok(c) => c,
            Err(e) => {
                log(format!("dialog error: {e:?}"));
                self.state = State::Idle;
                texts::unhook();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_wrap() {
        assert_eq!(next_health(next_health(next_health(next_health(next_health(100))))), 100);
        assert_eq!(next_distance(30.0), 1.0);
        assert_eq!(next_follow(50.0), 0.0);
        assert_eq!(next_follow(0.0), 10.0);
        assert_eq!(next_distance(5.0), 10.0);
        assert_eq!(next_formation(next_formation(next_formation(Formation::Line))), Formation::Line);
    }

    #[test]
    fn pages_after_choices() {
        assert_eq!(Dialog::next_page(Page::Main, Choice::Leave), None);
        assert_eq!(Dialog::next_page(Page::Options, Choice::Face), Some(Page::Options));
        let zone = ListKind { scope: Scope::Zone(0), kind: Some(1) };
        assert_eq!(Dialog::next_page(Page::List(zone, 3), Choice::Back), Some(Page::Kinds(Scope::Zone(0))));
        assert_eq!(Dialog::next_page(Page::Kinds(Scope::Zone(0)), Choice::Back), Some(Page::Zones(area_of(0))));
        assert_eq!(Dialog::next_page(Page::Kinds(Scope::All), Choice::Back), Some(Page::Pick));
        assert_eq!(Dialog::next_page(Page::List(zone, 3), Choice::PickListed(1, 0)), Some(Page::Main));
        assert_eq!(Dialog::next_page(Page::Zones(2), Choice::Back), Some(Page::Areas));
        assert_eq!(Dialog::next_page(Page::Options, Choice::OpenHotkeys), Some(Page::Hotkeys));
        assert_eq!(Dialog::next_page(Page::Hotkeys, Choice::HotkeysOn), Some(Page::Hotkeys));
        assert_eq!(Dialog::next_page(Page::Hotkeys, Choice::OpenHotkey(3)), Some(Page::Hotkey(3)));
        assert_eq!(Dialog::next_page(Page::Hotkey(3), Choice::Capture(3)), None);
        assert_eq!(Dialog::next_page(Page::Hotkey(3), Choice::Unbind(3)), Some(Page::Hotkeys));
        assert_eq!(Dialog::next_page(Page::Hotkey(3), Choice::Back), Some(Page::Hotkeys));
        assert_eq!(Dialog::next_page(Page::Hotkeys, Choice::Back), Some(Page::Options));
        assert_eq!(Dialog::next_page(Page::Pick, Choice::OpenAreas), Some(Page::Areas));
    }
}
