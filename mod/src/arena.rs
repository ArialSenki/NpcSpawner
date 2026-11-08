//! The spawned characters and what can be done with them.

use std::time::{Duration, Instant};

use eldenring::{
    cs::{CSDebugChrCreator, ChrIns, EnemyIns, FieldInsHandle, WorldChrMan},
    position::HavokPosition,
    rotation::Quaternion,
};
use fromsoftware_shared::Superclass;

use crate::{
    config::{AiMode, Config, Placement, Team},
    formation::{self, Lcg},
    game::{self, SpawnArgs},
    log,
};

/// Spirit-ash team (NpcParam teamType 47): fights for the player. Confirmed
/// in game (NativeTweaks' Melina companion, phase 0 of this mod).
pub const TEAM_PLAYER_SIDE: u8 = 47;
const COPY_RANGE: f32 = 30.0;
const SPAWN_TIMEOUT: Duration = Duration::from_secs(10);
/// Destroying a character unloads its model and resources; 76 in one frame
/// froze the game in 0.1.1 (3 per frame still stuttered), so one per frame.
const DESTROY_PER_FRAME: usize = 1;
/// An ally left beyond FollowDistance this long comes back to the player.
const FOLLOW_AFTER: Duration = Duration::from_secs(4);
/// ChrIns offset of the home point (Havok x, y, z), WW 2.7.2.0.
const HOME_OFFSET: usize = 0x80;


/// What the debug creator is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template {
    pub chr_id: i32,
    pub npc_param_id: i32,
    pub think_id: i32,
    pub chara_init_id: i32,
}

#[derive(Clone, Copy)]
struct Request {
    template: Template,
    cfg: Config,
    pos: [f32; 3],
    yaw: f32,
}

pub struct Spawned {
    pub handle: FieldInsHandle,
    pub template: Template,
    pub cfg: Config,
    pub spawn_pos: [f32; 3],
    pub spawn_yaw: f32,
    pub origin_team: u8,
    pub dead: bool,
    /// Since when an ally has been beyond FollowDistance.
    far_since: Option<Instant>,
}

/// Player data copied out of WorldChrMan for this frame.
#[derive(Clone, Copy)]
pub struct PlayerView {
    pub ptr: usize,
    pub pos: [f32; 3],
    pub forward: [f32; 2],
    pub target: Option<[f32; 3]>,
    pub team: u8,
}

#[derive(Default)]
pub struct Arena {
    /// CSDebugChrCreator (0 = not looked up yet or not found).
    pub creator: usize,
    pub creator_checked: bool,
    pending: Vec<Request>,
    waiting: Option<(Instant, usize, Request)>,
    pub spawned: Vec<Spawned>,
    /// Removed characters: hidden at once, destroyed (slot freed) a second later.
    doomed: Vec<(FieldInsHandle, Instant)>,
    /// Hidden characters that could not be destroyed; they keep their slot
    /// until the next loading screen empties the debug ChrSet.
    pub kept_hidden: u32,
    /// The game's allocator lookup (game::find_allocator_lookup), 0 = destroy off.
    pub lookup: usize,
    /// Destroyed / kept since the last report (reported when `doomed` empties).
    run_destroyed: u32,
    run_kept: u32,
    pub saved_point: Option<[f32; 3]>,
    last: Option<(Template, Config)>,
    /// Most recent templates first (for the dialog's list), at most 8.
    pub recent: Vec<Template>,
    rng: Option<Lcg>,
    /// Research: the first spawns of a session look for the point their AI
    /// goes back to (handle, spawned at, stage, spawn point, matches).
    probes: Vec<(FieldInsHandle, Instant, u8, [f32; 3], Vec<(String, usize, &'static str)>)>,
    probes_done: u32,

    last_summary: Option<Instant>,
}

fn pos_of(chr: &ChrIns) -> [f32; 3] {
    let p = chr.modules.physics.position;
    [p.0, p.1, p.2]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn set_yaw(chr: &mut ChrIns, yaw: f32) {
    let [x, y, z, w] = formation::yaw_quat(yaw);
    let physics = &mut chr.modules.physics;
    physics.orientation = Quaternion(x, y, z, w);
    physics.interpolated_orientation = Quaternion(x, y, z, w);
}

fn teleport(chr: &mut ChrIns, pos: [f32; 3]) {
    let physics = &mut chr.modules.physics;
    let w = physics.position.3;
    physics.position = HavokPosition(pos[0], pos[1], pos[2], w);
    physics.chr_proxy_pos_update_requested = true;
}

impl Arena {
    fn creator<'a>(&self) -> Option<&'a mut CSDebugChrCreator> {
        (self.creator != 0).then(|| unsafe { &mut *(self.creator as *mut CSDebugChrCreator) })
    }

    /// Loading screen or title: the characters went with the world.
    pub fn forget_world(&mut self) {
        if !self.spawned.is_empty() || !self.pending.is_empty() || self.waiting.is_some() || !self.doomed.is_empty() {
            log(format!(
                "world unloaded: forgetting {} spawned, {} being removed, {} pending",
                self.spawned.len(),
                self.doomed.len(),
                self.pending.len() + self.waiting.is_some() as usize
            ));
        }
        self.spawned.clear();
        self.doomed.clear();
        self.kept_hidden = 0;
        self.pending.clear();
        self.waiting = None;
        self.saved_point = None;
        // WorldChrMan is rebuilt with the world; look the creator up again.
        self.creator = 0;
        self.creator_checked = false;
    }

    /// Template from the INI, or a copy of the nearest living character.
    fn template_for(&self, cfg: &Config, world: &WorldChrMan, player: &PlayerView) -> Option<Template> {
        if cfg.npc_param_id > 0 {
            let think = if cfg.think_id > 0 {
                cfg.think_id
            } else {
                crate::catalog::think_of(cfg.npc_param_id as u32).map_or(cfg.npc_param_id, |t| t as i32)
            };
            return Some(Template {
                chr_id: Config::resolve_chr_id(cfg.npc_param_id, cfg.chr_id),
                npc_param_id: cfg.npc_param_id,
                think_id: think,
                chara_init_id: cfg.chara_init_id,
            });
        }
        let mut best: Option<(f32, Template)> = None;
        for set in world.chr_sets.iter().flatten() {
            for c in set.characters() {
                if c as *const ChrIns as usize == player.ptr || c.npc_param_id <= 0 || c.modules.data.hp <= 0 {
                    continue;
                }
                // Enemies only: not players, ghosts or Torrent (not EnemyIns),
                // not neutral NPCs (team 0), spirit ashes (47) or the
                // player's own side.
                if c.as_subclass::<EnemyIns>().is_none() || matches!(c.team_type, 0 | 47) || c.team_type == player.team {
                    continue;
                }
                let d = dist(pos_of(c), player.pos);
                if d > COPY_RANGE || best.as_ref().is_some_and(|b| b.0 <= d) {
                    continue;
                }
                let think = c.as_subclass::<EnemyIns>().map_or(c.npc_param_id, |e| e.npc_think_param);
                best = Some((
                    d,
                    Template {
                        chr_id: if cfg.chr_id > 0 { cfg.chr_id } else { c.character_id as i32 },
                        npc_param_id: c.npc_param_id,
                        think_id: if cfg.think_id > 0 { cfg.think_id } else { think },
                        chara_init_id: cfg.chara_init_id,
                    },
                ));
            }
        }
        match best {
            Some((d, t)) => {
                log(format!("copying the nearest character ({d:.1} m): {t:?}"));
                Some(t)
            }
            None => {
                log("NpcParamId = 0 and no living enemy within 30 m to copy");
                crate::menu::notice("No enemy nearby");
                None
            }
        }
    }

    fn queue(&mut self, template: Template, mut cfg: Config, player: &PlayerView, now_seed: u64) {
        let alive = self.spawned.len() + self.pending.len() + self.waiting.is_some() as usize;
        let room = (cfg.max_spawned as usize).saturating_sub(alive);
        if room == 0 {
            log(format!("MaxSpawned = {} reached; remove some first", cfg.max_spawned));
            return;
        }
        if (cfg.count as usize) > room {
            log(format!("MaxSpawned = {}: spawning {room} of {}", cfg.max_spawned, cfg.count));
            cfg.count = room as u32;
        }
        let ahead = |d: f32| {
            [player.pos[0] + player.forward[0] * d, player.pos[1], player.pos[2] + player.forward[1] * d]
        };
        let center = match cfg.placement {
            Placement::Front | Placement::Random => ahead(cfg.distance),
            Placement::Player => player.pos,
            Placement::Target => player.target.unwrap_or_else(|| ahead(cfg.distance)),
            Placement::Saved => self.saved_point.unwrap_or_else(|| ahead(cfg.distance)),
        };
        let mut points = if cfg.placement == Placement::Random {
            let rng = self.rng.get_or_insert(Lcg(now_seed | 1));
            formation::random_positions(player.pos, cfg.distance, cfg.count, rng)
        } else {
            formation::positions(center, player.forward, cfg.count, cfg.formation)
        };
        for p in &mut points {
            p[1] += 0.5; // a little above the ground, as in phase 0
        }
        for pos in points {
            let yaw = if cfg.face_player {
                formation::yaw_towards(pos, player.pos)
            } else {
                player.forward[0].atan2(player.forward[1])
            };
            self.pending.push(Request { template, cfg, pos, yaw });
        }
        self.last = Some((template, cfg));
        self.recent.retain(|t| *t != template);
        self.recent.insert(0, template);
        self.recent.truncate(8);
        log(format!(
            "queued {} x {template:?} {:?}/{:?} at {:.1} m, ai {:?}, team {:?}",
            cfg.count, cfg.placement, cfg.formation, cfg.distance, cfg.ai, cfg.team
        ));
    }

    pub fn spawn_from_ini(&mut self, cfg: Config, world: &WorldChrMan, player: &PlayerView, seed: u64) {
        if self.creator == 0 {
            log("spawn: no CSDebugChrCreator in this game version / world");
            return;
        }
        if let Some(t) = self.template_for(&cfg, world, player) {
            self.queue(t, cfg, player, seed);
        }
    }

    pub fn repeat_last(&mut self, player: &PlayerView, seed: u64) {
        match self.last {
            Some((t, cfg)) if self.creator != 0 => self.queue(t, cfg, player, seed),
            Some(_) => log("repeat: no creator"),
            None => log("repeat: nothing spawned yet"),
        }
    }

    /// One request per frame; the creator answers within a frame (15 ms).
    pub fn step_spawning(&mut self, world: &mut WorldChrMan) {
        let Some(creator) = self.creator() else {
            self.pending.clear();
            return;
        };
        let latest = game::last_created(creator);
        if let Some((since, before, req)) = self.waiting {
            if latest != 0 && latest != before {
                let chr = unsafe { &mut *(latest as *mut ChrIns) };
                self.adopt(chr, req);
                self.waiting = None;
            } else if since.elapsed() > SPAWN_TIMEOUT {
                log(format!("spawn timed out (10 s): {:?}; spawn flag {}", req.template, creator.spawn));
                creator.spawn = false;
                self.waiting = None;
            }
            return;
        }
        if self.pending.is_empty() {
            return;
        }
        let used = game::debug_set_used(world).unwrap_or(u32::MAX);
        if used >= world.debug_chr_set.capacity {
            log(format!(
                "debug ChrSet full ({used}/{}): dropping {} request(s); use Remove all",
                world.debug_chr_set.capacity,
                self.pending.len()
            ));
            self.pending.clear();
            return;
        }
        let req = self.pending.remove(0);
        let t = req.template;
        game::request_spawn(
            creator,
            &SpawnArgs {
                chr_id: t.chr_id,
                npc_param_id: t.npc_param_id,
                think_id: t.think_id,
                chara_init_id: t.chara_init_id,
                pos: req.pos,
            },
        );
        self.waiting = Some((Instant::now(), latest, req));
    }

    /// Settings applied once, when the character appears.
    fn adopt(&mut self, chr: &mut ChrIns, req: Request) {
        let cfg = req.cfg;
        let data = &mut chr.modules.data;
        if cfg.max_hp > 0 {
            data.max_hp = cfg.max_hp;
        }
        data.hp = ((data.max_hp as i64 * cfg.hp_percent as i64) / 100).max(1) as i32;
        set_yaw(chr, req.yaw);
        let s = Spawned {
            handle: chr.field_ins_handle,
            template: req.template,
            cfg,
            spawn_pos: req.pos,
            spawn_yaw: req.yaw,
            origin_team: chr.team_type,
            dead: false,
            far_since: None,
        };
        if cfg.research && self.probes_done < 2 {
            self.probes_done += 1;
            self.probes.push((chr.field_ins_handle, Instant::now(), 0, pos_of(chr), Vec::new()));
        }
        log(format!(
            "spawned #{}: npc {} c{:04} team {} hp {}/{} at {:?} (asked {:?})",
            self.spawned.len() + 1,
            chr.npc_param_id,
            chr.character_id,
            chr.team_type,
            chr.modules.data.hp,
            chr.modules.data.max_hp,
            pos_of(chr),
            req.pos
        ));
        apply(chr, &s);
        self.spawned.push(s);
    }

    pub fn has_last(&self) -> bool {
        self.last.is_some()
    }

    pub fn set_ai_all(&mut self, ai: AiMode) {
        for s in &mut self.spawned {
            s.cfg.ai = ai;
        }
        if !self.spawned.is_empty() {
            log(format!("AI -> {ai:?} for {}", self.spawned.len()));
        }
    }

    /// New side for every spawned character. With `respawn`, living ones are
    /// spawned again where they stand (same facing and health %), so their AI
    /// starts without the player as a target; the old ones are removed.
    pub fn set_team_all(&mut self, team: Team, world: &mut WorldChrMan, respawn: bool) {
        if self.spawned.is_empty() {
            return;
        }
        if !respawn {
            for s in &mut self.spawned {
                s.cfg.team = team;
            }
            log(format!("team -> {team:?} for {}", self.spawned.len()));
            return;
        }
        let mut again = Vec::new();
        let mut kept = Vec::new();
        for mut s in std::mem::take(&mut self.spawned) {
            let Some(chr) = world.chr_ins_by_handle_mut(&s.handle) else { continue };
            if s.dead || chr.modules.data.hp <= 0 {
                s.cfg.team = team;
                kept.push(s);
                continue;
            }
            let pos = pos_of(chr);
            let q = chr.modules.physics.orientation;
            let f = formation::forward_of([q.0, q.1, q.2, q.3]);
            let data = &chr.modules.data;
            let pct = ((data.hp as i64 * 100) / data.max_hp.max(1) as i64).clamp(1, 100) as u32;
            let mut cfg = s.cfg;
            cfg.team = team;
            cfg.hp_percent = pct;
            hide(chr);
            self.doomed.push((s.handle, Instant::now()));
            again.push(Request { template: s.template, cfg, pos: [pos[0], pos[1] + 0.2, pos[2]], yaw: f[0].atan2(f[1]) });
        }
        self.spawned = kept;
        log(format!("team -> {team:?}: {} spawned again in place", again.len()));
        // Before any other request, so they come back at once.
        for r in again.into_iter().rev() {
            self.pending.insert(0, r);
        }
    }

    /// Allies follow the player: an active ally beyond `distance` metres for
    /// FOLLOW_AFTER is moved behind the player (no loading, so no hitch; the
    /// 1.0.0 test build spawned it again and hitched). Its home point (where
    /// it goes back to without a target) moves too. Their AI does not walk
    /// after the player: writing the home point every 0.5 s did nothing in
    /// game. One per frame; 0 = off.
    pub fn follow(&mut self, world: &mut WorldChrMan, player: &PlayerView, distance: f32) {
        if distance <= 0.0 {
            return;
        }
        let mut back = None;
        for (i, s) in self.spawned.iter_mut().enumerate() {
            let ally = s.cfg.team == Team::Ally && s.cfg.ai == AiMode::Active && !s.dead;
            let far = ally
                && world
                    .chr_ins_by_handle_mut(&s.handle)
                    .is_some_and(|c| c.modules.data.hp > 0 && dist(pos_of(c), player.pos) > distance);
            if !far {
                s.far_since = None;
                continue;
            }
            let since = *s.far_since.get_or_insert_with(Instant::now);
            if back.is_none() && since.elapsed() >= FOLLOW_AFTER {
                back = Some(i);
            }
        }
        let Some(i) = back else { return };
        let s = &mut self.spawned[i];
        s.far_since = None;
        let Some(chr) = world.chr_ins_by_handle_mut(&s.handle) else { return };
        let from = pos_of(chr);
        // Behind the player, side by side.
        let f = player.forward;
        let side = (i % 5) as f32 - 2.0;
        let pos = [
            player.pos[0] - f[0] * 2.5 + f[1] * side * 1.2,
            player.pos[1] + 0.3,
            player.pos[2] - f[1] * 2.5 - f[0] * side * 1.2,
        ];
        let yaw = f[0].atan2(f[1]);
        teleport(chr, pos);
        set_yaw(chr, yaw);
        // Only where the home point is expected (it holds the last one): in
        // another game version +0x80 may be something else.
        let home = chr as *mut ChrIns as usize + HOME_OFFSET;
        // The game shifts it with every other position as the player moves
        // through the map (seen in game: whole metres), so the check is
        // against where the character stands, not the stored spawn point: an
        // ally left behind stays near its home point.
        let held = game::read_vec(home);
        let holds_home = held.is_some_and(|v| {
            v.iter().all(|x| x.is_finite())
                && ((v[0] - from[0]).powi(2) + (v[2] - from[2]).powi(2)).sqrt() < 60.0
                && (v[1] - from[1]).abs() < 30.0
        });
        if holds_home {
            unsafe {
                let p = home as *mut f32;
                p.write(pos[0]);
                p.add(1).write(pos[1]);
                p.add(2).write(pos[2]);
            }
        }
        s.spawn_pos = pos;
        s.spawn_yaw = yaw;
        log(format!(
            "follow: npc {} was {:.0} m away, moved behind the player; home point {}",
            s.template.npc_param_id,
            dist(from, player.pos),
            if holds_home {
                "moved too".to_string()
            } else {
                format!("left (+0x80 holds {held:?}, the character was at {from:?})")
            }
        ));
    }

    /// Research probe: 3 s after spawning, list the places holding the spawn
    /// point (Havok or map-local); then move the character 4 m away and, 1.5 s
    /// later, keep the places that still hold the old point: candidates for
    /// the point its AI goes back to. `local_offset` = Havok - map-local.
    pub fn probe_tick(&mut self, world: &mut WorldChrMan, local_offset: [f32; 3]) {
        let mut done = Vec::new();
        for (i, (handle, at, stage, spawn, found)) in self.probes.iter_mut().enumerate() {
            let Some(chr) = world.chr_ins_by_handle_mut(handle) else {
                done.push(i);
                continue;
            };
            let ptr = chr as *mut ChrIns as usize;
            let local = [spawn[0] - local_offset[0], spawn[1] - local_offset[1], spawn[2] - local_offset[2]];
            match *stage {
                0 if at.elapsed() > Duration::from_secs(3) => {
                    let ai = chr.as_subclass::<EnemyIns>().map_or(0, |e| e.com_manipulator);
                    let t0 = Instant::now();
                    *found = game::probe_positions(ptr, ai, &[("havok", *spawn), ("local", local)]);
                    log(format!("probe took {} ms", t0.elapsed().as_millis()));
                    log(format!(
                        "probe npc {}: spawn {spawn:?} (local {local:?}), now {:?}; {} places: {}",
                        chr.npc_param_id,
                        pos_of(chr),
                        found.len(),
                        found.iter().map(|f| format!("{} {}", f.0, f.2)).collect::<Vec<_>>().join(", ")
                    ));
                    let p = pos_of(chr);
                    teleport(chr, [p[0] + 4.0, p[1] + 0.3, p[2]]);
                    *stage = 1;
                    *at = Instant::now();
                }
                1 if at.elapsed() > Duration::from_millis(1500) => {
                    let now = pos_of(chr);
                    let kept: Vec<String> = found
                        .iter()
                        .filter(|f| {
                            let t = if f.2 == "havok" { *spawn } else { local };
                            game::read_vec(f.1).is_some_and(|v| (0..3).all(|k| (v[k] - t[k]).abs() <= 0.3))
                        })
                        .map(|f| format!("{} {}", f.0, f.2))
                        .collect();
                    log(format!(
                        "probe npc {}: moved to {now:?}; still holding the spawn point: {}",
                        chr.npc_param_id,
                        if kept.is_empty() { "none".to_string() } else { kept.join(", ") }
                    ));
                    done.push(i);
                }
                _ => {}
            }
        }
        for i in done.into_iter().rev() {
            self.probes.remove(i);
        }
    }

    pub fn cycle_ai(&mut self) {
        let Some(first) = self.spawned.first() else { return };
        let next = first.cfg.ai.next();
        for s in &mut self.spawned {
            s.cfg.ai = next;
        }
        log(format!("AI -> {next:?} for {}", self.spawned.len()));
    }

    pub fn toggle_team(&mut self, world: &mut WorldChrMan, respawn: bool) {
        let Some(first) = self.spawned.first() else { return };
        let next = first.cfg.team.next();
        self.set_team_all(next, world, respawn);
    }

    /// Living characters: full health, back to where they appeared. Dead or
    /// lost ones: hidden and spawned again with the same settings.
    pub fn reset_all(&mut self, world: &mut WorldChrMan) {
        let mut respawn = Vec::new();
        let mut doomed = Vec::new();
        let mut healed = 0;
        self.spawned.retain(|s| {
            match world.chr_ins_by_handle_mut(&s.handle) {
                Some(chr) if !s.dead && chr.modules.data.hp > 0 => {
                    let data = &mut chr.modules.data;
                    data.hp = data.max_hp;
                    teleport(chr, s.spawn_pos);
                    set_yaw(chr, s.spawn_yaw);
                    healed += 1;
                    true
                }
                found => {
                    if let Some(chr) = found {
                        hide(chr);
                        doomed.push((s.handle, Instant::now()));
                    }
                    respawn.push(Request { template: s.template, cfg: s.cfg, pos: s.spawn_pos, yaw: s.spawn_yaw });
                    false
                }
            }
        });
        log(format!("reset: {healed} healed and moved back, {} spawned again", respawn.len()));
        self.pending.extend(respawn);
        self.doomed.extend(doomed);
    }

    pub fn remove_last(&mut self, world: &mut WorldChrMan) {
        let Some(s) = self.spawned.pop() else { return };
        if let Some(chr) = world.chr_ins_by_handle_mut(&s.handle) {
            hide(chr);
            self.doomed.push((s.handle, Instant::now()));
        }
        log(format!(
            "removed #{}, {} left",
            self.spawned.len() + 1,
            self.spawned.len()
        ));
    }

    pub fn remove_all(&mut self, world: &mut WorldChrMan) {
        let n = self.spawned.len();
        for s in self.spawned.drain(..) {
            if let Some(chr) = world.chr_ins_by_handle_mut(&s.handle) {
                hide(chr);
                self.doomed.push((s.handle, Instant::now()));
            }
        }
        self.pending.clear();
        self.waiting = None;
        log(format!(
            "removed all ({n}); debug ChrSet {:?} of {}",
            game::debug_set_used(world),
            world.debug_chr_set.capacity
        ));
    }

    /// Destroys removed characters once they have been hidden for a second,
    /// so nothing in the game still points at them when they go.
    fn destroy_doomed(&mut self, world: &mut WorldChrMan, destroy: bool) {
        let wait = Duration::from_secs(1);
        if !self.doomed.iter().any(|(_, t)| t.elapsed() > wait) {
            return;
        }
        let mut ready = Vec::new();
        self.doomed.retain(|(h, t)| {
            if ready.len() < DESTROY_PER_FRAME && t.elapsed() > wait {
                ready.push(*h);
                false
            } else {
                true
            }
        });
        let (mut destroyed, mut kept) = (0, 0);
        for h in ready {
            let Some(chr) = world.chr_ins_by_handle_mut(&h) else { continue };
            let ptr = chr as *mut ChrIns as usize;
            if !destroy || self.lookup == 0 {
                kept += 1;
                continue;
            }
            if let Some(c) = self.creator() {
                if game::last_created(c) == ptr {
                    c.last_created_chr = None;
                }
            }
            match game::destroy_debug_chr(world, ptr, self.lookup) {
                Ok(()) => destroyed += 1,
                Err(e) => {
                    log(format!("could not destroy a removed character ({e}); it stays hidden"));
                    kept += 1;
                }
            }
        }
        self.kept_hidden += kept;
        self.run_destroyed += destroyed;
        self.run_kept += kept;
        if self.doomed.is_empty() {
            log(format!(
                "destroyed {}, kept hidden {} (total kept {}); debug ChrSet {:?} of {}; map characters {}",
                self.run_destroyed,
                self.run_kept,
                self.kept_hidden,
                game::debug_set_used(world),
                world.debug_chr_set.capacity,
                game::map_chr_summary(world)
            ));
            self.run_destroyed = 0;
            self.run_kept = 0;
        }
    }

    /// Settings that hold every frame, and a short summary every 5 s.
    pub fn maintain(&mut self, world: &mut WorldChrMan, destroy: bool) {
        self.destroy_doomed(world, destroy);
        let summary = !self.spawned.is_empty()
            && self.last_summary.is_none_or(|t| t.elapsed() > Duration::from_secs(5));
        let mut lines = Vec::new();
        self.spawned.retain_mut(|s| {
            let Some(chr) = world.chr_ins_by_handle_mut(&s.handle) else {
                lines.push(format!("npc {}: no longer in the world, dropped", s.template.npc_param_id));
                return false;
            };
            let hp = chr.modules.data.hp;
            if hp <= 0 {
                if !s.dead {
                    s.dead = true;
                    lines.push(format!("npc {}: died (Reset spawns it again)", s.template.npc_param_id));
                }
                return true;
            }
            if s.cfg.infinite_hp && hp <= chr.modules.data.max_hp / 10 {
                chr.modules.data.hp = chr.modules.data.max_hp;
            }
            apply(chr, s);
            if summary {
                lines.push(format!(
                    "  npc {} c{:04}: hp {}/{} team {} {:?} at {:?}",
                    s.template.npc_param_id,
                    chr.character_id,
                    chr.modules.data.hp,
                    chr.modules.data.max_hp,
                    chr.team_type,
                    s.cfg.ai,
                    pos_of(chr)
                ));
            }
            true
        });
        if summary {
            self.last_summary = Some(Instant::now());
            lines.insert(
                0,
                format!(
                    "{} spawned, {} kept hidden; debug ChrSet {:?} of {}; map characters {}",
                    self.spawned.len(),
                    self.kept_hidden,
                    game::debug_set_used(world),
                    world.debug_chr_set.capacity,
                    game::map_chr_summary(world)
                ),
            );
        }
        for l in lines {
            log(l);
        }
    }
}

fn apply(chr: &mut ChrIns, s: &Spawned) {
    let f = &mut chr.debug_flags;
    f.set_disabled_updates(s.cfg.ai == AiMode::Paused);
    let still = s.cfg.ai == AiMode::Still;
    f.set_disabled_movement(still);
    f.set_disabled_secondary_actions(still);
    f.set_disabled_hit(s.cfg.invulnerable);
    chr.team_type = match s.cfg.team {
        Team::AsSpawned => s.origin_team,
        Team::Ally => TEAM_PLAYER_SIDE,
        Team::Neutral => s.cfg.neutral_team,
        Team::Raw(v) => v,
    };
}

fn hide(chr: &mut ChrIns) {
    chr.debug_flags.set_character_disabled(true);
    chr.debug_flags.set_force_unloaded(true);
}
