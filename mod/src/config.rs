//! NpcSpawner.ini: what to spawn and how. Read again on every spawn, so it
//! can be edited while the game runs. Pure code (unit-tested).

use crate::hotkeys::{self, Hotkey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// `Distance` metres in front of the player.
    Front,
    /// On the player.
    Player,
    /// On the locked-on target (front when nothing is locked on).
    Target,
    /// The point saved with Ctrl+F5 (front when none is saved).
    Saved,
    /// Random points within `Distance` metres of the player.
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formation {
    /// Side by side, 1.5 m apart, across the player's view.
    Line,
    /// On a circle around the placement point.
    Circle,
    /// Rows of up to 4.
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiMode {
    /// The character's AI runs.
    Active,
    /// Frozen entirely: animation, physics and AI (debug flag "no updates").
    Paused,
    /// Alive and animated, but neither moves nor attacks.
    Still,
}

impl Team {
    /// Hostile -> Ally -> Neutral -> Hostile (a raw team goes back to Hostile).
    pub fn next(self) -> Self {
        match self {
            Team::AsSpawned => Team::Ally,
            Team::Ally => Team::Neutral,
            _ => Team::AsSpawned,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::AsSpawned => "hostile",
            Team::Ally => "ally",
            Team::Neutral => "neutral",
            Team::Raw(_) => "custom team",
        }
    }
}

impl AiMode {
    pub fn next(self) -> Self {
        match self {
            AiMode::Active => AiMode::Paused,
            AiMode::Paused => AiMode::Still,
            AiMode::Still => AiMode::Active,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    /// The team the character spawns with (its NpcParam teamType).
    AsSpawned,
    /// The spirit-ash team (47): fights for the player.
    Ally,
    /// `NeutralTeam` (default 28, friendly NPCs such as Kale): does not fight
    /// the player. Team 0 (Roundtable Hold NPCs) cannot even be hit.
    Neutral,
    /// A raw teamType value, for experiments.
    Raw(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// 0 = a copy of the nearest living character within 30 m.
    pub npc_param_id: i32,
    /// 0 = (NpcParamId / 10000) % 10000.
    pub chr_id: i32,
    /// 0 = the copied character's, or the NpcParam id.
    pub think_id: i32,
    pub chara_init_id: i32,
    pub count: u32,
    /// Most spawned characters alive at once (the debug ChrSet holds 76; many
    /// of them make the game deactivate map characters).
    pub max_spawned: u32,
    pub distance: f32,
    pub placement: Placement,
    pub formation: Formation,
    pub face_player: bool,
    pub ai: AiMode,
    pub team: Team,
    /// Health at spawn, % of the maximum (1-100).
    pub hp_percent: u32,
    /// Overrides the maximum health (0 = the NpcParam's).
    pub max_hp: i32,
    /// Health never drops below 1.
    pub infinite_hp: bool,
    /// Changing the side of spawned characters spawns them again in place,
    /// so their AI forgets the player as a target (otherwise allies keep
    /// finishing their attacks on the player, harmlessly).
    pub side_change_respawn: bool,
    /// Team used by Side = Neutral.
    pub neutral_team: u8,
    pub invulnerable: bool,
    /// Removed characters are destroyed a second later, freeing their slot
    /// (0 = only hidden; the slot comes back at the next loading screen).
    pub destroy_removed: bool,
    /// Experimental: raise the game's near / far character update budgets
    /// (WorldChrManDbg OmissionUpdateNum) to at least these values (0 = game's).
    pub update_near: i32,
    /// Menu as its own System tab (ERNativeUI custom tabs). Off: an entry in
    /// Mod Settings. Custom tabs failed to install with the NativeTweaks
    /// fork r156 on 2.7.2.0 and took every provider's menu down with them.
    pub own_tab: bool,
    /// The name search and the dialog's lists show every NpcParam row
    /// (false: one row per character, without unnamed and non-combat rows).
    pub search_all: bool,
    /// Allies (Side = Ally, AI active) left beyond this many metres for 4 s
    /// are moved behind the player (0 = off).
    pub follow_distance: f32,
    /// Research tools: Ctrl+Shift+F7 memory dumps, and the catalogue written
    /// next to the DLL (NpcSpawner-catalog.tsv, NpcSpawner-npcnames.tsv).
    pub research: bool,
    /// Raise the game's active-character limit (normal and alternate) to at
    /// least this value (0 = the game's own; WW 2.7.2.0 activates 58).
    pub max_active: i32,
    pub update_far: i32,
    /// Hotkeys on (false: none of them does anything).
    pub hotkeys_on: bool,
    /// One per hotkeys::ACTIONS (Hotkey::NONE = that action has none).
    pub hotkeys: [Hotkey; hotkeys::COUNT],
}

impl Default for Config {
    fn default() -> Self {
        Config {
            npc_param_id: 0,
            chr_id: 0,
            think_id: 0,
            chara_init_id: -1,
            count: 1,
            max_spawned: 30,
            distance: 5.0,
            placement: Placement::Front,
            formation: Formation::Line,
            face_player: true,
            ai: AiMode::Active,
            team: Team::AsSpawned,
            hp_percent: 100,
            max_hp: 0,
            infinite_hp: false,
            side_change_respawn: true,
            neutral_team: 28,
            invulnerable: false,
            destroy_removed: true,
            update_near: 0,
            max_active: 0,
            own_tab: false,
            search_all: false,
            follow_distance: 20.0,
            research: false,
            update_far: 0,
            hotkeys_on: true,
            hotkeys: hotkeys::DEFAULTS,
        }
    }
}

pub const MAX_COUNT: u32 = 20;

fn bool_of(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Config {
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut c = Config::default();
        let mut warnings = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split(';').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                warnings.push(format!("line {}: no '='", n + 1));
                continue;
            };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            let lv = v.to_ascii_lowercase();
            let ok = match k.as_str() {
                "npcparamid" => v.parse().map(|x| c.npc_param_id = x).is_ok(),
                "chrid" => v.parse().map(|x| c.chr_id = x).is_ok(),
                "thinkid" => v.parse().map(|x| c.think_id = x).is_ok(),
                "charainitid" => v.parse().map(|x| c.chara_init_id = x).is_ok(),
                "count" => v.parse::<u32>().map(|x| c.count = x.clamp(1, MAX_COUNT)).is_ok(),
                "maxspawned" => v.parse::<u32>().map(|x| c.max_spawned = x.clamp(1, 76)).is_ok(),
                "distance" => v.parse::<f32>().map(|x| c.distance = x.clamp(0.0, 50.0)).is_ok(),
                "placement" => {
                    let p = match lv.as_str() {
                        "front" => Some(Placement::Front),
                        "player" => Some(Placement::Player),
                        "target" => Some(Placement::Target),
                        "saved" => Some(Placement::Saved),
                        "random" => Some(Placement::Random),
                        _ => None,
                    };
                    p.map(|p| c.placement = p).is_some()
                }
                "formation" => {
                    let f = match lv.as_str() {
                        "line" => Some(Formation::Line),
                        "circle" => Some(Formation::Circle),
                        "grid" => Some(Formation::Grid),
                        _ => None,
                    };
                    f.map(|f| c.formation = f).is_some()
                }
                "faceplayer" => bool_of(v).map(|b| c.face_player = b).is_some(),
                "ai" => {
                    let a = match lv.as_str() {
                        "active" => Some(AiMode::Active),
                        "paused" => Some(AiMode::Paused),
                        "still" => Some(AiMode::Still),
                        _ => None,
                    };
                    a.map(|a| c.ai = a).is_some()
                }
                "team" => {
                    let t = match lv.as_str() {
                        "hostile" | "asspawned" => Some(Team::AsSpawned),
                        "ally" => Some(Team::Ally),
                        "neutral" => Some(Team::Neutral),
                        _ => v.parse::<u8>().ok().map(Team::Raw),
                    };
                    t.map(|t| c.team = t).is_some()
                }
                "hppercent" => v.parse::<u32>().map(|x| c.hp_percent = x.clamp(1, 100)).is_ok(),
                "maxhp" => v.parse::<i32>().map(|x| c.max_hp = x.max(0)).is_ok(),
                "infinitehp" => bool_of(v).map(|b| c.infinite_hp = b).is_some(),
                "neutralteam" => v.parse::<u8>().map(|x| c.neutral_team = x).is_ok(),
                "sidechangerespawn" => bool_of(v).map(|b| c.side_change_respawn = b).is_some(),
                "invulnerable" => bool_of(v).map(|b| c.invulnerable = b).is_some(),
                "searchall" => bool_of(v).map(|b| c.search_all = b).is_some(),
                "followdistance" => v.parse::<f32>().map(|x| c.follow_distance = x.clamp(0.0, 100.0)).is_ok(),
                "research" => bool_of(v).map(|b| c.research = b).is_some(),
                // Keys of the removed arena (0.6.x INIs): ignored.
                "arenablock" | "arenax" | "arenay" | "arenaz" | "arenayaw" | "keepernpcparamid"
                | "keeperchrid" | "keeperthinkid" | "keeperactionbutton" => true,
                "owntab" => bool_of(v).map(|b| c.own_tab = b).is_some(),
                "maxactive" => v.parse::<i32>().map(|x| c.max_active = x.clamp(0, 500)).is_ok(),
                "updatenear" => v.parse::<i32>().map(|x| c.update_near = x.clamp(0, 1000)).is_ok(),
                "updatefar" => v.parse::<i32>().map(|x| c.update_far = x.clamp(0, 1000)).is_ok(),
                "destroyremoved" => bool_of(v).map(|b| c.destroy_removed = b).is_some(),
                "hotkeys" => bool_of(v).map(|b| c.hotkeys_on = b).is_some(),
                _ if hotkeys::ACTIONS.iter().any(|a| a.0.eq_ignore_ascii_case(&k)) => {
                    let i = hotkeys::ACTIONS.iter().position(|a| a.0.eq_ignore_ascii_case(&k)).unwrap_or(0);
                    Hotkey::parse(v).map(|h| c.hotkeys[i] = h).is_some()
                }
                _ => {
                    warnings.push(format!("line {}: unknown key '{k}'", n + 1));
                    true
                }
            };
            if !ok {
                warnings.push(format!("line {}: bad value '{v}' for '{k}'", n + 1));
            }
        }
        for (a, b) in hotkeys::clashes(&c.hotkeys) {
            warnings.push(format!(
                "[Hotkeys]: {} and {} have the same keys ({}); both run",
                hotkeys::ACTIONS[a].0,
                hotkeys::ACTIONS[b].0,
                c.hotkeys[a].name()
            ));
        }
        (c, warnings)
    }

    /// `text` (an INI) with its [Hotkeys] section, and any hotkey key outside
    /// it, replaced by these hotkeys; the rest is kept as it is.
    pub fn with_hotkeys(&self, text: &str) -> String {
        let crlf = text.contains("\r\n") || text.is_empty();
        let mut out: Vec<&str> = Vec::new();
        let mut in_section = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_section = t.eq_ignore_ascii_case("[hotkeys]");
            }
            let key = t.split(['=', ';']).next().unwrap_or("").trim();
            let hotkey_key = t.contains('=')
                && (key.eq_ignore_ascii_case("hotkeys") || hotkeys::ACTIONS.iter().any(|a| a.0.eq_ignore_ascii_case(key)));
            if !in_section && !hotkey_key {
                out.push(line);
            }
        }
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
        let mut s = out.join("\n");
        if !s.is_empty() {
            s.push_str("\n\n");
        }
        s.push_str(&self.hotkeys_ini());
        if crlf { s.replace('\n', "\r\n") } else { s }
    }

    /// The [Hotkeys] section of the INI.
    fn hotkeys_ini(&self) -> String {
        let mut s = String::from(
            "[Hotkeys]\n\
             ; Changes apply while the game runs (the INI is checked every second).\n\
             ; A key with Ctrl, Shift and / or Alt: Ctrl+F6, Shift+K, Alt+Num5, F9, Mouse4.\n\
             ; Keys: A-Z, 0-9, F1-F24, Num0-Num9, NumAdd, NumSubtract, NumMultiply, NumDivide,\n\
             ; NumDecimal, Insert, Delete, Home, End, PageUp, PageDown, Up, Down, Left, Right,\n\
             ; Space, Enter, Tab, Backspace, Pause, Mouse3 (middle), Mouse4, Mouse5.\n\
             ; None = no hotkey. The modifiers must match exactly. Avoid Alt+F4.\n\
             ; The talk menu can also assign them: Spawn options > Hotkeys.\n\
             ; 0 = every hotkey off.\n",
        );
        s.push_str(&format!("Hotkeys = {}\n", if self.hotkeys_on { 1 } else { 0 }));
        for (i, (key, label)) in hotkeys::ACTIONS.iter().enumerate() {
            s.push_str(&format!("; {label}\n{key} = {}\n", self.hotkeys[i].name()));
        }
        s
    }

    /// The INI text for these settings (Save these settings in the tab).
    pub fn to_ini(&self) -> String {
        let placement = match self.placement {
            Placement::Front => "Front",
            Placement::Player => "Player",
            Placement::Target => "Target",
            Placement::Saved => "Saved",
            Placement::Random => "Random",
        };
        let formation = match self.formation {
            Formation::Line => "Line",
            Formation::Circle => "Circle",
            Formation::Grid => "Grid",
        };
        let ai = match self.ai {
            AiMode::Active => "Active",
            AiMode::Paused => "Paused",
            AiMode::Still => "Still",
        };
        let team = match self.team {
            Team::AsSpawned => "Hostile".to_string(),
            Team::Ally => "Ally".to_string(),
            Team::Neutral => "Neutral".to_string(),
            Team::Raw(v) => v.to_string(),
        };
        let b = |v: bool| if v { 1 } else { 0 };
        format!(
            "; NPC Spawner - read again on every spawn (Ctrl+F6), no restart needed.\n\
             ; Written by \"Save these settings\" in the NPC Spawner tab.\n\
             [Spawn]\n\
             ; NpcParam id. 0 = a copy of the nearest living enemy within 30 m.\n\
             NpcParamId = {}\n\
             ; Model (cXXXX). 0 = (NpcParamId / 10000) % 10000\n\
             ChrId = {}\n\
             ; NpcThinkParam (AI). 0 = automatic: the copied character's, or the one the game's maps give it\n\
             ThinkId = {}\n\
             CharaInitId = {}\n\
             ; 1-20\n\
             Count = {}\n\
             ; most spawned alive at once (1-76)\n\
             MaxSpawned = {}\n\
             ; metres (also the radius for Random)\n\
             Distance = {}\n\
             ; Front, Player, Target (lock-on), Saved (Ctrl+F5), Random\n\
             Placement = {placement}\n\
             ; Line, Circle, Grid\n\
             Formation = {formation}\n\
             FacePlayer = {}\n\
             \n\
             [Behaviour]\n\
             ; Active, Paused, Still\n\
             AI = {ai}\n\
             ; Hostile (as spawned), Ally (team 47), Neutral (NeutralTeam, does not fight you) or a raw teamType number\n\
             Team = {team}\n\
             ; health at spawn, % of max\n\
             HpPercent = {}\n\
             ; 0 = the NpcParam's maximum\n\
             MaxHp = {}\n\
             ; refilled when it drops to 10 %\n\
             InfiniteHp = {}\n\
             Invulnerable = {}\n\
             ; changing the side spawns them again in place, so they forget you as a target\n\
             SideChangeRespawn = {}\n\
             ; team of Side = Neutral: 28 friendly NPCs (can be hit), 0 Roundtable NPCs (cannot)\n\
             NeutralTeam = {}\n\
             ; allies left farther than this (metres) for 4 s are moved behind you (0 = off)\n\
             FollowDistance = {}\n\
             \n\
             [Remove]\n\
             ; removed characters are destroyed 1 s later, freeing their slot (0 = only hidden)\n\
             DestroyRemoved = {}\n\
             \n\
             [Limits]\n\
             ; Experimental. Raise the game's active-character limit to at least this\n\
             ; (0 = the game's own, about 60). Higher = more enemies at once, more load.\n\
             MaxActive = {}\n\
             ; Per-frame character update budgets near / far (0 = the game's, 10 / 15).\n\
             UpdateNear = {}\n\
             UpdateFar = {}\n\
             \n\
             [Menu]\n\
             ; 0 = an entry in Mod Settings (recommended). 1 = own tab in the System\n\
             ; screen: experimental, it can take the whole menu down; restart needed.\n\
             OwnTab = {}\n\
             ; 1 = the name search and the dialog's lists show every NpcParam row;\n\
             ; 0 = one row per character, without unnamed and non-combat rows.\n\
             SearchAll = {}\n\
             \n\
             [Research]\n\
             ; 1 = Ctrl+Shift+F7 writes memory dumps to dumps\\, and the full catalogue of\n\
             ; NpcParam rows is written next to the DLL (NpcSpawner-catalog.tsv).\n\
             Research = {}\n\
             \n\
             {}",
            self.npc_param_id,
            self.chr_id,
            self.think_id,
            self.chara_init_id,
            self.count,
            self.max_spawned,
            self.distance,
            b(self.face_player),
            self.hp_percent,
            self.max_hp,
            b(self.infinite_hp),
            b(self.invulnerable),
            b(self.side_change_respawn),
            self.neutral_team,
            self.follow_distance,
            b(self.destroy_removed),
            self.max_active,
            self.update_near,
            self.update_far,
            b(self.own_tab),
            b(self.search_all),
            b(self.research),
            self.hotkeys_ini(),
        )
    }

    /// Model id the debug creator loads (cXXXX).
    pub fn resolve_chr_id(npc_param_id: i32, chr_id: i32) -> i32 {
        if chr_id > 0 { chr_id } else { (npc_param_id / 10000) % 10000 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let (c, w) = Config::parse("");
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn parses_all_keys_and_comments() {
        let text = "[Spawn]\nNpcParamId = 21000000 ; Margit\nCount=50\nDistance = 3.5\nPlacement = Circle\n\
                    Formation = circle\nAI = still\nTeam = Ally\nHpPercent = 0\nMaxHp = 1000\nInfiniteHp = yes\n\
                    Invulnerable = 1\nFacePlayer = off\nDestroyRemoved = 0\n";
        let (c, w) = Config::parse(text);
        assert_eq!(c.npc_param_id, 21000000);
        assert_eq!(c.count, MAX_COUNT);
        assert_eq!(c.distance, 3.5);
        assert_eq!(c.placement, Placement::Front, "Circle is not a placement");
        assert_eq!(c.formation, Formation::Circle);
        assert_eq!(c.ai, AiMode::Still);
        assert_eq!(c.team, Team::Ally);
        assert_eq!(c.hp_percent, 1);
        assert_eq!(c.max_hp, 1000);
        assert!(c.infinite_hp && c.invulnerable && !c.face_player && !c.destroy_removed);
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn raw_team_and_bad_values() {
        let (c, w) = Config::parse("Team = 6\nCount = many\nFoo = 1\nMaxSpawned = 500\n");
        assert_eq!(c.max_spawned, 76);
        assert_eq!(c.team, Team::Raw(6));
        assert_eq!(c.count, 1);
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn chr_id_from_npc_param() {
        assert_eq!(Config::resolve_chr_id(46900008, 0), 4690);
        assert_eq!(Config::resolve_chr_id(121000000, 0), 2100);
        assert_eq!(Config::resolve_chr_id(21000000, 2180), 2180);
    }

    #[test]
    fn ini_round_trip() {
        let mut c = Config::default();
        c.npc_param_id = 21000000;
        c.count = 7;
        c.placement = Placement::Random;
        c.formation = Formation::Grid;
        c.ai = AiMode::Still;
        c.team = Team::Raw(6);
        c.hp_percent = 40;
        c.invulnerable = true;
        c.destroy_removed = false;
        c.max_active = 120;
        c.chara_init_id = 3000;
        c.own_tab = true;
        c.side_change_respawn = false;
        c.neutral_team = 0;
        c.search_all = true;
        c.follow_distance = 35.0;
        c.research = true;
        let (back, w) = Config::parse(&c.to_ini());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back, c);
        let (d, w) = Config::parse(&Config::default().to_ini());
        assert!(w.is_empty());
        assert_eq!(d, Config::default());
    }

    #[test]
    fn team_cycle_and_neutral() {
        assert_eq!(Team::AsSpawned.next().next().next(), Team::AsSpawned);
        let (c, w) = Config::parse("Team = Neutral\n");
        assert!(w.is_empty());
        assert_eq!(c.team, Team::Neutral);
    }

    /// The INI shipped with the mod is the defaults; NPCTA_WRITE_INI=1 writes it.
    #[test]
    fn hotkeys_replace_only_their_section() {
        let mut c = Config::default();
        c.hotkeys[hotkeys::SPAWN] = Hotkey::parse("F6").unwrap();
        let old = "[Spawn]\r\nNpcParamId = 5\r\n\r\n[Hotkeys]\r\nSpawn = Ctrl+F6\r\n\r\n[Menu]\r\nSearchAll = 1\r\n";
        let new = c.with_hotkeys(old);
        assert!(new.contains("\r\n"));
        let (p, w) = Config::parse(&new);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(p.npc_param_id, 5);
        assert!(p.search_all);
        assert_eq!(p.hotkeys, c.hotkeys);
        assert_eq!(new.matches("[Hotkeys]").count(), 1);
        // Twice gives the same text.
        assert_eq!(c.with_hotkeys(&new), new);
        // An INI without the section gets it.
        let (q, _) = Config::parse(&c.with_hotkeys("[Spawn]\nCount = 3\n"));
        assert_eq!((q.count, q.hotkeys), (3, c.hotkeys));
    }

    #[test]
    fn shipped_ini_is_the_defaults() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/NpcSpawner.ini");
        let ini = Config::default().to_ini().replace('\n', "\r\n");
        if std::env::var("NPCTA_WRITE_INI").is_ok() {
            std::fs::write(path, &ini).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap_or_default(), ini);
    }

    #[test]
    fn old_arena_keys_are_ignored() {
        let (c, w) = Config::parse("[Arena]
ArenaBlock = 123
KeeperNpcParamId = 5
KeeperActionButton = 1000
");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn ai_cycle() {
        assert_eq!(AiMode::Active.next().next().next(), AiMode::Active);
    }
}
