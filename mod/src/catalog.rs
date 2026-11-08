//! A catalogue of every NpcParam row, built from the running game: id, model,
//! name, kind, team, HP, runes. Written to NpcSpawner-catalog.tsv and kept
//! for the name search. Names, best first:
//!  1. the row's own NpcName (game's texts, in the game's language);
//!  2. the community row name from Smithbox (English, MIT, data/npcparam_names.tsv,
//!     embedded; see THIRD-PARTY-NOTICES.txt);
//!  3. the boss-bar name of the row's model (90CCCC00V in NpcName).
//!
//! Zones: where each row is placed in the game's maps, and the AI (ThinkParam)
//! the maps give it (data/npc_zones.tsv, made by tools/zones.py from every
//! MSB of the game, read with Smithbox's libraries; zone names from Smithbox's
//! MapNames.json, MIT).
//!
//! Names: NpcParam.nameId is looked up in the NpcName message files. The
//! lookup at WW 2.7.2.0 0x140D12AD0 tries categories 0x6D, 0x1B, 0x13E, 0x1A2
//! in turn; 18 / 318 / 418 (base game and the two DLC banks) are the usual
//! NpcName ids, so all of them are tried and the log tells which one answers.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use eldenring::cs::{NpcParam, NpcThinkParam, SoloParamRepository};
use fromsoftware_shared::FromStatic;

use crate::{log, texts};

/// Categories tried for NPC names, in order.
/// The game's NPC-name lookup (WW 2.7.2.0 0x140D13910) tries these in this order
/// (patch overrides, base game, then the DLC files).
pub const NAME_CATEGORIES: [u32; 4] = [0x77, 0x12, 0x148, 0x1AC];

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u32,
    pub chr: u32,
    pub name: String,
    /// "game" (the row's own nameId), "smithbox" or "model".
    pub name_source: &'static str,
    /// Community (English) name, also searched.
    pub english: String,
    /// NpcThinkParam (AI) spawned with this row: the one the game's maps give
    /// it, else the row with the same id, else the nearest base row of the
    /// same character (id rounded down to 10 / 100 / 1000 / 10000).
    /// None = no AI row found.
    pub think: Option<u32>,
    /// Zones of the game's maps where this row is placed.
    pub zones: &'static [u16],
    /// Named, with health, able to fight, not a dummy or corpse.
    pub fighter: bool,
    /// Battle goal of that AI: 0 / -1 = no combat (story or idle NPC).
    pub battle_goal: Option<i32>,
    /// Shown by the filtered search: one row per name and model, named,
    /// with health, able to fight, not a dummy or corpse.
    pub useful: bool,
    pub title: String,
    pub kind: &'static str,
    pub team: u8,
    pub npc_type: u8,
    pub hp: u32,
    pub runes: u32,
    pub threat: u32,
}

static CATALOG: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// Smithbox's NpcParam row names (id TAB name, sorted by id).
const COMMUNITY_NAMES: &str = include_str!("../data/npcparam_names.tsv");
/// Zones (Z lines) and placements (id, zones, map AI) from the game's maps.
const ZONE_DATA: &str = include_str!("../data/npc_zones.tsv");

/// A map zone: its area (group of zones) and name.
pub struct Zone {
    pub area: &'static str,
    pub name: &'static str,
}

struct Zones {
    zones: Vec<Zone>,
    /// Area names in order.
    areas: Vec<&'static str>,
    /// NpcParam id -> (zones, map AI), sorted by id.
    placed: Vec<(u32, Vec<u16>, u32)>,
}

fn zone_data() -> &'static Zones {
    static Z: std::sync::OnceLock<Zones> = std::sync::OnceLock::new();
    Z.get_or_init(|| {
        let mut z = Zones { zones: Vec::new(), areas: Vec::new(), placed: Vec::new() };
        for l in ZONE_DATA.lines() {
            let f: Vec<&'static str> = l.split('\t').collect();
            if f[0] == "Z" && f.len() >= 4 {
                if !z.areas.contains(&f[2]) {
                    z.areas.push(f[2]);
                }
                z.zones.push(Zone { area: f[2], name: f[3] });
            } else if f.len() >= 3 {
                let (Ok(id), Ok(think)) = (f[0].parse(), f[2].parse()) else { continue };
                let zs = f[1].split(',').filter_map(|x| x.parse().ok()).collect();
                z.placed.push((id, zs, think));
            }
        }
        z.placed.sort_by_key(|p| p.0);
        z
    })
}

pub fn zones() -> &'static [Zone] {
    &zone_data().zones
}

pub fn areas() -> &'static [&'static str] {
    &zone_data().areas
}

/// Zone indexes of an area.
pub fn zones_of_area(area: usize) -> Vec<u16> {
    let d = zone_data();
    let Some(a) = d.areas.get(area) else { return Vec::new() };
    (0..d.zones.len() as u16).filter(|i| d.zones[*i as usize].area == *a).collect()
}

fn placement(id: u32) -> Option<&'static (u32, Vec<u16>, u32)> {
    let d = zone_data();
    d.placed.binary_search_by_key(&id, |p| p.0).ok().map(|i| &d.placed[i])
}

fn community_names() -> Vec<(u32, &'static str)> {
    COMMUNITY_NAMES
        .lines()
        .filter_map(|l| {
            let (id, name) = l.split_once('\t')?;
            Some((id.parse().ok()?, name))
        })
        .collect()
}

/// What a team type means, for the catalogue's Kind column. Only 6 (enemy)
/// and 47 (spirit ash) are confirmed in game; the rest stay "team N".
pub fn kind_of(team: u8, phantom: bool) -> &'static str {
    match team {
        6 => "enemy",
        47 => "spirit ash",
        _ if phantom => "phantom",
        0 => "neutral NPC",
        _ => "other",
    }
}

fn name_of(name_id: i32, hits: &mut [u32; NAME_CATEGORIES.len()]) -> String {
    if name_id <= 0 {
        return String::new();
    }
    for (i, c) in NAME_CATEGORIES.iter().enumerate() {
        if let Some(n) = texts::lookup(*c, name_id) {
            hits[i] += 1;
            return n.replace(['\t', '\n', '\r'], " ");
        }
    }
    String::new()
}

/// Builds the catalogue (a few milliseconds for ~7000 rows) and writes it.
/// `write`: also write the catalogue next to the DLL (Research = 1).
pub fn build(dir: &std::path::Path, write: bool) -> usize {
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else {
        log("catalog: params not loaded yet");
        return 0;
    };
    let mut hits = [0u32; NAME_CATEGORIES.len()];
    // Boss-bar names are NpcName ids 90CCCC00V (CCCC = model, V = variant),
    // e.g. 902130000 Margit, 902130002 Morgott (both c2130). All banks.
    BUILT_WITH_TEXTS.store(texts::ready(), Ordering::Release);
    let mut all_names: Vec<(i32, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut per_bank = Vec::new();
    for b in 0..texts::bank_count().min(16) {
        for c in NAME_CATEGORIES {
            let n = texts::all_of(b, c);
            per_bank.push((b, c, n.len()));
            for (id, t) in n {
                if seen.insert(id) {
                    all_names.push((id, t));
                }
            }
        }
    }
    let mut by_model: std::collections::BTreeMap<u32, Vec<String>> = Default::default();
    for (id, t) in &all_names {
        if (900_000_000..1_000_000_000).contains(id) {
            let chr = ((*id / 1000) % 10_000) as u32;
            let names = by_model.entry(chr).or_default();
            if !names.contains(t) && names.len() < 3 {
                names.push(t.clone());
            }
        }
    }
    let community = community_names();
    let mut out = Vec::new();
    for (id, row) in repo.rows::<NpcParam>() {
        let own = name_of(row.name_id(), &mut hits);
        let chr = (id / 10000) % 10000;
        let english = community
            .binary_search_by_key(&id, |(i, _)| *i)
            .ok()
            .map(|i| community[i].1.to_string())
            .unwrap_or_default();
        let (name, name_source) = if !own.is_empty() {
            (own, "game")
        } else if !english.is_empty() {
            (english.clone(), "smithbox")
        } else if let Some(n) = by_model.get(&chr) {
            (n.join(" / "), "model")
        } else {
            (String::new(), "")
        };
        let title = name_of(row.role_name_id(), &mut [0; NAME_CATEGORIES.len()]);
        let placed = placement(id);
        let think = placed
            .map(|p| p.2)
            .filter(|t| *t > 1 && repo.get::<NpcThinkParam>(*t).is_some())
            .or_else(|| {
                [1, 10, 100, 1000, 10_000]
                    .iter()
                    .map(|m| id - id % m)
                    .find(|t| repo.get::<NpcThinkParam>(*t).is_some())
            });
        let battle_goal = think.and_then(|t| repo.get::<NpcThinkParam>(t)).map(|t| t.battle_goal_id());
        let team = row.team_type();
        out.push(Entry {
            id,
            chr,
            name,
            name_source,
            english,
            think,
            zones: placed.map_or(&[][..], |p| &p.1[..]),
            fighter: false,
            battle_goal,
            useful: false,
            title,
            kind: kind_of(team, row.phantom_shader_id() > 0),
            team,
            npc_type: row.npc_type(),
            hp: row.hp(),
            runes: row.get_soul(),
            threat: row.threat_lv(),
        });
    }
    mark_useful(&mut out);
    let named = out.iter().filter(|e| e.name_source == "game").count();
    let by_smithbox = out.iter().filter(|e| e.name_source == "smithbox").count();
    let by_model_n = out.iter().filter(|e| e.name_source == "model").count();
    log(format!(
        "catalog: {} NpcParam rows ({} useful, {} with a base AI): {named} game names, {by_smithbox} Smithbox names, {by_model_n} model names ({} models); \
         names per category {:?} = {:?}; NpcName texts (bank, category, count) {:?}",
        out.len(),
        out.iter().filter(|e| e.useful).count(),
        out.iter().filter(|e| e.think.is_some_and(|t| t != e.id)).count(),
        by_model.len(),
        NAME_CATEGORIES,
        hits,
        per_bank
    ));
    let mut tsv = String::from(
        "NpcParamId\tModel\tName\tNameFrom\tEnglishName\tTitle\tRole\tUseful\tThinkId\tZones\tBattleGoal\tKind\tTeamType\tNpcType\tHP\tRunes\tThreat\n",
    );
    for e in &out {
        tsv.push_str(&format!(
            "{}\tc{:04}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            e.id,
            e.chr,
            e.name,
            e.name_source,
            e.english,
            e.title,
            role_of(e).0,
            e.useful as u8,
            e.think.map_or("-".to_string(), |t| t.to_string()),
            e.zones.iter().filter_map(|z| zones().get(*z as usize)).map(|z| z.name).collect::<Vec<_>>().join(", "),
            e.battle_goal.map_or("-".to_string(), |g| g.to_string()),
            e.kind,
            e.team,
            e.npc_type,
            e.hp,
            e.runes,
            e.threat
        ));
    }
    let names = &all_names;
    let mut ntsv = String::from("NameId\tText\n");
    for (id, t) in names {
        ntsv.push_str(&format!("{id}\t{}\n", t.replace(['\t', '\n', '\r'], " ")));
    }
    if write {
        let _ = std::fs::write(dir.join("NpcSpawner-npcnames.tsv"), ntsv);
        log(format!("catalog: {} NPC name texts (all name files) written to NpcSpawner-npcnames.tsv", names.len()));
        match std::fs::write(dir.join("NpcSpawner-catalog.tsv"), tsv) {
            Ok(()) => log("catalog: written to NpcSpawner-catalog.tsv"),
            Err(e) => log(format!("catalog: could not write the file: {e}")),
        }
    }
    let n = out.len();
    *CATALOG.lock().unwrap_or_else(|e| e.into_inner()) = out;
    n
}

/// Words of rows that are not characters to fight: training dummies,
/// corpses, dead or "base" parts of a boss (lower case, English names).
const NOT_USEFUL: [&str; 6] = ["dummy", "corpse", " - dead", "(dead)", ": base ", "(base)"];

fn is_fighter(e: &Entry) -> bool {
    let english = e.english.to_ascii_lowercase();
    matches!(e.name_source, "game" | "smithbox")
        && e.hp > 1
        && !e.battle_goal.is_some_and(|g| g <= 0)
        && !NOT_USEFUL.iter().any(|w| english.contains(w))
}

/// Which of two rows of one character to show: placed in the maps first,
/// then with an AI, then the lowest id (the base row).
fn rank(e: &Entry) -> (bool, bool, u32) {
    (e.zones.is_empty(), e.think.is_none(), e.id)
}

/// Keeps one row per name and model (the best by rank).
fn one_per_character<'a>(rows: impl Iterator<Item = &'a Entry>) -> Vec<&'a Entry> {
    let mut best: std::collections::HashMap<(String, u32), &Entry> = Default::default();
    for e in rows {
        let key = (fold(&e.name), e.chr);
        match best.get(&key) {
            Some(b) if rank(b) <= rank(e) => {}
            _ => {
                best.insert(key, e);
            }
        }
    }
    best.into_values().collect()
}

/// Marks fighters, and the rows the filtered search shows without a zone:
/// one fighter per name and model.
fn mark_useful(rows: &mut [Entry]) {
    for e in rows.iter_mut() {
        e.fighter = is_fighter(e);
    }
    let ids: std::collections::HashSet<u32> =
        one_per_character(rows.iter().filter(|e| e.fighter)).into_iter().map(|e| e.id).collect();
    for e in rows.iter_mut() {
        e.useful = ids.contains(&e.id);
    }
}

/// The catalogue was built with the game's message lookup at hand.
static BUILT_WITH_TEXTS: AtomicBool = AtomicBool::new(false);
static LAST_BUILD: Mutex<Option<Instant>> = Mutex::new(None);

/// True when the catalogue should be built (again): not built yet, or built
/// before the game's texts could be read. At most every 5 s (a build takes
/// tens of milliseconds), and once for the texts.
pub fn needs_build(texts_ready: bool) -> bool {
    if is_built() && (!texts_ready || BUILT_WITH_TEXTS.load(Ordering::Acquire)) {
        return false;
    }
    let mut last = LAST_BUILD.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

pub fn is_built() -> bool {
    !CATALOG.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// AI a row spawns with by default (catalogue built): the maps' one, or its
/// own, or its base row's.
pub fn think_of(id: u32) -> Option<u32> {
    let c = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    let i = c.binary_search_by_key(&id, |e| e.id).ok()?;
    c[i].think
}

/// Name of a row from the catalogue (None when unnamed or not built).
pub fn name(id: i32) -> Option<String> {
    let c = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    let i = c.binary_search_by_key(&(id as u32), |e| e.id).ok()?;
    let e = &c[i];
    (!e.name.is_empty()).then(|| e.name.clone())
}

/// Short role tag of a row for search results, and its sort rank (rows that
/// fight the player first). Team types seen in this game: 6 enemy, 7 strong
/// enemy, 26 hostile NPC, 27 invader, 28 friendly NPC / ally summon,
/// 0 / 2 neutral or friendly, 47 spirit ash.
pub fn role_of(e: &Entry) -> (&'static str, u8) {
    let n = e.english.to_ascii_lowercase();
    if e.battle_goal.is_some_and(|g| g <= 0) {
        ("no combat", 6)
    } else if n.contains("spirit summon") || e.team == 47 {
        ("spirit", 5)
    } else if n.contains("(boss)") || e.team == 7 {
        ("boss", 0)
    } else if e.team == 27 {
        ("invader", 1)
    } else if e.team == 6 || e.team == 26 {
        ("enemy", 1)
    } else if e.team == 28 {
        ("ally", 3)
    } else {
        ("NPC", 4)
    }
}

/// One search hit.
#[derive(Clone, Debug)]
pub struct Hit {
    pub id: u32,
    pub chr: u32,
    /// AI to spawn with (0 = the NpcParam id itself).
    pub think: u32,
    pub label: String,
    /// Short form for the Result row's label: "Name [tag]".
    pub short: String,
}

fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
            'ñ' | 'Ñ' => 'n',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// What the search and the lists show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// Every row (false: one per character, fighters only).
    pub all: bool,
    /// A group of TYPES.
    pub kind: Option<usize>,
    /// An area of areas().
    pub area: Option<usize>,
    /// A zone of zones() (inside the area).
    pub zone: Option<u16>,
}

impl Filter {
    pub fn narrows(&self) -> bool {
        self.kind.is_some() || self.area.is_some() || self.zone.is_some()
    }

    fn keeps(&self, e: &Entry) -> bool {
        if e.name.is_empty() && e.english.is_empty() {
            return false;
        }
        if !self.all && !e.fighter {
            return false;
        }
        if let Some(k) = self.kind {
            if !TYPES.get(k).is_some_and(|t| t.1.contains(&role_of(e).0)) {
                return false;
            }
        }
        if let Some(z) = self.zone {
            return e.zones.contains(&z);
        }
        if let Some(a) = self.area {
            let name = areas().get(a).copied().unwrap_or("");
            return e.zones.iter().any(|z| zones().get(*z as usize).is_some_and(|z| z.area == name));
        }
        true
    }

    fn pick<'a>(&self, rows: impl Iterator<Item = &'a Entry>) -> Vec<&'a Entry> {
        let rows = rows.filter(|e| self.keeps(e));
        if self.all { rows.collect() } else { one_per_character(rows) }
    }
}

/// Rows whose id, name or English name matches `query` (case and accents
/// ignored; every word must appear) and the filter. Exact ids first, then
/// names starting with the query, then the rest. An empty query lists every
/// row of the filter (when it narrows anything), by name. Returns at most
/// `max` hits and how many matched.
pub fn search(query: &str, max: usize, filter: Filter) -> (Vec<Hit>, usize) {
    let q = fold(query.trim());
    if q.is_empty() {
        if !filter.narrows() {
            return (Vec::new(), 0);
        }
        let all = list(filter);
        let n = all.len();
        return (all.into_iter().take(max).collect(), n);
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let c = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    let mut scored: Vec<(u8, &Entry)> = Vec::new();
    for e in filter.pick(c.iter()) {
        let id = e.id.to_string();
        let name = fold(&e.name);
        let english = fold(&e.english);
        let score = if id == q {
            0
        } else if name.starts_with(&q) || english.starts_with(&q) {
            1
        } else if words.iter().all(|w| name.contains(w) || english.contains(w)) {
            2
        } else if id.starts_with(&q) {
            3
        } else {
            continue;
        };
        scored.push((score, e));
    }
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(role_of(a.1).1.cmp(&role_of(b.1).1))
            .then(a.1.name.cmp(&b.1.name))
            .then(a.1.id.cmp(&b.1.id))
    });
    let n = scored.len();
    (scored.into_iter().take(max).map(|(_, e)| hit_of(e)).collect(), n)
}

fn hit_of(e: &Entry) -> Hit {
    let tag = role_of(e).0;
    let label = if e.english.is_empty() || fold(&e.english) == fold(&e.name) {
        format!("[{tag}] {} ({})", e.name, e.id)
    } else {
        format!("[{tag}] {} / {} ({})", e.name, e.english, e.id)
    };
    let short = format!("{} [{tag}]", if e.name.is_empty() { &e.english } else { &e.name });
    let think = e.think.filter(|t| *t != e.id).unwrap_or(0);
    Hit { id: e.id, chr: e.chr, think, label, short }
}

/// Types of character (role tags of role_of), for the filters and lists.
pub const TYPES: [(&str, &[&str]); 5] = [
    ("Bosses", &["boss"]),
    ("Enemies", &["enemy"]),
    ("Invaders and hostile NPCs", &["invader"]),
    ("Spirit ashes and allies", &["spirit", "ally"]),
    ("Other NPCs", &["NPC", "no combat"]),
];

/// Position of a row's type in TYPES (bosses first).
fn type_rank(e: &Entry) -> usize {
    let tag = role_of(e).0;
    TYPES.iter().position(|t| t.1.contains(&tag)).unwrap_or(TYPES.len())
}

/// Every row of the filter, by type (bosses, enemies, invaders, spirits,
/// other NPCs) and then by name.
pub fn list(filter: Filter) -> Vec<Hit> {
    let c = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows = filter.pick(c.iter());
    rows.sort_by_cached_key(|e| (type_rank(e), fold(if e.name.is_empty() { &e.english } else { &e.name }), e.id));
    rows.into_iter().map(hit_of).collect()
}

/// How many rows the filter shows (for the dialog's counts).
pub fn count(filter: Filter) -> usize {
    let c = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    filter.pick(c.iter()).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_table_is_sorted_and_named() {
        let t = community_names();
        assert!(t.len() > 6000);
        assert!(t.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(t.iter().any(|(i, n)| *i == 21300014 && n.contains("Margit")));
    }

    fn entry(id: u32, name: &str, think: Option<u32>, hp: u32) -> Entry {
        Entry {
            id,
            chr: (id / 10000) % 10000,
            name: name.into(),
            name_source: "smithbox",
            english: name.into(),
            think,
            zones: &[],
            fighter: false,
            battle_goal: think.map(|_| 1),
            useful: false,
            title: String::new(),
            kind: "enemy",
            team: 6,
            npc_type: 0,
            hp,
            runes: 0,
            threat: 0,
        }
    }

    #[test]
    fn useful_keeps_one_row_per_character() {
        let mut rows = vec![
            entry(21300033, "Margit", Some(21300000), 900),
            entry(21300000, "Margit", Some(21300000), 900),
            entry(21300014, "Margit, the Fell Omen", Some(21300014), 900),
            entry(1002000, "Patrol Dummy", Some(1), 50),
            entry(81200000, "Merciless Chariot (Base)", Some(1), 0),
            entry(36610000, "Putrid Corpse", Some(1), 300),
        ];
        mark_useful(&mut rows);
        let mut ids: Vec<u32> = rows.iter().filter(|e| e.useful).map(|e| e.id).collect();
        ids.sort();
        assert_eq!(ids, vec![21300000, 21300014]);
    }

    #[test]
    fn zone_data_parses() {
        assert!(zones().len() > 100, "{}", zones().len());
        assert!(areas().contains(&"Catacombs"));
        assert!(zones().iter().any(|z| z.name == "Limgrave" && z.area == "The Lands Between"));
        let p = placement(1200020).expect("placed");
        assert_eq!(p.2, 1200000);
        assert!(!zones_of_area(0).is_empty());
    }

    #[test]
    fn fold_ignores_case_and_accents() {
        assert_eq!(fold("Kalé Ñandú"), "kale nandu");
    }
}
