//! The compositor shortcut that opens the launcher, so pressing it again
//! closes it. HeroWM (like niri and other smithay compositors) sends every
//! key to an overlay that holds the keyboard, its own shortcuts included,
//! so the shortcut never reaches HeroWM while the launcher is open: the
//! launcher reads `[keybinds]` in `~/.config/fht/compositor.toml` and
//! watches for the ones that run `herolauncher` itself. (sway and
//! Hyprland handle their shortcuts first and just run it again.)

use std::ffi::{c_char, c_int, CString};

use heroui::fltk::app;
use heroui::fltk::enums::EventState;

/// Modifiers (Super, Shift, Alt, Ctrl) and the key's X keysym, which is
/// what FLTK reports for keys (lowercase for letters).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Combo {
    pub mods: [bool; 4],
    pub key: u32,
}

#[link(name = "xkbcommon")]
extern "C" {
    fn xkb_keysym_from_name(name: *const c_char, flags: c_int) -> u32;
}

/// A keysym by name ("space", "Return", "d"), any case; letters lowercase.
fn keysym(name: &str) -> Option<u32> {
    let c = CString::new(name).ok()?;
    let k = match unsafe { xkb_keysym_from_name(c.as_ptr(), 0) } {
        0 => unsafe { xkb_keysym_from_name(c.as_ptr(), 1) },
        k => k,
    };
    // Shifted letters are the same key.
    Some(if (0x41..=0x5a).contains(&k) { k + 0x20 } else { k }).filter(|&k| k != 0)
}

/// A HeroWM key pattern, e.g. "Super-Space" or "M-S-d".
fn parse(pattern: &str) -> Option<Combo> {
    let mut mods = [false; 4];
    let mut key = None;
    for part in pattern.split('-') {
        match part.trim() {
            "Super" | "Mod" | "Logo" | "Meta" | "M" => mods[0] = true,
            "Shift" | "S" => mods[1] = true,
            "Alt" | "A" => mods[2] = true,
            "Ctrl" | "Control" | "C" => mods[3] = true,
            k if key.is_none() => key = keysym(k),
            _ => return None,
        }
    }
    Some(Combo { mods, key: key? })
}

/// The keybinds in a HeroWM config that run herolauncher.
fn from_herowm(text: &str) -> Vec<Combo> {
    let Ok(doc) = text.parse::<toml::Table>() else { return vec![] };
    let Some(binds) = doc.get("keybinds").and_then(|b| b.as_table()) else { return vec![] };
    binds
        .iter()
        .filter(|(_, v)| {
            let t = v.as_table();
            t.and_then(|t| t.get("action")).and_then(|a| a.as_str()) == Some("run-command")
                && t.and_then(|t| t.get("arg")).and_then(|a| a.as_str()).is_some_and(|a| a.split_whitespace().next().is_some_and(|c| c.ends_with("herolauncher")))
        })
        .filter_map(|(k, _)| parse(k))
        .collect()
}

/// The shortcuts to close on (none if there's no HeroWM config).
pub fn load() -> Vec<Combo> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    dir.and_then(|d| std::fs::read_to_string(d.join("fht/compositor.toml")).ok()).map_or(vec![], |t| from_herowm(&t))
}

/// Whether the key event being handled is the shortcut that opened the
/// launcher (read once).
pub fn is_toggle() -> bool {
    static KEYS: std::sync::OnceLock<Vec<Combo>> = std::sync::OnceLock::new();
    pressed(KEYS.get_or_init(load))
}

/// Whether the key event being handled is one of `combos`.
fn pressed(combos: &[Combo]) -> bool {
    if combos.is_empty() {
        return false;
    }
    let s = app::event_state();
    let mods = [s.contains(EventState::Meta), s.contains(EventState::Shift), s.contains(EventState::Alt), s.contains(EventState::Ctrl)];
    let key = app::event_key().bits() as u32;
    let key = if (0x41..=0x5a).contains(&key) { key + 0x20 } else { key };
    combos.iter().any(|c| c.mods == mods && c.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_launcher_keybinds() {
        let conf = r#"
[keybinds]
Super-Return = { action = "run-command", arg = "foot" }
Super-Space = { action = "run-command", arg = "herolauncher" }
M-S-d = { action = "run-command", arg = "/usr/bin/herolauncher --menu" }
Super-Shift-Space = "select-previous-layout"

[keybinds.Alt-F1]
action = "run-command"
arg = "herolauncher"
"#;
        let c = from_herowm(conf);
        assert_eq!(c.len(), 3, "{c:?}");
        assert!(c.contains(&Combo { mods: [true, false, false, false], key: 0x20 }));
        assert!(c.contains(&Combo { mods: [true, true, false, false], key: 0x64 }));
        assert!(c.contains(&Combo { mods: [false, false, true, false], key: 0xffbe }));
        assert_eq!(parse("Super-NotAKey"), None);
    }
}
