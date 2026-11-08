//! The shared talk menu of the NativeTweaks ERNativeUI fork (r157+, F11 by
//! default). When ERNativeUI.dll exports ERNativeUIFork_TalkRegister, the
//! host owns the talk-menu key: NPC Spawner registers its row there and does
//! not listen to its own F11 (dialog.rs stays as the fallback without
//! it). The pages and rows are the same as dialog.rs's, without Back and
//! Leave (the host adds them). Callbacks run on the game thread.

use std::{
    ffi::c_void,
    panic::AssertUnwindSafe,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use windows::{
    Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    core::{PCSTR, w},
};

use crate::{
    arena::Template,
    dialog::{self, Choice, Page},
    erui::{self, Utf16View},
    log,
};

type Menu = *mut c_void;
type BuildFn = unsafe extern "C" fn(*mut c_void, Menu, u32);
type ChooseFn = unsafe extern "C" fn(*mut c_void, Menu, u32, u32);
type RegisterFn = unsafe extern "C" fn(*const Utf16View, BuildFn, ChooseFn, *mut c_void, *mut u64) -> erui::Result;
type AddRowFn = unsafe extern "C" fn(Menu, u32, *const Utf16View) -> erui::Result;
type NavFn = unsafe extern "C" fn(Menu, u32) -> erui::Result;
type MenuFn = unsafe extern "C" fn(Menu) -> erui::Result;
type OpenFn = unsafe extern "C" fn(u64) -> erui::Result;
type IsOpenFn = unsafe extern "C" fn() -> u32;
type OpenAtFn = unsafe extern "C" fn(u64, u32) -> erui::Result;

#[derive(Clone, Copy)]
struct Host {
    add_row: AddRowFn,
    open_page: NavFn,
    replace_page: NavFn,
    back: MenuFn,
    close: MenuFn,
    open: OpenFn,
    is_open: Option<IsOpenFn>,
    /// ERNativeUI r157-talk13+: opens a given page of the entry.
    open_at: Option<OpenAtFn>,
}

unsafe impl Send for Host {}

static HOST: Mutex<Option<Host>> = Mutex::new(None);
static ENTRY: AtomicU64 = AtomicU64::new(0);
/// Our page numbers: indexes into this list (0 = Main).
static PAGES: Mutex<Vec<Page>> = Mutex::new(Vec::new());
/// Rows of each page as last built (page number, choices by row id).
static ROWS: Mutex<Vec<(u32, Vec<Choice>)>> = Mutex::new(Vec::new());
/// Choices that act on the world, run by the frame task.
static QUEUE: Mutex<Vec<Choice>> = Mutex::new(Vec::new());

/// What the frame task last saw (the pages show it).
pub struct Snapshot {
    pub spawned: usize,
    pub recent: Vec<Template>,
    pub has_last: bool,
}
pub static SNAPSHOT: Mutex<Snapshot> = Mutex::new(Snapshot { spawned: 0, recent: Vec::new(), has_last: false });

/// True when NPC Spawner's row is in the host's talk menu.
pub fn registered() -> bool {
    ENTRY.load(Ordering::Acquire) != 0
}

pub fn take_queue() -> Vec<Choice> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn page_id(page: Page) -> u32 {
    let mut p = PAGES.lock().unwrap_or_else(|e| e.into_inner());
    if p.is_empty() {
        p.push(Page::Main);
    }
    match p.iter().position(|x| *x == page) {
        Some(i) => i as u32,
        None => {
            p.push(page);
            (p.len() - 1) as u32
        }
    }
}

/// The host opens an entry on its first page only: after a hotkey is
/// assigned, page 0 shows another page for a while.
struct FirstPage {
    page: Page,
    /// The menu was seen open since open_at.
    seen_open: bool,
    since: std::time::Instant,
    /// Page 0 was built once with it.
    built: bool,
    /// Page 0 will be built again by our own navigation (a setting on it,
    /// or back from one of its pages): keep it. Any other build of page 0
    /// (the player went back to the list of mods and in again) shows Main.
    expect: bool,
}
static FIRST_PAGE: Mutex<Option<FirstPage>> = Mutex::new(None);

fn page_of(id: u32) -> Page {
    if id == 0 {
        if let Some(f) = FIRST_PAGE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            return f.page;
        }
    }
    PAGES.lock().unwrap_or_else(|e| e.into_inner()).get(id as usize).copied().unwrap_or(Page::Main)
}

/// The page a build of `id` shows (page 0: see FirstPage).
fn page_to_build(id: u32) -> Page {
    if id == 0 {
        let mut first = FIRST_PAGE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = first.as_mut() {
            if !f.built || f.expect {
                f.built = true;
                f.expect = false;
                return f.page;
            }
            *first = None;
            return Page::Main;
        }
    }
    page_of(id)
}

/// Our navigation will show page 0 again: keep its page.
fn expect_first_page() {
    if let Some(f) = FIRST_PAGE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        f.expect = true;
    }
}

/// Opens NPC Spawner's entry of the shared talk menu at `page` (any thread):
/// with ERNativeUIFork_TalkOpenAt (Back from it goes to page 0, then the
/// list of mods), else page 0 shows `page` for a while (FirstPage).
pub fn open_at(page: Page) {
    let entry = ENTRY.load(Ordering::Acquire);
    if let (Some(h), true) = (host(), entry != 0) {
        if let Some(f) = h.open_at {
            let r = unsafe { f(entry, page_id(page)) };
            if r == erui::OK {
                return;
            }
            log(format!("talk: ERNativeUIFork_TalkOpenAt failed ({r}); opening the first page"));
        }
    }
    *FIRST_PAGE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(FirstPage { page, seen_open: false, since: std::time::Instant::now(), built: false, expect: false });
    open();
}

/// Frame task: page 0 is Main again once the menu opened by open_at closes
/// (or after a minute without ERNativeUIFork_TalkIsOpen).
pub fn tick() {
    let mut first = FIRST_PAGE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(f) = first.as_mut() else { return };
    let open = host().and_then(|h| h.is_open).map(|f| unsafe { f() } != 0);
    match open {
        Some(true) => f.seen_open = true,
        Some(false) if f.seen_open || f.since.elapsed() > std::time::Duration::from_secs(5) => *first = None,
        None if f.since.elapsed() > std::time::Duration::from_secs(60) => *first = None,
        _ => {}
    }
}

/// Registers the row (once ERNativeUI is connected; any thread).
pub fn register() {
    let Ok(module) = (unsafe { GetModuleHandleW(w!("ERNativeUI.dll")) }) else { return };
    let get = |name: &[u8]| unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) };
    let Some(reg) = get(b"ERNativeUIFork_TalkRegister\0") else {
        log("talk: ERNativeUI has no shared talk menu; F11 opens NPC Spawner's own");
        return;
    };
    let (Some(add_row), Some(open_page), Some(replace_page), Some(back), Some(close), Some(open)) = (
        get(b"ERNativeUIFork_TalkAddRow\0"),
        get(b"ERNativeUIFork_TalkOpenPage\0"),
        get(b"ERNativeUIFork_TalkReplacePage\0"),
        get(b"ERNativeUIFork_TalkBack\0"),
        get(b"ERNativeUIFork_TalkClose\0"),
        get(b"ERNativeUIFork_TalkOpen\0"),
    ) else {
        log("talk: ERNativeUI's talk menu lacks some exports; F11 opens NPC Spawner's own");
        return;
    };
    unsafe {
        *HOST.lock().unwrap_or_else(|e| e.into_inner()) = Some(Host {
            add_row: std::mem::transmute(add_row),
            open_page: std::mem::transmute(open_page),
            replace_page: std::mem::transmute(replace_page),
            back: std::mem::transmute(back),
            close: std::mem::transmute(close),
            open: std::mem::transmute(open),
            is_open: get(b"ERNativeUIFork_TalkIsOpen\0").map(|f| std::mem::transmute::<_, IsOpenFn>(f)),
            open_at: get(b"ERNativeUIFork_TalkOpenAt\0").map(|f| std::mem::transmute::<_, OpenAtFn>(f)),
        });
        let reg: RegisterFn = std::mem::transmute(reg);
        let name = erui::w("NPC Spawner");
        let view = Utf16View::new(&name);
        let mut entry = 0u64;
        let r = reg(&view, build, choose, std::ptr::null_mut(), &mut entry);
        if r == erui::OK && entry != 0 {
            ENTRY.store(entry, Ordering::Release);
            log("talk: NPC Spawner is a row of ERNativeUI's talk menu; its own F11 is off");
        } else {
            log(format!("talk: ERNativeUIFork_TalkRegister failed ({r}); F11 opens NPC Spawner's own"));
        }
    }
}

/// Opens NPC Spawner's page of the shared talk menu (any thread).
pub fn open() {
    let entry = ENTRY.load(Ordering::Acquire);
    if let (Some(h), true) = (host(), entry != 0) {
        unsafe { (h.open)(entry) };
    }
}

fn host() -> Option<Host> {
    *HOST.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe extern "C" fn build(_: *mut c_void, menu: Menu, page: u32) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| build_inner(menu, page))).is_err() {
        log("talk: panic while building a page (caught)");
    }
}

fn build_inner(menu: Menu, page: u32) {
    let Some(h) = host() else { return };
    let cfg = crate::current_config_pub();
    let hits: Vec<String> = crate::menu::search_hits().iter().map(|h| h.label.clone()).collect();
    let (spawned, recent, has_last) = {
        let s = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner());
        (s.spawned, s.recent.clone(), s.has_last)
    };
    let v = dialog::View {
        cfg: &cfg,
        spawned,
        recent: &recent,
        hits: &hits,
        has_last,
        describe: &crate::describe_pub,
    };
    let rows: Vec<(Choice, String)> = dialog::Dialog::rows(page_to_build(page), &v)
        .into_iter()
        .filter(|(c, _)| !matches!(c, Choice::Back | Choice::Leave))
        .take(63)
        .collect();
    for (i, (_, text)) in rows.iter().enumerate() {
        let t = erui::w(text);
        let view = Utf16View::new(&t);
        unsafe { (h.add_row)(menu, i as u32, &view) };
    }
    let mut all = ROWS.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|(p, _)| *p != page);
    all.push((page, rows.into_iter().map(|r| r.0).collect()));
}

unsafe extern "C" fn choose(_: *mut c_void, menu: Menu, page: u32, row: u32) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| choose_inner(menu, page, row))).is_err() {
        log("talk: panic while handling a choice (caught)");
    }
}

fn choose_inner(menu: Menu, page: u32, row: u32) {
    let Some(h) = host() else { return };
    let choice = ROWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(p, _)| *p == page)
        .and_then(|(_, r)| r.get(row as usize).copied());
    let Some(choice) = choice else { return };
    log(format!("talk menu: {choice:?}"));
    let recent = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()).recent.clone();
    crate::settings_choice(choice, &crate::menu::search_hits(), &recent);
    if crate::is_world_choice(choice) {
        QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push(choice);
    }
    let from = page_of(page);
    let Some(to) = dialog::Dialog::next_page(from, choice) else {
        unsafe { (h.close)(menu) };
        return;
    };
    // Page 0 shown as another page: it is built again after a setting on it
    // and when one of its pages goes back to it.
    if (page == 0 && to != Page::Main) || matches!((from, to), (Page::Hotkey(_), Page::Hotkeys)) {
        expect_first_page();
    }
    unsafe {
        match (from, to) {
            _ if to == from => {}
            (Page::List(a, _), Page::List(b, _)) if a == b => {
                (h.replace_page)(menu, page_id(to));
            }
            (Page::Hotkey(_), Page::Hotkeys) => {
                (h.back)(menu);
            }
            // A character was picked deep in the lists: back to the first page.
            (_, Page::Main) if from != Page::Options => {
                (h.close)(menu);
                (h.open)(ENTRY.load(Ordering::Acquire));
            }
            (_, Page::Main) => {
                (h.back)(menu);
            }
            _ => {
                (h.open_page)(menu, page_id(to));
            }
        }
    }
}
