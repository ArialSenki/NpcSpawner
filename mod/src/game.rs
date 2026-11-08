//! Raw access to the game: memory checks, finding CSDebugChrCreator and
//! freeing the debug ChrSet. Everything here is unsafe underneath and is only
//! called from the game's frame task.

use std::mem::{offset_of, size_of};

use eldenring::cs::{CSDebugChrCreator, ChrIns, ChrSet, WorldChrMan};
use fromsoftware_shared::{F32Vector4, program::Program, vftable_classname};
use pelite::pe64::{Pe, PeObject};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery,
};

use crate::log;

/// True when [addr, addr + len) is committed, readable memory.
pub fn readable(addr: usize, len: usize) -> bool {
    if addr < 0x10000 {
        return false;
    }
    let mut info = MEMORY_BASIC_INFORMATION::default();
    let n = unsafe {
        VirtualQuery(
            Some(addr as *const core::ffi::c_void),
            &mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if n == 0 || info.State != MEM_COMMIT || info.Protect.0 & (PAGE_GUARD.0 | PAGE_NOACCESS.0) != 0 {
        return false;
    }
    addr + len <= info.BaseAddress as usize + info.RegionSize
}

pub fn read_usize(addr: usize) -> Option<usize> {
    readable(addr, 8).then(|| unsafe { (addr as *const usize).read_unaligned() })
}

/// RTTI class name of the object at `obj`, if it has a game vtable.
pub fn class_name_of(obj: usize) -> Option<String> {
    let vt = read_usize(obj)?;
    if !readable(vt.wrapping_sub(8), 16) {
        return None;
    }
    vftable_classname(&Program::current(), vt)
}

fn scan(world: usize, range: std::ops::Range<usize>) -> Option<(usize, usize)> {
    for off in range.step_by(8) {
        let Some(p) = read_usize(world + off) else { continue };
        if class_name_of(p).is_some_and(|n| n.contains("CSDebugChrCreator")) {
            return Some((off, p));
        }
    }
    None
}

/// Finds CSDebugChrCreator in WorldChrMan. The SDK's field (+0x1E648) is 0 in
/// WW 2.7.2.0, where the creator is at +0x1E658, so look near the SDK offset
/// first and only then in a wider window (slow: a few seconds).
/// Where the creator was found (0 = not yet). Every world load looks there
/// first: the scan reads RTTI names and took 200 ms in game.
static CREATOR_OFFSET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// The wide window was scanned without finding it: not again this session.
static WIDE_SCAN_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn find_creator(world: &WorldChrMan) -> usize {
    use std::sync::atomic::Ordering;
    let base = world as *const WorldChrMan as usize;
    let sdk = offset_of!(WorldChrMan, debug_chr_creator);
    let known = CREATOR_OFFSET.load(Ordering::Relaxed);
    let found = (known != 0)
        .then(|| scan(base, known..known + 8))
        .flatten()
        .or_else(|| scan(base, sdk - 0x100..sdk + 0x100))
        .or_else(|| {
            if WIDE_SCAN_FAILED.load(Ordering::Relaxed) {
                return None;
            }
            log("creator not near the SDK offset; scanning a wider window");
            let r = scan(base, 0x1D000..0x20000);
            if r.is_none() {
                WIDE_SCAN_FAILED.store(true, Ordering::Relaxed);
            }
            r
        });
    match found {
        Some((off, p)) if readable(p, size_of::<CSDebugChrCreator>()) => {
            if off != known {
                log(format!("CSDebugChrCreator at WorldChrMan+{off:#x} (SDK {sdk:#x}) = {p:#x}"));
            }
            CREATOR_OFFSET.store(off, Ordering::Relaxed);
            p
        }
        Some((off, _)) => {
            log(format!("CSDebugChrCreator at +{off:#x} is not fully readable; spawning disabled"));
            0
        }
        None => {
            log("CSDebugChrCreator not found; spawning disabled");
            0
        }
    }
}

pub struct SpawnArgs {
    pub chr_id: i32,
    pub npc_param_id: i32,
    pub think_id: i32,
    pub chara_init_id: i32,
    pub pos: [f32; 3],
}

/// Fills the creator and raises its spawn flag (the game creates the
/// character on its next step; it shows up in `last_created_chr`).
/// Same as fromsoftware-rs WorldChrMan::spawn_debug_character.
pub fn request_spawn(creator: &mut CSDebugChrCreator, a: &SpawnArgs) {
    let mut name = format!("c{:0>4}", a.chr_id).encode_utf16().collect::<Vec<_>>();
    name.resize(0x20, 0);
    let d = &mut creator.init_data;
    d.name.clone_from_slice(&name);
    d.chara_init_param_id = a.chara_init_id;
    d.npc_param_id = a.npc_param_id;
    d.npc_think_param_id = a.think_id;
    d.event_entity_id = 0;
    d.talk_id = 0;
    d.spawn_position = F32Vector4(a.pos[0], a.pos[1], a.pos[2], 0.0);
    creator.spawn = true;
}

pub fn last_created(creator: &CSDebugChrCreator) -> usize {
    creator.last_created_chr.map_or(0, |p| p.as_ptr() as usize)
}

fn in_module(va: usize) -> bool {
    Program::current().va_to_rva(va as u64).is_ok()
}

/// Finds `pattern` (None = any byte) in [start, start + len).
fn find_pattern(start: usize, len: usize, pattern: &[Option<u8>]) -> Option<usize> {
    if !readable(start, len) {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
    bytes
        .windows(pattern.len())
        .position(|w| w.iter().zip(pattern).all(|(b, p)| p.is_none_or(|p| p == *b)))
        .map(|i| start + i)
}

fn pat(s: &str) -> Vec<Option<u8>> {
    s.split_whitespace()
        .map(|b| if b == "??" { None } else { Some(u8::from_str_radix(b, 16).unwrap()) })
        .collect()
}

/// The game's "allocator of this pointer" lookup, taken from the debug
/// ChrSet's own free_chr_list (vtable slot 8), which destroys each character
/// as: alloc = lookup(chr); chr->vtable[1](chr, 0); alloc->vtable[13](alloc, chr);
/// then clears its entry. WW 2.7.2.0: free_chr_list 0x140495970, lookup
/// 0x140E1D770. Found by bytes so it follows other game versions.
pub fn find_allocator_lookup(world: &WorldChrMan) -> usize {
    let set = &world.debug_chr_set as *const ChrSet<ChrIns> as usize;
    let name = class_name_of(set);
    if !name.as_deref().is_some_and(|n| n.contains("ChrSet")) {
        log(format!("destroy disabled: debug ChrSet vtable class is {name:?}"));
        return 0;
    }
    let Some(f) = read_usize(set).and_then(|vt| read_usize(vt + 0x40)) else { return 0 };
    if !in_module(f) {
        log(format!("destroy disabled: free_chr_list {f:#x} is outside eldenring.exe"));
        return 0;
    }
    // mov rcx,rax; call lookup; mov r8,[r14]; xor edx,edx; mov rcx,r14;
    // mov rdi,rax; call [r8+8] (destructor); mov r8,[rdi]; mov rdx,r14;
    // mov rcx,rdi; call [r8+68h] (free)
    let body = pat("48 8B C8 E8 ?? ?? ?? ?? 4D 8B 06 33 D2 49 8B CE 48 8B F8 41 FF 50 08 4C 8B 07 49 8B D6 48 8B CF 41 FF 50 68");
    // movsxd rcx,esi; shl rcx,4; add rcx,[rbx+18h]; call clear_entry
    let clear = pat("48 63 CE 48 C1 E1 04 48 03 4B 18 E8");
    let (Some(at), Some(_)) = (find_pattern(f, 0x100, &body), find_pattern(f, 0x100, &clear)) else {
        log(format!("destroy disabled: free_chr_list at {f:#x} does not have the expected code"));
        return 0;
    };
    let rel = unsafe { ((at + 4) as *const i32).read_unaligned() } as isize;
    let lookup = (at as isize + 8 + rel) as usize;
    if !in_module(lookup) {
        return 0;
    }
    log(format!(
        "destroy ready: free_chr_list {:#x}, allocator lookup {:#x} (RVAs)",
        f - Program::current().image().as_ptr() as usize,
        lookup - Program::current().image().as_ptr() as usize
    ));
    lookup
}

/// Destroys one character of the debug ChrSet the way free_chr_list does,
/// without touching the ChrSet itself (free_chr_list also sets its capacity
/// to 0 and frees its entry array, which broke spawning in 0.1.0).
pub fn destroy_debug_chr(world: &WorldChrMan, chr_ptr: usize, lookup: usize) -> Result<(), &'static str> {
    let chr = unsafe { &*(chr_ptr as *const ChrIns) };
    let set = &world.debug_chr_set;
    let entries = set.entries.as_ptr() as usize;
    let entry_size = size_of::<eldenring::cs::ChrSetEntry<ChrIns>>();
    let entry = chr.chr_set_entry.as_ptr() as usize;
    if entry < entries || entry >= entries + set.capacity as usize * entry_size || (entry - entries) % entry_size != 0 {
        return Err("not in the debug ChrSet");
    }
    if read_usize(entry) != Some(chr_ptr) {
        return Err("its entry points elsewhere");
    }
    let lookup_fn: extern "C" fn(usize) -> usize = unsafe { std::mem::transmute(lookup) };
    let alloc = lookup_fn(chr_ptr);
    let dtor = read_usize(chr_ptr).and_then(|vt| read_usize(vt + 8)).filter(|f| in_module(*f));
    let free = read_usize(alloc).and_then(|vt| read_usize(vt + 0x68)).filter(|f| in_module(*f));
    let (Some(dtor), Some(free)) = (dtor, free) else {
        return Err("destructor or allocator not found");
    };
    let dtor: extern "C" fn(usize, u32) -> usize = unsafe { std::mem::transmute(dtor) };
    let free: extern "C" fn(usize, usize) = unsafe { std::mem::transmute(free) };
    dtor(chr_ptr, 0);
    free(alloc, chr_ptr);
    // clear_entry (WW 2.7.2.0 0x1403C3830): chr = null, status = 0, flags &= 0xF8
    unsafe {
        (entry as *mut usize).write(0);
        ((entry + 8) as *mut u16).write(0);
        let flags = (entry + 0xA) as *mut u8;
        flags.write(flags.read() & 0xF8);
    }
    Ok(())
}

/// Occupied entries of the debug ChrSet, reading its entry array only when it
/// is readable memory.
pub fn debug_set_used(world: &WorldChrMan) -> Option<u32> {
    let set = &world.debug_chr_set;
    let entries = set.entries.as_ptr() as usize;
    let size = set.capacity as usize * size_of::<eldenring::cs::ChrSetEntry<ChrIns>>();
    if set.capacity == 0 || set.capacity > 4096 || !readable(entries, size) {
        return None;
    }
    Some(set.characters().count() as u32)
}

/// Map characters (every ChrSet but the debug one): load status, update
/// (omission) mode, hidden and near the player. Shows what the game does to
/// them while many characters are spawned.
pub fn map_chr_summary(world: &WorldChrMan) -> String {
    let debug = &world.debug_chr_set as *const ChrSet<ChrIns> as usize;
    let mut status = [0u32; 6];
    let mut modes: Vec<(i32, u32)> = Vec::new();
    let (mut loaded, mut disabled, mut near, mut near_no_update) = (0u32, 0u32, 0u32, 0u32);
    for set in world.chr_sets.iter().flatten() {
        let set: &ChrSet<ChrIns> = set;
        if set as *const ChrSet<ChrIns> as usize == debug {
            continue;
        }
        let entries = set.entries.as_ptr() as usize;
        let size = set.capacity as usize * size_of::<eldenring::cs::ChrSetEntry<ChrIns>>();
        if set.capacity == 0 || set.capacity > 4096 || !readable(entries, size) {
            continue;
        }
        for i in 0..set.capacity as usize {
            let e = unsafe { &*set.entries.as_ptr().add(i) };
            let Some(chr) = e.chr_ins else { continue };
            let chr = unsafe { chr.as_ref() };
            loaded += 1;
            let st = unsafe { *(&e.chr_load_status as *const _ as *const u8) } as usize;
            if st < status.len() {
                status[st] += 1;
            }
            let mode = unsafe { *(&chr.omission_mode as *const _ as *const i32) };
            match modes.iter_mut().find(|m| m.0 == mode) {
                Some(m) => m.1 += 1,
                None => modes.push((mode, 1)),
            }
            if chr.debug_flags.character_disabled() {
                disabled += 1;
            }
            if chr.distance_to_player_sqr < 50.0 * 50.0 {
                near += 1;
                if mode == -2 {
                    near_no_update += 1;
                }
            }
        }
    }
    modes.sort();
    format!(
        "{loaded} loaded; status unl/init/act/net/ready/unloading {status:?}; update modes {modes:?}; \
         disabled {disabled}; within 50 m {near} ({near_no_update} not updated)"
    )
}

/// The game's per-frame character update budgets (WorldChrManDbg,
/// OmissionUpdateNum.Near/Far: Normal, Overload, Emergency). None when the
/// singleton is missing or its values do not look like budgets.
pub fn update_budgets() -> Option<(&'static mut eldenring::cs::WorldChrManDbg, [[i32; 3]; 2], i32, i32)> {
    use fromsoftware_shared::FromStatic;
    let dbg = unsafe { eldenring::cs::WorldChrManDbg::instance_mut() }.ok()?;
    let base = dbg as *mut eldenring::cs::WorldChrManDbg as usize;
    if !readable(base, size_of::<eldenring::cs::WorldChrManDbg>()) {
        return None;
    }
    let n = &dbg.omission_update_num_near;
    let f = &dbg.omission_update_num_far;
    let v = [[n.normal, n.overload, n.emergency], [f.normal, f.overload, f.emergency]];
    if v.iter().flatten().any(|x| !(0..=10000).contains(x)) {
        log(format!("update budgets look wrong ({v:?}); not used"));
        return None;
    }
    let ty = unsafe { *(&dbg.omission_update_num_type as *const _ as *const i32) };
    let ov = unsafe { *(&dbg.omission_update_num_type_override as *const _ as *const i32) };
    Some((dbg, v, ty, ov))
}

/// Raises the update budgets to at least `near` / `far` (0 = leave as is).
pub fn raise_update_budgets(near: i32, far: i32) {
    let Some((dbg, before, ty, ov)) = update_budgets() else { return };
    if near > 0 {
        let b = &mut dbg.omission_update_num_near;
        b.normal = b.normal.max(near);
        b.overload = b.overload.max(near);
        b.emergency = b.emergency.max(near);
    }
    if far > 0 {
        let b = &mut dbg.omission_update_num_far;
        b.normal = b.normal.max(far);
        b.overload = b.overload.max(far);
        b.emergency = b.emergency.max(far);
    }
    let after = update_budgets().map(|x| x.1);
    log(format!(
        "update budgets near/far [normal, overload, emergency]: {before:?} -> {after:?}; type {ty}, override {ov}"
    ));
}

/// Copies [base, base + len) into `out`; unreadable pages are left as zeros.
pub fn copy_memory(base: usize, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    let mut off = 0;
    while off < len {
        let addr = base + off;
        let page_end = (addr | 0xFFF) + 1;
        let n = (page_end - addr).min(len - off);
        if readable(addr, n) {
            unsafe { std::ptr::copy_nonoverlapping(addr as *const u8, out.as_mut_ptr().add(off), n) };
        }
        off += n;
    }
    out
}

/// Static slot holding the object with the game's active-character limits,
/// found once by code bytes (WW 2.7.2.0: [0x143D6D228], read at 0x14050FDD5
/// by the function that hands out activation slots, WorldChrMan +0x10 layout):
///   mov rax,[rip+X]; cmp byte [rax+F4h],0; je; mov ecx,[rax+F8h]; jmp;
///   mov ecx,[rax+ECh]; mov eax,[rbx+...]
/// It walks the priority list (WorldChrMan+0x1F1E8) and activates only the
/// first `+0xEC - [WorldChrMan+0x1E628]` (`+0xF8` while byte `+0xF4` is set);
/// `+0xF0` / `+0xFC` is the smaller full-update share.
/// 0 = not found yet (searched again at every world load until found).
pub static LIMITS_SLOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Finds the limits object once; later calls are free.
pub fn ensure_limits_slot() {
    use std::sync::atomic::Ordering;
    if LIMITS_SLOT.load(Ordering::Acquire) == 0 {
        LIMITS_SLOT.store(find_limits_slot(), Ordering::Release);
    }
}

pub fn find_limits_slot() -> usize {
    let program = Program::current();
    let image = program.image().as_ptr() as usize;
    let Some(text) = program.section_headers().by_name(".text") else { return 0 };
    let start = image + text.VirtualAddress as usize;
    let len = text.VirtualSize as usize;
    let pattern = pat("48 8B 05 ?? ?? ?? ?? 80 B8 F4 00 00 00 00 74 08 8B 88 F8 00 00 00 EB 06 8B 88 EC 00 00 00 8B 83");
    let Some(at) = find_pattern_in_text(start, len, &pattern) else {
        log("active-character limits: code not found (MaxActive unavailable)");
        return 0;
    };
    let rel = unsafe { ((at + 3) as *const i32).read_unaligned() } as isize;
    let slot = (at as isize + 7 + rel) as usize;
    log(format!(
        "active-character limits: code at RVA {:#x}, object pointer at RVA {:#x}",
        at - image,
        slot - image
    ));
    slot
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub flag_e8: u8,
    pub max_normal: i32,
    pub full_normal: i32,
    pub alt: u8,
    pub max_alt: i32,
    pub full_alt: i32,
}

fn limits_object() -> Option<usize> {
    let slot = LIMITS_SLOT.load(std::sync::atomic::Ordering::Acquire);
    if slot == 0 {
        return None;
    }
    let obj = read_usize(slot)?;
    readable(obj + 0xE8, 0x18).then_some(obj)
}

pub fn read_limits() -> Option<Limits> {
    let o = limits_object()?;
    unsafe {
        Some(Limits {
            flag_e8: *((o + 0xE8) as *const u8),
            max_normal: *((o + 0xEC) as *const i32),
            full_normal: *((o + 0xF0) as *const i32),
            alt: *((o + 0xF4) as *const u8),
            max_alt: *((o + 0xF8) as *const i32),
            full_alt: *((o + 0xFC) as *const i32),
        })
    }
}

/// Raises the active-character limits to at least `max` (0 = leave them).
/// The full-update share is raised by the same amount as the limit.
pub fn raise_limits(max: i32) {
    let Some(before) = read_limits() else {
        log("active-character limits: not available");
        return;
    };
    let plausible = |v: i32| (1..=1000).contains(&v);
    if ![before.max_normal, before.full_normal, before.max_alt, before.full_alt].iter().all(|v| plausible(*v)) {
        log(format!("active-character limits look wrong ({before:?}); not changed"));
        return;
    }
    if max > 0 {
        let o = limits_object().unwrap();
        let raise = |off: usize, to: i32| unsafe {
            let p = (o + off) as *mut i32;
            if *p < to {
                *p = to;
            }
        };
        raise(0xEC, max);
        raise(0xF8, max);
        raise(0xF0, before.full_normal + (max - before.max_normal).max(0));
        raise(0xFC, before.full_alt + (max - before.max_alt).max(0));
    }
    let after = read_limits();
    let changed = after.is_some_and(|a| {
        a.max_normal != before.max_normal || a.max_alt != before.max_alt || a.full_normal != before.full_normal
    });
    if changed || max == 0 {
        log(format!("active-character limits: {before:?} -> {after:?} (MaxActive = {max})"));
    }
}

/// find_pattern over a large range, one committed readable memory region at
/// a time (the game's code section can be split into several regions).
pub fn find_pattern_in_text(start: usize, len: usize, pattern: &[Option<u8>]) -> Option<usize> {
    let end = start + len;
    let mut addr = start;
    while addr < end {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let n = unsafe {
            VirtualQuery(Some(addr as *const core::ffi::c_void), &mut info, size_of::<MEMORY_BASIC_INFORMATION>())
        };
        if n == 0 {
            return None;
        }
        let region_end = (info.BaseAddress as usize + info.RegionSize).min(end);
        let chunk = region_end - addr;
        if chunk >= pattern.len() && readable(addr, chunk) {
            if let Some(at) = find_pattern(addr, chunk, pattern) {
                return Some(at);
            }
        }
        addr = region_end;
    }
    None
}


// ---- research: where a character keeps the point its AI goes back to

fn vec_at(addr: usize) -> Option<[f32; 3]> {
    readable(addr, 12).then(|| unsafe {
        let p = addr as *const f32;
        [p.read_unaligned(), p.add(1).read_unaligned(), p.add(2).read_unaligned()]
    })
}

fn near(v: [f32; 3], t: [f32; 3], tol: f32) -> bool {
    v.iter().all(|x| x.is_finite()) && (0..3).all(|i| (v[i] - t[i]).abs() <= tol)
}

/// Places (as "path") in a character's memory holding one of `targets`
/// (within 0.3 m): the character itself and its EnemyIns tail (0x600 bytes),
/// the objects it points to (0x400 bytes each), and the objects its AI
/// manipulator points to. Cheap: one VirtualQuery per block (the 1.0.0 probe
/// queried every 4 bytes and hung the game).
pub fn probe_positions(chr: usize, manipulator: usize, targets: &[(&str, [f32; 3])]) -> Vec<(String, usize, &'static str)> {
    let mut out = Vec::new();
    let scan = |base: usize, len: usize, path: &str, out: &mut Vec<(String, usize, &'static str)>| {
        if !readable(base, len) {
            return;
        }
        let words = unsafe { std::slice::from_raw_parts(base as *const f32, len / 4) };
        for i in 0..words.len().saturating_sub(2) {
            let v = [words[i], words[i + 1], words[i + 2]];
            for (name, t) in targets {
                if near(v, *t, 0.3) {
                    let name: &'static str = if *name == "havok" { "havok" } else { "local" };
                    out.push((format!("{path}+{:#x}", i * 4), base + i * 4, name));
                }
            }
        }
    };
    let pointers = |base: usize, len: usize| -> Vec<(usize, usize)> {
        if !readable(base, len) {
            return Vec::new();
        }
        let p = unsafe { std::slice::from_raw_parts(base as *const usize, len / 8) };
        p.iter()
            .enumerate()
            .filter(|(_, v)| **v > 0x10000 && **v < 0x7FFF_FFFF_FFFF && **v % 8 == 0)
            .map(|(i, v)| (i * 8, *v))
            .collect()
    };
    scan(chr, 0x600, "chr", &mut out);
    for (o1, p1) in pointers(chr, 0x600) {
        if p1 != chr {
            scan(p1, 0x400, &format!("chr+{o1:#x}"), &mut out);
        }
    }
    if manipulator != 0 {
        for (o2, p2) in pointers(manipulator, 0x800) {
            if p2 != chr && p2 != manipulator {
                scan(p2, 0x400, &format!("ai+{o2:#x}"), &mut out);
            }
        }
    }
    out
}

pub fn read_vec(addr: usize) -> Option<[f32; 3]> {
    vec_at(addr)
}
