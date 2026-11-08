//! Our own texts in the game's talk menus. The game reads every message
//! through MsgRepositoryImp's lookup (WW 2.7.2.0 0x142671370, found by bytes
//! in all five supported versions):
//!     lookup(repo, bnd, category, id): bnd < [repo+10h] && category < [repo+14h]
//!         -> file = [[repo+8] + bnd*8][category]; jmp fmg_lookup(file, id)
//! Its first 12 bytes become `mov rax, hook; jmp rax`; the hook answers our
//! reserved ids and otherwise does exactly what the original did (the code is
//! that short, so no trampoline is needed).

use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use fromsoftware_shared::program::Program;
use pelite::pe64::{Pe, PeObject};
use windows::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
    Threading::GetCurrentProcess,
};

use crate::log;

/// First id of our texts (any category). 64 slots.
pub const BASE_ID: i32 = 1_990_000_000;
pub const SLOTS: usize = 64;
const SLOT_LEN: usize = 160;

struct Slot(UnsafeCell<[u16; SLOT_LEN]>);
// Written by the frame task, read by the game's lookups; a torn read can only
// show a mixed text for one frame.
unsafe impl Sync for Slot {}

static TEXTS: [Slot; SLOTS] = [const { Slot(UnsafeCell::new([0; SLOT_LEN])) }; SLOTS];
static FMG_LOOKUP: AtomicUsize = AtomicUsize::new(0);
/// Address of MsgRepositoryImp::LookupEntry (0 = not found yet).
static READY: AtomicUsize = AtomicUsize::new(0);
/// Its first 12 bytes, put back when the dialog closes.
static ORIGINAL: std::sync::Mutex<[u8; 12]> = std::sync::Mutex::new([0; 12]);
static HOOKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The game's message lookup was found: game texts (names) can be read.
pub fn ready() -> bool {
    READY.load(Ordering::Acquire) != 0
}

/// Our dialog texts are being served (the hook is in place).
pub fn hooked() -> bool {
    HOOKED.load(Ordering::Acquire)
}

/// Writes 12 bytes of code that other threads may be running: first a
/// 2-byte `jmp $` at the head (one aligned store, so a thread arriving now
/// waits on it), then the tail, then the real head. A thread already past
/// the head can still meet a half-written tail; the hook is only put in and
/// taken out when NPC Spawner's own talk menu opens and closes.
fn write_code(at: usize, bytes: &[u8; 12]) -> bool {
    let mut old = PAGE_PROTECTION_FLAGS(0);
    unsafe {
        if VirtualProtect(at as *const _, 12, PAGE_EXECUTE_READWRITE, &mut old).is_err() {
            return false;
        }
        if at % 2 == 0 {
            use std::sync::atomic::{AtomicU16, Ordering};
            let head = &*(at as *const AtomicU16);
            head.store(u16::from_le_bytes([0xEB, 0xFE]), Ordering::SeqCst);
            let _ = FlushInstructionCache(GetCurrentProcess(), Some(at as *const _), 2);
            std::ptr::copy_nonoverlapping(bytes.as_ptr().add(2), (at + 2) as *mut u8, 10);
            let _ = FlushInstructionCache(GetCurrentProcess(), Some(at as *const _), 12);
            head.store(u16::from_le_bytes([bytes[0], bytes[1]]), Ordering::SeqCst);
        } else {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, 12);
        }
        let mut ignored = PAGE_PROTECTION_FLAGS(0);
        let _ = VirtualProtect(at as *const _, 12, old, &mut ignored);
        let _ = FlushInstructionCache(GetCurrentProcess(), Some(at as *const _), 12);
    }
    true
}

fn our_patch() -> [u8; 12] {
    let mut patch = [0u8; 12];
    patch[0] = 0x48;
    patch[1] = 0xB8;
    patch[2..10].copy_from_slice(&(hook as *const () as usize as u64).to_le_bytes());
    patch[10] = 0xFF;
    patch[11] = 0xE0;
    patch
}

/// Hooks the message lookup while the talk menu is open, so its entries show
/// our texts. Only then: other mods (NativeTweaks' shop) look this function
/// up by its bytes, which the hook changes. Skipped when someone else has
/// changed its bytes since start-up.
pub fn hook_for_dialog() -> bool {
    if hooked() {
        return true;
    }
    let at = READY.load(Ordering::Acquire);
    if at == 0 || !crate::game::readable(at, 12) {
        return false;
    }
    let now = unsafe { *(at as *const [u8; 12]) };
    if now != *ORIGINAL.lock().unwrap_or_else(|e| e.into_inner()) {
        log("texts: the message lookup was changed by another mod; the talk menu uses the game's Leave text only");
        return false;
    }
    if !write_code(at, &our_patch()) {
        log("texts: VirtualProtect failed");
        return false;
    }
    HOOKED.store(true, Ordering::Release);
    true
}

/// Puts the message lookup back when the talk menu closes (if it still has
/// our hook).
pub fn unhook() {
    if !hooked() {
        return;
    }
    let at = READY.load(Ordering::Acquire);
    if unsafe { *(at as *const [u8; 12]) } == our_patch() {
        let original = *ORIGINAL.lock().unwrap_or_else(|e| e.into_inner());
        write_code(at, &original);
    }
    HOOKED.store(false, Ordering::Release);
}

pub fn id(slot: usize) -> i32 {
    BASE_ID + slot as i32
}

/// Sets the text of `slot` (cut to 159 UTF-16 units).
pub fn set(slot: usize, text: &str) {
    let Some(s) = TEXTS.get(slot) else { return };
    let buf = unsafe { &mut *s.0.get() };
    let mut n = 0;
    for u in text.encode_utf16().take(SLOT_LEN - 1) {
        buf[n] = u;
        n += 1;
    }
    buf[n] = 0;
}

extern "C" fn hook(repo: usize, bnd: u32, category: u32, id: i32) -> *const u16 {
    if (BASE_ID..BASE_ID + SLOTS as i32).contains(&id) {
        return unsafe { (*TEXTS[(id - BASE_ID) as usize].0.get()).as_ptr() };
    }
    unsafe {
        if bnd >= *((repo + 0x10) as *const u32) || category >= *((repo + 0x14) as *const u32) {
            return std::ptr::null();
        }
        let banks = *((repo + 8) as *const usize);
        let files = *((banks + bnd as usize * 8) as *const usize);
        if files == 0 {
            return std::ptr::null();
        }
        let file = *((files + category as usize * 8) as *const usize);
        if file == 0 {
            return std::ptr::null();
        }
        let f: extern "C" fn(usize, i32) -> *const u16 = std::mem::transmute(FMG_LOOKUP.load(Ordering::Relaxed));
        f(file, id)
    }
}

/// Reads a game message (None when missing). `category` is the msgbnd file id.
pub fn lookup(category: u32, id: i32) -> Option<String> {
    use fromsoftware_shared::FromStatic;
    if !ready() {
        return None;
    }
    let repo = unsafe { eldenring::cs::MsgRepositoryImp::instance() }.ok()? as *const _ as usize;
    let p = hook(repo, 0, category, id);
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while n < 512 && unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) });
    (!s.is_empty()).then_some(s)
}

fn pat(s: &str) -> Vec<Option<u8>> {
    s.split_whitespace()
        .map(|b| if b == "??" { None } else { Some(u8::from_str_radix(b, 16).unwrap()) })
        .collect()
}

/// Searches of the code so far, and when the last one ran.
static TRIES: std::sync::Mutex<(u32, Option<std::time::Instant>)> = std::sync::Mutex::new((0, None));
/// A failed search scans the whole code section: retried every 5 s, at most
/// 24 times (it can fail while the game is still starting).
const RETRY_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_TRIES: u32 = 24;

/// Finds the message lookup (once, early); the hook itself is only put in
/// while the talk menu is open (hook_for_dialog). False when the code is not
/// the expected one (another mod may already hook it).
pub fn install() -> bool {
    if ready() {
        return true;
    }
    // Another thread searching now, too soon after a failed search, or
    // given up: skip.
    let Ok(mut tries) = TRIES.try_lock() else { return false };
    if ready() {
        return true;
    }
    if tries.0 >= MAX_TRIES || tries.1.is_some_and(|t| t.elapsed() < RETRY_EVERY) {
        return false;
    }
    tries.0 += 1;
    tries.1 = Some(std::time::Instant::now());
    let attempt = tries.0;
    let program = Program::current();
    let image = program.image().as_ptr() as usize;
    let Some(text) = program.section_headers().by_name(".text") else { return false };
    let start = image + text.VirtualAddress as usize;
    let pattern = pat(
        "3B 51 10 73 29 44 3B 41 14 73 23 48 8B 41 08 8B D2 48 8B 0C D0 48 85 C9 74 14 41 8B C0 48 8B 0C C1 48 85 C9 74 08 41 8B D1 E9",
    );
    let Some(at) = crate::game::find_pattern_in_text(start, text.VirtualSize as usize, &pattern) else {
        // WW 2.7.2.0 / JP 2.7.2.1 keep it at RVA 0x2671370: show what is there.
        let known = image + 0x2671370;
        let bytes = if crate::game::readable(known, 16) {
            let b = unsafe { std::slice::from_raw_parts(known as *const u8, 16) };
            b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
        } else {
            "unreadable".to_string()
        };
        if attempt == 1 || attempt == MAX_TRIES {
            log(format!(
                "texts: message lookup not found (bytes at the 1.17.2 address: {bytes}); {}",
                if attempt == MAX_TRIES {
                    "giving up: game names come from the built-in list and F11 shows only Leave"
                } else {
                    "trying again every 5 s"
                }
            ));
        }
        return false;
    };
    let rel = unsafe { ((at + 0x2A) as *const i32).read_unaligned() } as isize;
    let fmg = (at as isize + 0x2E + rel) as usize;
    FMG_LOOKUP.store(fmg, Ordering::Relaxed);
    *ORIGINAL.lock().unwrap_or_else(|e| e.into_inner()) = unsafe { *(at as *const [u8; 12]) };
    READY.store(at, Ordering::Release);
    log(format!(
        "texts: message lookup found at RVA {:#x} (file lookup RVA {:#x}); hooked only while the talk menu is open",
        at - image,
        fmg - image
    ));
    true
}

/// Every (id, text) of one message file. The loaded FMG keeps its
/// group count at +0x0C, the string offset table pointer at +0x18 and its
/// groups at +0x28 (16 bytes each: first index, first id, last id), as the
/// game's own file lookup (WW 2.7.2.0 0x142671C40) reads them.
/// Number of message banks (base game, DLC...).
pub fn bank_count() -> u32 {
    use fromsoftware_shared::FromStatic;
    let Ok(repo) = (unsafe { eldenring::cs::MsgRepositoryImp::instance() }) else { return 0 };
    unsafe { *((repo as *const _ as usize + 0x10) as *const u32) }
}

pub fn all_of(bank: u32, category: u32) -> Vec<(i32, String)> {
    use fromsoftware_shared::FromStatic;
    let mut out = Vec::new();
    let Ok(repo) = (unsafe { eldenring::cs::MsgRepositoryImp::instance() }) else { return out };
    let repo = repo as *const _ as usize;
    unsafe {
        if bank >= *((repo + 0x10) as *const u32) || category >= *((repo + 0x14) as *const u32) {
            return out;
        }
        let banks = *((repo + 8) as *const usize);
        let files = *((banks + bank as usize * 8) as *const usize);
        if files == 0 {
            return out;
        }
        let file = *((files + category as usize * 8) as *const usize);
        if file == 0 || !crate::game::readable(file, 0x28) {
            return out;
        }
        let groups = *((file + 0x0C) as *const u32) as usize;
        let table = *((file + 0x18) as *const usize);
        if groups > 100_000 || !crate::game::readable(file + 0x28, groups * 16) {
            return out;
        }
        for g in 0..groups {
            let base = file + 0x28 + g * 16;
            let index = *(base as *const i32);
            let first = *((base + 4) as *const i32);
            let last = *((base + 8) as *const i32);
            if last < first || last - first > 1_000_000 {
                continue;
            }
            for id in first..=last {
                let off = *((table + (index + id - first) as usize * 8) as *const usize);
                if off == 0 {
                    continue;
                }
                let p = (file + off) as *const u16;
                let mut n = 0;
                while n < 512 && *p.add(n) != 0 {
                    n += 1;
                }
                if n > 0 {
                    out.push((id, String::from_utf16_lossy(std::slice::from_raw_parts(p, n))));
                }
            }
        }
    }
    out
}
