//! Hotkeys the player assigns: a key and its modifiers per action, written as
//! "Ctrl+F6", "Shift+Num5", "Mouse4" or "None". Pure code (unit-tested),
//! except `pressed`, which reads the keyboard.

/// The actions with a hotkey, in this order everywhere: INI key, label.
pub const ACTIONS: [(&str, &str); 10] = [
    ("SavePoint", "Save the spawn point"),
    ("Spawn", "Spawn"),
    ("SpawnAgain", "Spawn the last one again"),
    ("CycleAi", "AI of all (active, paused, still)"),
    ("ToggleTeam", "Side of all (as spawned, yours)"),
    ("ResetAll", "Reset all"),
    ("RemoveLast", "Remove the last one"),
    ("RemoveAll", "Remove all"),
    ("TalkMenu", "Talk menu"),
    ("Dump", "Memory dump (Research = 1)"),
];
pub const COUNT: usize = ACTIONS.len();

pub const SAVE_POINT: usize = 0;
pub const SPAWN: usize = 1;
pub const SPAWN_AGAIN: usize = 2;
pub const CYCLE_AI: usize = 3;
pub const TOGGLE_TEAM: usize = 4;
pub const RESET_ALL: usize = 5;
pub const REMOVE_LAST: usize = 6;
pub const REMOVE_ALL: usize = 7;
pub const TALK_MENU: usize = 8;
pub const DUMP: usize = 9;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hotkey {
    /// Windows virtual-key code; 0 = no hotkey.
    pub vk: u16,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

const fn key(vk: u16, ctrl: bool, shift: bool) -> Hotkey {
    Hotkey { vk, ctrl, shift, alt: false }
}

/// The default hotkeys: Ctrl + F5...F10, and F11 for the talk menu.
pub const DEFAULTS: [Hotkey; COUNT] = [
    key(0x74, true, false), // Ctrl+F5
    key(0x75, true, false), // Ctrl+F6
    key(0x75, true, true),  // Ctrl+Shift+F6
    key(0x76, true, false), // Ctrl+F7
    key(0x77, true, false), // Ctrl+F8
    key(0x78, true, false), // Ctrl+F9
    key(0x79, true, false), // Ctrl+F10
    key(0x79, true, true),  // Ctrl+Shift+F10
    key(0x7A, false, false), // F11 (as NativeTweaks' talk menu)
    key(0x76, true, true),  // Ctrl+Shift+F7
];

/// Names of the keys that can be assigned (no modifiers, no Escape and no
/// left / right mouse button).
const NAMES: &[(&str, u16)] = &[
    ("Backspace", 0x08),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Pause", 0x13),
    ("Space", 0x20),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("NumMultiply", 0x6A),
    ("NumAdd", 0x6B),
    ("NumSubtract", 0x6D),
    ("NumDecimal", 0x6E),
    ("NumDivide", 0x6F),
    ("Mouse3", 0x04),
    ("Mouse4", 0x05),
    ("Mouse5", 0x06),
];

/// Name of a virtual key ("F6", "K", "7", "Num5", "PageUp"...).
pub fn key_name(vk: u16) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => Some((vk as u8 as char).to_string()),
        0x60..=0x69 => Some(format!("Num{}", vk - 0x60)),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6F)),
        _ => NAMES.iter().find(|n| n.1 == vk).map(|n| n.0.to_string()),
    }
}

fn key_of(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();
    if n.len() == 1 {
        let c = n.as_bytes()[0];
        return match c {
            b'0'..=b'9' => Some(c as u16),
            b'a'..=b'z' => Some(c.to_ascii_uppercase() as u16),
            _ => None,
        };
    }
    if let Some(d) = n.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()) {
        return (1..=24).contains(&d).then_some(0x6F + d);
    }
    if let Some(d) = n.strip_prefix("num").and_then(|d| d.parse::<u16>().ok()) {
        return (d <= 9).then_some(0x60 + d);
    }
    NAMES.iter().find(|k| k.0.eq_ignore_ascii_case(&n)).map(|k| k.1)
}

/// Every key `capture` listens to.
pub fn assignable() -> impl Iterator<Item = u16> {
    (0x30..=0x39).chain(0x41..=0x5A).chain(0x60..=0x69).chain(0x70..=0x87).chain(NAMES.iter().map(|n| n.1))
}

impl Hotkey {
    pub const NONE: Hotkey = Hotkey { vk: 0, ctrl: false, shift: false, alt: false };

    pub fn is_none(&self) -> bool {
        self.vk == 0
    }

    /// "Ctrl+Shift+F6", "F9", "None" (case ignored; "Control" and "-" also
    /// work). None for an unknown key.
    pub fn parse(text: &str) -> Option<Hotkey> {
        let t = text.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("off") {
            return Some(Hotkey::NONE);
        }
        let mut h = Hotkey::NONE;
        let parts: Vec<&str> = t.split(['+', '-']).map(str::trim).collect();
        let (last, mods) = parts.split_last()?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => h.ctrl = true,
                "shift" => h.shift = true,
                "alt" => h.alt = true,
                _ => return None,
            }
        }
        h.vk = key_of(last)?;
        Some(h)
    }

    pub fn name(&self) -> String {
        let Some(k) = key_name(self.vk) else { return "None".to_string() };
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s + &k
    }
}

/// Pairs of actions with the same hotkey (both run).
pub fn clashes(keys: &[Hotkey; COUNT]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for a in 0..COUNT {
        for b in a + 1..COUNT {
            if !keys[a].is_none() && keys[a] == keys[b] {
                out.push((a, b));
            }
        }
    }
    out
}

/// The keyboard right now, for `pressed` and capture.
#[derive(Clone, Copy)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

pub fn down(vk: u16) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
}

pub fn modifiers() -> Modifiers {
    Modifiers { ctrl: down(0x11), shift: down(0x10), alt: down(0x12) }
}

/// The hotkey is held now, with exactly its modifiers.
pub fn pressed(h: &Hotkey, m: Modifiers) -> bool {
    !h.is_none() && h.ctrl == m.ctrl && h.shift == m.shift && h.alt == m.alt && down(h.vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_names() {
        assert_eq!(Hotkey::parse("Ctrl+F6"), Some(DEFAULTS[SPAWN]));
        assert_eq!(Hotkey::parse(" ctrl + shift + f10 "), Some(DEFAULTS[REMOVE_ALL]));
        assert_eq!(Hotkey::parse("None"), Some(Hotkey::NONE));
        assert_eq!(Hotkey::parse(""), Some(Hotkey::NONE));
        assert_eq!(Hotkey::parse("Alt+Num5").map(|h| h.name()), Some("Alt+Num5".to_string()));
        assert_eq!(Hotkey::parse("shift-k").map(|h| h.name()), Some("Shift+K".to_string()));
        assert_eq!(Hotkey::parse("Mouse4").map(|h| h.vk), Some(0x05));
        assert_eq!(Hotkey::parse("Ctrl+"), None);
        assert_eq!(Hotkey::parse("Hyper+F1"), None);
        assert_eq!(Hotkey::parse("F25"), None);
        for h in DEFAULTS {
            assert_eq!(Hotkey::parse(&h.name()), Some(h));
        }
        for vk in assignable() {
            let h = Hotkey { vk, ..Hotkey::NONE };
            assert_eq!(Hotkey::parse(&h.name()), Some(h), "{vk:#x}");
        }
    }

    #[test]
    fn defaults_do_not_clash() {
        assert!(clashes(&DEFAULTS).is_empty());
        let mut k = DEFAULTS;
        k[RESET_ALL] = k[SPAWN];
        assert_eq!(clashes(&k), vec![(SPAWN, RESET_ALL)]);
    }
}
