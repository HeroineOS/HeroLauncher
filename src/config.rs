//! `~/.config/hero/launcher.toml`: favorites and looks. The launcher edits
//! its favorites itself, with toml_edit, so comments and layout are kept.

use std::path::PathBuf;

use serde::Deserialize;

pub const DEFAULT: &str = include_str!("../res/launcher.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// Desktop file ids shown first, as a grid ("Start" / favorites).
    pub favorites: Vec<String>,
    /// How apps are shown: "list" (rows), "grid" (icons with names), or
    /// "split" (favorites on one side, all apps on the other).
    pub layout: Layout,
    /// Category buttons (Games, Internet...) to narrow the apps down.
    pub categories: bool,
    /// The panel's size (width: 0 = a default for the layout).
    pub width: i32,
    pub height: i32,
    /// For apps that run in a terminal ("" = $TERMINAL or a common one).
    pub terminal: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { favorites: vec![], layout: Layout::List, categories: true, width: 0, height: 540, terminal: String::new() }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    #[default]
    List,
    Grid,
    Split,
}

impl Config {
    /// The panel's size.
    pub fn size(&self) -> (i32, i32) {
        let w = match (self.width, self.layout) {
            // Two sides need room.
            (w, Layout::Split) if w > 0 => w.max(640),
            (0, Layout::Split) => 720,
            (0, _) => 480,
            (w, _) => w,
        };
        (w, self.height)
    }
}

pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("hero/launcher.toml"))
}

pub fn parse(text: &str) -> Result<Config, String> {
    let mut c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    if c.width != 0 {
        c.width = c.width.clamp(240, 2000);
    }
    c.height = c.height.clamp(200, 2000);
    Ok(c)
}

/// The config, or the defaults (with a warning if the file is broken).
pub fn load() -> Config {
    let Some(p) = path() else { return Config::default() };
    match std::fs::read_to_string(&p) {
        Ok(t) => parse(&t).unwrap_or_else(|e| {
            eprintln!("herolauncher: {}: {e}", p.display());
            Config::default()
        }),
        Err(_) => parse(DEFAULT).unwrap_or_default(),
    }
}

/// Saves `favorites`, keeping the rest of the file as it is (written from
/// the commented default first if there's none).
pub fn save_favorites(favorites: &[String]) -> Result<(), String> {
    let p = path().ok_or("no config directory")?;
    let text = std::fs::read_to_string(&p).unwrap_or_else(|_| DEFAULT.to_owned());
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{}: {e}", p.display()))?;
    let mut list = toml_edit::Array::new();
    for f in favorites {
        list.push(f.as_str());
    }
    // Keep the comment after the key, if any.
    let decor = doc.get("favorites").and_then(|i| i.as_value()).map(|v| v.decor().clone());
    doc["favorites"] = toml_edit::value(list);
    if let (Some(d), Some(v)) = (decor, doc["favorites"].as_value_mut()) {
        *v.decor_mut() = d;
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses() {
        let c = parse(DEFAULT).unwrap();
        assert!(c.size().0 >= 240 && c.favorites.is_empty() && c.layout == Layout::List);
        assert_eq!(parse("layout = \"split\"\nwidth = 460").unwrap().size().0, 640, "two sides need room");
        assert_eq!(parse("layout = \"split\"").unwrap().size().0, 720);
        assert!(parse("layout = \"tiles\"").is_err());
        assert!(parse("favorits = []").is_err(), "typos are reported");
    }

    #[test]
    fn saving_keeps_comments() {
        let dir = std::env::temp_dir().join(format!("herolauncher-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("hero")).unwrap();
        std::fs::write(dir.join("hero/launcher.toml"), "# mine\nfavorites = [\"a\"] # top apps\nwidth = 500\n").unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        save_favorites(&["a".into(), "b".into()]).unwrap();
        let t = std::fs::read_to_string(dir.join("hero/launcher.toml")).unwrap();
        assert!(t.contains("# mine") && t.contains("# top apps") && t.contains("width = 500"), "{t}");
        assert_eq!(parse(&t).unwrap().favorites, ["a", "b"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
