//! HeroLauncher: find an installed app and start it.
//!
//! `herolauncher` opens it in the middle of the screen; `herolauncher
//! --menu --edge top --x 40 --offset 38` opens it as a menu at a panel
//! button (HeroBar's launcher module does that). Running it while it's
//! open closes it, so one keyboard shortcut toggles it. It runs only while
//! it's open: no memory used otherwise.
//!
//! On Wayland it's a see-through layer-shell overlay over the whole screen
//! that takes the keyboard (type right away); the panel is drawn where it
//! belongs and a click anywhere else closes it. On X11 it's a borderless
//! window that grabs the pointer, for the same effect.

mod apps;
mod config;
mod list;

use std::cell::Cell;
use std::rc::Rc;

use heroui::fltk::app as fapp;
use heroui::fltk::draw;
use heroui::fltk::enums::{Event, FrameType, Key};
use heroui::fltk::group::Group;
use heroui::fltk::prelude::*;
use heroui::prelude::*;

use apps::App as AppInfo;
use config::Config;

/// Where the panel goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    Center,
    /// Under (top edge) or over (bottom edge) a panel button: its left
    /// edge at `x`, `offset` px from that screen edge.
    Menu { bottom: bool, x: i32, offset: i32 },
}

/// The panel's rectangle in a `w`×`h` screen.
fn panel_rect(place: Place, (pw, ph): (i32, i32), w: i32, h: i32) -> (i32, i32, i32, i32) {
    const MARGIN: i32 = 8;
    let pw = pw.min(w - 2 * MARGIN).max(1);
    let ph = ph.min(h - 2 * MARGIN).max(1);
    match place {
        Place::Center => ((w - pw) / 2, (h - ph) / 2, pw, ph),
        Place::Menu { bottom, x, offset } => {
            let x = x.clamp(MARGIN, (w - pw - MARGIN).max(MARGIN));
            let y = if bottom { h - offset - ph } else { offset };
            (x, y.clamp(MARGIN.min(offset), (h - ph).max(0)), pw, ph)
        }
    }
}

/// A line of the list: a heading, a favorite (in the grid), an app row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    Header(&'static str),
    Fav(usize),
    Row(usize),
}

impl Shown {
    pub fn app(self) -> Option<usize> {
        match self {
            Shown::Fav(i) | Shown::Row(i) => Some(i),
            Shown::Header(_) => None,
        }
    }
}

/// An app's right-click menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub app: usize,
    /// Where it drops down from, in the list widget.
    pub rect: (i32, i32, i32, i32),
    pub labels: Vec<&'static str>,
    pub acts: Vec<MenuAct>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuAct {
    Launch,
    AddFav,
    RemoveFav,
    /// Move the favorite earlier (-1) or later (1).
    MoveFav(i32),
}

#[derive(Clone, Debug)]
pub enum Msg {
    Query(String),
    /// Enter: start the selected (or best) match.
    Submit,
    /// Up/Down.
    Step(i32),
    /// Start the app at this line.
    Launch(usize),
    /// Right-click on an app (its index), at this rectangle of the list.
    OpenMenuAt(usize, (i32, i32, i32, i32)),
    CloseMenu,
    MenuPick(usize),
    Close,
}

pub struct Launcher {
    pub apps: Vec<AppInfo>,
    pub cfg: Config,
    pub query: String,
    pub shown: Vec<Shown>,
    /// The selected line (keyboard), if any.
    pub sel: Option<usize>,
    pub menu: Option<Menu>,
    pub place: Place,
    /// The app index of each favorite that's installed, in order.
    fav_apps: Vec<usize>,
}

impl Launcher {
    fn new(apps: Vec<AppInfo>, cfg: Config, place: Place) -> Launcher {
        let mut l = Launcher { apps, cfg, query: String::new(), shown: vec![], sel: None, menu: None, place, fav_apps: vec![] };
        l.refresh();
        l
    }

    /// Rebuilds the list for the query: favorites and all apps, or the
    /// matches (the best one selected, for Enter).
    fn refresh(&mut self) {
        self.fav_apps = self.cfg.favorites.iter().filter_map(|id| self.apps.iter().position(|a| a.id == *id)).collect();
        self.shown.clear();
        if self.query.trim().is_empty() {
            if !self.fav_apps.is_empty() {
                self.shown.push(Shown::Header("Favorites"));
                self.shown.extend(self.fav_apps.iter().map(|&i| Shown::Fav(i)));
                self.shown.push(Shown::Header("All apps"));
            }
            self.shown.extend((0..self.apps.len()).map(Shown::Row));
            self.sel = None;
        } else {
            self.shown.extend(apps::search(&self.apps, &self.query).into_iter().map(Shown::Row));
            self.sel = self.shown.iter().position(|s| s.app().is_some());
        }
    }

    fn launch(&self, app: usize) -> Task<Msg> {
        if let Some(a) = self.apps.get(app) {
            apps::launch(a, &self.cfg.terminal);
        }
        Task::quit()
    }

    fn is_fav(&self, app: usize) -> bool {
        self.apps.get(app).is_some_and(|a| self.cfg.favorites.contains(&a.id))
    }

    pub fn menu_for(&self, app: usize, rect: (i32, i32, i32, i32)) -> Menu {
        let mut m: Vec<(&'static str, MenuAct)> = vec![("Open", MenuAct::Launch)];
        if self.is_fav(app) {
            let pos = self.fav_apps.iter().position(|&i| i == app).unwrap_or(0);
            if pos > 0 {
                m.push(("Move earlier", MenuAct::MoveFav(-1)));
            }
            if pos + 1 < self.fav_apps.len() {
                m.push(("Move later", MenuAct::MoveFav(1)));
            }
            m.push(("Remove from favorites", MenuAct::RemoveFav));
        } else {
            m.push(("Add to favorites", MenuAct::AddFav));
        }
        let (labels, acts) = m.into_iter().unzip();
        Menu { app, rect, labels, acts }
    }

    fn edit_favorites(&mut self, app: usize, act: MenuAct) {
        let Some(id) = self.apps.get(app).map(|a| a.id.clone()) else { return };
        let favs = &mut self.cfg.favorites;
        match act {
            MenuAct::AddFav => favs.push(id),
            MenuAct::RemoveFav => favs.retain(|f| *f != id),
            MenuAct::MoveFav(d) => {
                // Among the installed favorites (others keep their places).
                let Some(k) = self.fav_apps.iter().position(|&i| i == app) else { return };
                let Some(&other) = self.fav_apps.get((k as i32 + d) as usize) else { return };
                let other = self.apps[other].id.clone();
                let (Some(a), Some(b)) = (favs.iter().position(|f| *f == id), favs.iter().position(|f| *f == other)) else { return };
                favs.swap(a, b);
            }
            MenuAct::Launch => return,
        }
        if let Err(e) = config::save_favorites(&self.cfg.favorites) {
            eprintln!("herolauncher: {e}");
        }
        let sel_app = self.sel.and_then(|s| self.shown.get(s)).and_then(|s| s.app());
        self.refresh();
        if sel_app.is_some() {
            self.sel = self.shown.iter().position(|s| s.app() == sel_app);
        }
    }
}

impl heroui::App for Launcher {
    type Message = Msg;

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Query(q) => {
                self.query = q;
                self.menu = None;
                self.refresh();
            }
            Msg::Submit => {
                let line = self.sel.or_else(|| (!self.query.trim().is_empty()).then(|| self.shown.iter().position(|s| s.app().is_some())).flatten());
                if let Some(app) = line.and_then(|l| self.shown.get(l)).and_then(|s| s.app()) {
                    return self.launch(app);
                }
            }
            Msg::Step(d) => {
                let selectable: Vec<usize> = (0..self.shown.len()).filter(|&i| self.shown[i].app().is_some()).collect();
                if selectable.is_empty() {
                    return Task::none();
                }
                let cur = self.sel.and_then(|s| selectable.iter().position(|&i| i == s));
                let next = match cur {
                    None if d > 0 => 0,
                    None => selectable.len() - 1,
                    Some(c) => (c as i32 + d).clamp(0, selectable.len() as i32 - 1) as usize,
                };
                self.sel = Some(selectable[next]);
            }
            Msg::Launch(line) => {
                if let Some(app) = self.shown.get(line).and_then(|s| s.app()) {
                    return self.launch(app);
                }
            }
            Msg::OpenMenuAt(app, rect) => self.menu = Some(self.menu_for(app, rect)),
            Msg::CloseMenu => self.menu = None,
            Msg::MenuPick(k) => {
                let Some(m) = self.menu.take() else { return Task::none() };
                match m.acts.get(k) {
                    Some(MenuAct::Launch) => return self.launch(m.app),
                    Some(&act) => self.edit_favorites(m.app, act),
                    None => {}
                }
            }
            Msg::Close => return Task::quit(),
        }
        Task::none()
    }

    fn view(&self) -> Element<Self, Msg> {
        let panel = column(vec![
            row(vec![icon(|_: &Launcher| "search".to_string(), 18).fixed(28), focused(text_input_submit(|l: &Launcher| l.query.clone(), Msg::Query, Msg::Submit))])
                .fixed(38),
            popover_at(
                list::view(),
                |l: &Launcher| l.menu.as_ref().map(|m| m.rect),
                |l: &Launcher| l.menu.is_some(),
                Msg::CloseMenu,
                menu_size,
                menu_view(),
            ),
        ])
        .padding(12)
        .spacing(8);
        overlay(panel, self.place, (self.cfg.width, self.cfg.height))
    }
}

/// `input`, focused once the window shows (type right away).
fn focused(input: Element<Launcher, Msg>) -> Element<Launcher, Msg> {
    Element::new(move |ctx| {
        let w = input.build(ctx);
        let mut w2 = w.clone();
        fapp::add_timeout3(0.0, move |_| {
            let _ = w2.take_focus();
        });
        w
    })
}

/// The whole window: `panel` at its place, its background drawn here. A
/// click outside it or Escape closes the launcher; Up/Down move the
/// selection while typing.
fn overlay(panel: Element<Launcher, Msg>, place: Place, size: (i32, i32)) -> Element<Launcher, Msg> {
    Element::new(move |ctx| {
        let mut g = Group::default();
        g.set_frame(FrameType::NoBox);
        let mut child = panel.build(ctx);
        g.end();
        let rect = Rc::new(Cell::new((0, 0, 1, 1)));
        // Full-screen overlay (Wayland): the panel at its place. Else the
        // window is the panel.
        let full = heroui::is_layer();
        {
            let rect = rect.clone();
            g.resize_callback(move |_, x, y, w, h| {
                let r = if full { panel_rect(place, size, w, h) } else { (0, 0, w, h) };
                rect.set(r);
                child.resize(x + r.0, y + r.1, r.2, r.3);
            });
        }
        {
            let rect = rect.clone();
            g.draw(move |g| {
                let t = heroui::theme::current();
                let (x, y, w, h) = rect.get();
                let (x, y) = (g.x() + x, g.y() + y);
                let r = if heroui::is_transparent() { t.radius.min(14) } else { 0 };
                draw::set_draw_color(t.border);
                draw::draw_rounded_rectf(x, y, w, h, r);
                draw::set_draw_color(t.background);
                draw::draw_rounded_rectf(x + 1, y + 1, w - 2, h - 2, (r - 1).max(0));
                g.draw_children();
            });
        }
        let emit = ctx.emitter();
        g.handle(move |g, ev| match ev {
            Event::Push => {
                let (x, y, w, h) = rect.get();
                let (px, py) = (fapp::event_x() - g.x(), fapp::event_y() - g.y());
                if px < x || py < y || px >= x + w || py >= y + h {
                    emit(Msg::Close);
                    return true;
                }
                false
            }
            Event::KeyDown | Event::Shortcut => match fapp::event_key() {
                Key::Escape => {
                    emit(Msg::Close);
                    true
                }
                Key::Up => {
                    emit(Msg::Step(-1));
                    true
                }
                Key::Down => {
                    emit(Msg::Step(1));
                    true
                }
                _ => false,
            },
            _ => false,
        });
        g.as_base_widget()
    })
}

const MENU_ROW: i32 = 32;

fn menu_size(l: &Launcher) -> (i32, i32) {
    let n = l.menu.as_ref().map_or(1, |m| m.labels.len()) as i32;
    let t = heroui::theme::current();
    draw::set_font(t.font(), t.font_size);
    let w = l.menu.as_ref().map_or(100, |m| m.labels.iter().map(|s| draw::width(s).ceil() as i32).max().unwrap_or(100)) + 40;
    (w.max(180), 12 + n * MENU_ROW + (n - 1) * 2)
}

/// The open menu's items.
fn menu_view() -> Element<Launcher, Msg> {
    column(vec![list(
        |l: &Launcher| l.menu.as_ref().map_or(0, |m| m.labels.len()),
        |k| {
            Element::new(move |ctx| {
                let label: Rc<std::cell::RefCell<&'static str>> = Rc::new(std::cell::RefCell::new(""));
                let mut b = custom_button({
                    let label = label.clone();
                    move |b| {
                        let t = heroui::theme::current();
                        let a = if b.value() { 1.0 } else { heroui::hover::hover_amount(b) };
                        if a > 0.0 {
                            draw::set_draw_color(heroui::widgets::mix(t.background, t.surface_alt, a));
                            draw::draw_rounded_rectf(b.x(), b.y(), b.w(), b.h(), t.radius.min(8));
                        }
                        draw::set_font(t.font(), t.font_size);
                        draw::set_draw_color(t.text);
                        draw::draw_text2(&label.borrow(), b.x() + 12, b.y(), b.w() - 12, b.h(), heroui::fltk::enums::Align::Left | heroui::fltk::enums::Align::Inside);
                    }
                });
                let emit = ctx.emitter();
                b.set_callback(move |_| emit(Msg::MenuPick(k)));
                let mut w = b.clone();
                ctx.bind(move |l: &Launcher| {
                    let s = l.menu.as_ref().and_then(|m| m.labels.get(k).copied()).unwrap_or("");
                    if *label.borrow() != s {
                        *label.borrow_mut() = s;
                        heroui::widgets::repaint(&mut w);
                    }
                });
                b.as_base_widget()
            })
            .fixed(MENU_ROW)
        },
    )
    .spacing(2)])
    .padding(6)
}

fn usage() -> ! {
    eprintln!(
        "usage: herolauncher [--menu --edge top|bottom --x X --offset N]\n\
         \n  Opens the launcher in the middle of the screen, or (--menu) as a menu at a\n  \
         panel button: its left edge at X, N px from the top or bottom edge.\n  \
         Run it again to close it."
    );
    std::process::exit(2)
}

fn parse_args() -> Place {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut menu = false;
    let (mut bottom, mut x, mut offset) = (false, 0, 0);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = || it.next().and_then(|v| v.parse::<i32>().ok()).unwrap_or_else(|| usage());
        match a.as_str() {
            "--menu" => menu = true,
            "--center" => menu = false,
            "--x" => x = num(),
            "--offset" => offset = num(),
            "--edge" => match it.next().map(String::as_str) {
                Some("top") => bottom = false,
                Some("bottom") => bottom = true,
                _ => usage(),
            },
            "--print-default-config" => {
                print!("{}", config::DEFAULT);
                std::process::exit(0)
            }
            _ => usage(),
        }
    }
    if menu {
        Place::Menu { bottom, x, offset }
    } else {
        Place::Center
    }
}

/// Where the running launcher's process id is kept.
fn pid_file() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("herolauncher.pid")
}

/// If a launcher is open, closes it and returns true (this run toggles it
/// off). Otherwise records this one.
fn toggle_off() -> bool {
    let file = pid_file();
    if let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse::<i32>().ok()) {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if pid as u32 != std::process::id() && comm.trim() == "herolauncher" {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            return true;
        }
    }
    let _ = std::fs::write(&file, std::process::id().to_string());
    false
}

fn main() {
    let place = parse_args();
    if toggle_off() {
        return;
    }
    let cfg = config::load();
    let apps = apps::all();
    let mut s = Settings::new("Launcher");
    s.class = Some("herolauncher".into());
    s.kind = WindowKind::Overlay;
    s.transparent = true;
    s.decorated = false;
    s.size = (cfg.width, cfg.height);
    // X11 (no layer-shell): the window is the panel, placed like it.
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        let (sx, sy, sw, sh) = fapp::screen_xywh(0);
        let (x, y, w, h) = panel_rect(place, s.size, sw, sh);
        s.position = Some((sx + x, sy + y));
        s.size = (w, h);
    }
    let r = heroui::run(Launcher::new(apps, cfg, place), s);
    let _ = std::fs::remove_file(pid_file());
    if let Err(e) = r {
        eprintln!("herolauncher: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_placement() {
        assert_eq!(panel_rect(Place::Center, (400, 300), 1000, 800), (300, 250, 400, 300));
        // Under a button near the right edge: kept on the screen.
        assert_eq!(panel_rect(Place::Menu { bottom: false, x: 900, offset: 38 }, (400, 300), 1000, 800), (592, 38, 400, 300));
        // Over a button of a bottom bar.
        assert_eq!(panel_rect(Place::Menu { bottom: true, x: 0, offset: 38 }, (400, 300), 1000, 800), (8, 462, 400, 300));
    }

    #[test]
    fn query_and_favorites() {
        let mk = |id: &str| AppInfo { id: id.into(), name: id.into(), exec: id.into(), ..Default::default() };
        let cfg = Config { favorites: vec!["b".into(), "gone".into()], ..Default::default() };
        let mut l = Launcher::new(vec![mk("a"), mk("b"), mk("c")], cfg, Place::Center);
        assert_eq!(l.shown[..3], [Shown::Header("Favorites"), Shown::Fav(1), Shown::Header("All apps")]);
        assert_eq!(l.sel, None);
        l.query = "c".into();
        l.refresh();
        assert_eq!((l.shown.as_slice(), l.sel), (&[Shown::Row(2)][..], Some(0)));
        let m = l.menu_for(0, (0, 0, 1, 1));
        assert_eq!(m.labels, ["Open", "Add to favorites"]);
    }
}
