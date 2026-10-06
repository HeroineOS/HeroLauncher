//! Installed applications, from `.desktop` files (the freedesktop.org
//! Desktop Entry spec): finding them, searching them, starting them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct App {
    /// Desktop file id: "firefox-esr" for firefox-esr.desktop,
    /// "kde-okular" for kde/okular.desktop.
    pub id: String,
    pub name: String,
    /// `GenericName=` ("Web Browser").
    pub generic: String,
    pub comment: String,
    /// `Keywords=`, `;`-separated.
    pub keywords: String,
    /// `Icon=`: a theme icon name or a path.
    pub icon: String,
    /// `Exec=` with the field codes (%U, %f...) removed.
    pub exec: String,
    pub terminal: bool,
    /// Its menu category (one, like XFCE's menu): an index into
    /// [`CATEGORIES`].
    pub category: usize,
}

/// Menu categories, like XFCE's: (label, the freedesktop.org main
/// categories that go in it, best first). An app goes in the first one
/// whose categories it lists (Settings before System: many apps are
/// both); the last one, "Other", takes the rest.
pub const CATEGORIES: [(&str, &[&str]); 12] = [
    ("Games", &["Game"]),
    ("Development", &["Development"]),
    ("Graphics", &["Graphics"]),
    ("Internet", &["Network"]),
    ("Multimedia", &["AudioVideo", "Audio", "Video"]),
    ("Office", &["Office"]),
    ("Education", &["Education", "Science"]),
    ("Settings", &["Settings"]),
    ("System", &["System"]),
    ("Accessories", &["Utility"]),
    ("Documentation", &["Documentation"]),
    ("Other", &[]),
];

/// The category for a `Categories=` value.
fn category_of(categories: &str) -> usize {
    let listed: Vec<&str> = categories.split(';').map(str::trim).collect();
    CATEGORIES.iter().position(|(_, main)| main.iter().any(|m| listed.contains(m))).unwrap_or(CATEGORIES.len() - 1)
}

/// Where `.desktop` files are, most important first (the user's own
/// override the system's).
fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) => dirs.push(PathBuf::from(d)),
        None => dirs.extend(std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))),
    }
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let sys = if sys.is_empty() { "/usr/local/share:/usr/share".to_owned() } else { sys };
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs.into_iter().map(|d| d.join("applications")).collect()
}

/// The language keys to look for, best first: "de_AT", "de".
fn languages() -> Vec<String> {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty())).unwrap_or_default();
    let base = lang.split(['.', '@']).next().unwrap_or("");
    if base.is_empty() || base == "C" || base == "POSIX" {
        return vec![];
    }
    let mut out = vec![base.to_owned()];
    if let Some((l, _)) = base.split_once('_') {
        out.push(l.to_owned());
    }
    out
}

/// Parses an entry; None if it shouldn't be listed (hidden, not an
/// application, not for this desktop, its program missing).
pub fn parse(id: &str, text: &str, langs: &[String], desktops: &[String]) -> Option<App> {
    let mut app = App { id: id.to_owned(), category: CATEGORIES.len() - 1, ..Default::default() };
    let mut in_entry = false;
    let mut kind = String::new();
    // Rank of the language each localized value came from (lower: better).
    let mut ranks = [usize::MAX; 4];
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        // "Name[de]" -> ("Name", rank of "de").
        let (key, rank) = match k.split_once('[') {
            Some((base, rest)) => match langs.iter().position(|l| rest.trim_end_matches(']') == l) {
                Some(r) => (base, r),
                None => continue,
            },
            None => (k, langs.len()),
        };
        let slot = match key {
            "Name" => Some((0, &mut app.name)),
            "GenericName" => Some((1, &mut app.generic)),
            "Comment" => Some((2, &mut app.comment)),
            "Keywords" => Some((3, &mut app.keywords)),
            _ => None,
        };
        if let Some((i, field)) = slot {
            if rank <= ranks[i] {
                ranks[i] = rank;
                *field = v.to_owned();
            }
            continue;
        }
        match key {
            "Type" => kind = v.to_owned(),
            "Icon" => app.icon = v.to_owned(),
            "Exec" => app.exec = strip_field_codes(v),
            "Terminal" => app.terminal = v == "true",
            "Categories" => app.category = category_of(v),
            "NoDisplay" | "Hidden" if v == "true" => return None,
            "OnlyShowIn" if !v.split(';').any(|d| desktops.iter().any(|x| x == d)) => return None,
            "NotShowIn" if v.split(';').any(|d| desktops.iter().any(|x| x == d)) => return None,
            "TryExec" if !program_exists(v) => return None,
            _ => {}
        }
    }
    if kind != "Application" || app.exec.is_empty() {
        return None;
    }
    if app.name.is_empty() {
        app.name = id.to_owned();
    }
    Some(app)
}

fn program_exists(p: &str) -> bool {
    if p.contains('/') {
        return Path::new(p).is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(p).is_file()))
}

fn strip_field_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            // "%%" is a percent sign; %f %u %i %c %k... are dropped (no
            // files to open).
            if let Some('%') = chars.next() {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every installed app to show, sorted by name.
pub fn all() -> Vec<App> {
    let langs = languages();
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').map(str::to_owned).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for dir in application_dirs() {
        scan(&dir, "", &langs, &desktops, &mut seen, &mut out);
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Reads the entries in `dir` (and below: "kde/okular.desktop" has id
/// "kde-okular"). An id seen before (a more important dir) wins, even
/// when that one is hidden.
fn scan(dir: &Path, prefix: &str, langs: &[String], desktops: &[String], seen: &mut HashSet<String>, out: &mut Vec<App>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if path.is_dir() {
            scan(&path, &format!("{prefix}{name}-"), langs, desktops, seen, out);
            continue;
        }
        let Some(stem) = name.strip_suffix(".desktop") else { continue };
        let id = format!("{prefix}{stem}");
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(app) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&id, &t, langs, desktops)) {
            out.push(app);
        }
    }
}

/// How well `app` matches `query` (lowercase): lower is better, None is no
/// match. The name first (whole, start, a word's start, anywhere), then
/// what it is ("browser"), its keywords and description, its command.
pub fn score(app: &App, query: &str) -> Option<u8> {
    let name = app.name.to_lowercase();
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(query)) {
        return Some(2);
    }
    if name.contains(query) {
        return Some(3);
    }
    if app.generic.to_lowercase().contains(query) || app.keywords.to_lowercase().split(';').any(|k| k.starts_with(query)) {
        return Some(4);
    }
    if app.comment.to_lowercase().contains(query) || app.exec.to_lowercase().split_whitespace().next().is_some_and(|e| e.contains(query)) {
        return Some(5);
    }
    None
}

/// The apps matching `query`, best first (indexes into `apps`).
pub fn search(apps: &[App], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(u8, usize)> = apps.iter().enumerate().filter_map(|(i, a)| score(a, &q).map(|s| (s, i))).collect();
    // Stable: equally good matches stay sorted by name.
    hits.sort_by_key(|&(s, _)| s);
    hits.into_iter().map(|(_, i)| i).collect()
}

/// The terminal to run `Terminal=true` apps in: the configured one, else
/// $TERMINAL, else the first installed of a few common ones.
fn terminal(configured: &str) -> String {
    if !configured.is_empty() {
        return configured.to_owned();
    }
    if let Ok(t) = std::env::var("TERMINAL") {
        if !t.is_empty() {
            return t;
        }
    }
    ["x-terminal-emulator", "foot", "alacritty", "kitty", "wezterm", "xfce4-terminal", "konsole", "xterm"]
        .into_iter()
        .find(|t| program_exists(t))
        .unwrap_or("xterm")
        .to_owned()
}

/// Starts `app` on its own (HeroUI's launch: its own session, not our
/// child, no signals blocked or ignored), so it never depends on the
/// launcher, which exits right after.
pub fn launch(app: &App, terminal_cmd: &str) {
    let cmd = if app.terminal { format!("{} -e {}", terminal(terminal_cmd), app.exec) } else { app.exec.clone() };
    if let Err(e) = heroui::process::launch(&format!("exec {cmd}")) {
        eprintln!("herolauncher: can't start {cmd}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> Option<App> {
        parse("x", text, &["de_AT".into(), "de".into()], &["HeroWM".into()])
    }

    #[test]
    fn parses_entries() {
        let a = app("[Desktop Entry]\nType=Application\nName=Files\nName[de]=Dateien\nGenericName=File Manager\nExec=nautilus %U\nIcon=folder\nKeywords=folder;explorer;\n[Desktop Action new]\nName=Other\n").unwrap();
        assert_eq!((a.name.as_str(), a.exec.as_str(), a.icon.as_str()), ("Dateien", "nautilus", "folder"));
        assert!(app("[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n").is_none());
        assert!(app("[Desktop Entry]\nType=Link\nName=A\nURL=x\n").is_none());
        assert!(app("[Desktop Entry]\nType=Application\nName=A\nExec=a\nOnlyShowIn=KDE;\n").is_none());
        assert!(app("[Desktop Entry]\nType=Application\nName=A\nExec=a\nOnlyShowIn=GNOME;HeroWM;\n").is_some());
        assert!(app("[Desktop Entry]\nType=Application\nName=A\nExec=a\nTryExec=/no/such/program\n").is_none());
        assert_eq!(strip_field_codes("app --x=100%% %U"), "app --x=100%");
        let cat = |c: &str| CATEGORIES[category_of(c)].0;
        assert_eq!((cat("Game;ArcadeGame;"), cat("Network;WebBrowser;"), cat("Settings;System;")), ("Games", "Internet", "Settings"));
        assert_eq!(cat("X-Unknown;"), "Other");
    }

    #[test]
    fn ranks_matches() {
        let mk = |name: &str, generic: &str| App { name: name.into(), generic: generic.into(), exec: name.to_lowercase(), ..Default::default() };
        let apps = vec![mk("Firefox", "Web Browser"), mk("Fire Starter", ""), mk("Browser Tool", ""), mk("Text Editor", "")];
        let names = |q: &str| search(&apps, q).into_iter().map(|i| apps[i].name.as_str()).collect::<Vec<_>>();
        assert_eq!(names("fire"), ["Firefox", "Fire Starter"]);
        assert_eq!(names("browser"), ["Browser Tool", "Firefox"], "name before what it is");
        assert_eq!(names("edit"), ["Text Editor"]);
        assert!(names("zzz").is_empty());
    }
}
