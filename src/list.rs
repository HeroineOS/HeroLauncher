//! The lists under the search field: favorites as a grid of icons with
//! names, then the apps as rows (icon, name, what it is) or as a grid; or,
//! while searching, the matches. One widget drawing every line, with one
//! event handler (cheap however many apps there are). Also the category
//! buttons.

use std::cell::RefCell;
use std::rc::Rc;

use heroui::fltk::app;
use heroui::fltk::app::MouseButton;
use heroui::fltk::draw;
use heroui::fltk::enums::{Align, Event, FrameType};
use heroui::fltk::frame::Frame;
use heroui::fltk::prelude::*;
use heroui::prelude::*;
use heroui::widgets::{mix, repaint};

use crate::apps::CATEGORIES;
use crate::{Launcher, Msg, Part, Shown};

const ROW: i32 = 42;
const HEADER: i32 = 30;
const CELL: (i32, i32) = (92, 86);

type Rect = (i32, i32, i32, i32);

/// What a line shows (copied from the state when it changes).
#[derive(Clone, PartialEq)]
struct Line {
    /// Its index in `Launcher::shown`.
    idx: usize,
    shown: Shown,
    name: String,
    detail: String,
    icon: String,
}

#[derive(Default)]
struct View {
    /// Apps as grid cells too (not just favorites).
    grid: bool,
    /// Favorites side, empty: a hint is shown.
    hint: bool,
    lines: Vec<Line>,
    sel: Option<usize>,
    hover: heroui::hover::HoverFade,
    scroll: i32,
    /// The line pressed (left button), and where the press was.
    pressed: Option<(usize, i32)>,
    /// Touch scrolling: the last pointer y.
    scrolling: Option<i32>,
}

impl View {
    /// Each line's rectangle (in the widget, scrolled): favorites flow in
    /// a grid, the rest are full-width rows.
    fn layout(&self, w: i32) -> Vec<Rect> {
        let cols = (w / CELL.0).max(1);
        let gx = (w - cols * CELL.0) / 2;
        let mut out = Vec::with_capacity(self.lines.len());
        let (mut y, mut col) = (0, 0);
        for l in &self.lines {
            let cell = matches!(l.shown, Shown::Fav(_)) || (self.grid && matches!(l.shown, Shown::Row(_)));
            match l.shown {
                _ if cell => {
                    if col == cols {
                        col = 0;
                        y += CELL.1;
                    }
                    out.push((gx + col * CELL.0, y, CELL.0, CELL.1));
                    col += 1;
                }
                _ => {
                    if col > 0 {
                        y += CELL.1;
                        col = 0;
                    }
                    let h = if matches!(l.shown, Shown::Header(_)) { HEADER } else { ROW };
                    out.push((0, y, w, h));
                    y += h;
                }
            }
        }
        out.into_iter().map(|(x, y, w, h)| (x, y - self.scroll, w, h)).collect()
    }

    fn content_h(&self, w: i32) -> i32 {
        let s = self.scroll;
        self.layout(w).last().map_or(0, |&(_, y, _, h)| y + s + h)
    }

    fn scroll_by(&mut self, w: i32, h: i32, dy: i32) {
        self.scroll = (self.scroll + dy).clamp(0, (self.content_h(w) - h).max(0));
    }

    /// The app line at (px, py), in the widget.
    fn at(&self, w: i32, (px, py): (i32, i32)) -> Option<usize> {
        self.layout(w)
            .iter()
            .position(|&(x, y, cw, ch)| px >= x && px < x + cw && py >= y && py < y + ch)
            .filter(|&i| self.lines[i].shown.app().is_some())
    }

    /// Scrolls so line `i` is in view.
    fn reveal(&mut self, w: i32, h: i32, i: usize) {
        let Some(&(_, y, _, ch)) = self.layout(w).get(i) else { return };
        if y < 0 {
            self.scroll_by(w, h, y - 4);
        } else if y + ch > h {
            self.scroll_by(w, h, y + ch - h + 4);
        }
    }
}

fn paint(v: &View, x: i32, y: i32, w: i32, h: i32) {
    let t = heroui::theme::current();
    draw::push_clip(x, y, w, h);
    let r = t.radius.min(10);
    for (i, (l, (lx, ly, lw, lh))) in v.lines.iter().zip(v.layout(w)).enumerate() {
        if ly + lh < 0 || ly > h {
            continue;
        }
        let (lx, ly) = (x + lx, y + ly);
        let selected = v.sel == Some(l.idx);
        let a = v.hover.amount(i);
        if selected {
            draw::set_draw_color(mix(t.surface_alt, t.accent, 0.35));
            draw::draw_rounded_rectf(lx, ly, lw, lh, r);
        } else if a > 0.0 {
            draw::set_draw_color(mix(t.background, t.surface_alt, a));
            draw::draw_rounded_rectf(lx, ly, lw, lh, r);
        }
        let app_icon = |ix: i32, iy: i32, s: i32| {
            if !heroui::icons::draw(&l.icon, ix, iy, s, t.text) {
                heroui::icons::draw("app", ix, iy, s, t.text);
            }
        };
        match l.shown {
            Shown::Header(text) => {
                draw::set_font(t.bold_font(), t.font_size - 1);
                draw::set_draw_color(t.text_dim);
                draw::draw_text2(text, lx + 6, ly, lw - 12, lh, Align::Left | Align::Inside);
            }
            Shown::Fav(_) | Shown::Row(_) if lw < w => {
                let s = 40;
                app_icon(lx + (lw - s) / 2, ly + 10, s);
                draw::set_font(t.font(), (t.font_size - 2).max(9));
                draw::set_draw_color(t.text);
                draw::draw_text2(&fit(&l.name, lw - 8), lx, ly + 10 + s + 6, lw, t.font_size, Align::Center | Align::Inside);
            }
            Shown::Row(_) | Shown::Fav(_) => {
                let s = 28;
                app_icon(lx + 8, ly + (lh - s) / 2, s);
                let tx = lx + 8 + s + 12;
                draw::set_font(t.font(), t.font_size);
                draw::set_draw_color(t.text);
                let nw = draw::width(&l.name).ceil() as i32;
                draw::draw_text2(&l.name, tx, ly, lw - (tx - lx) - 8, lh, Align::Left | Align::Inside | Align::Clip);
                // What it is, dim, after the name while there's room.
                let dx = tx + nw + 10;
                let room = lx + lw - 8 - dx;
                if !l.detail.is_empty() && room > 40 {
                    draw::set_font(t.font(), t.font_size - 2);
                    draw::set_draw_color(t.text_dim);
                    draw::draw_text2(&fit(&l.detail, room), dx, ly, room, lh, Align::Left | Align::Inside);
                }
            }
        }
    }
    if v.hint && v.lines.is_empty() {
        draw::set_font(t.font(), t.font_size - 1);
        draw::set_draw_color(t.text_dim);
        draw::draw_text2("Right-click an app to\nadd it here", x, y + 8, w, 40, Align::Center | Align::Top);
    }
    draw::pop_clip();
    // A thin scrollbar while there's more than fits.
    let total = v.content_h(w);
    if total > h {
        let bh = (h * h / total).max(24);
        let by = y + (h - bh) * v.scroll / (total - h).max(1);
        draw::set_draw_color(mix(t.background, t.text_dim, 0.5));
        draw::draw_rounded_rectf(x + w - 4, by, 3, bh, 1);
    }
}

/// `text` cut to fit `w` px (font set), with an ellipsis.
fn fit(text: &str, w: i32) -> String {
    if draw::width(text) as i32 <= w {
        return text.to_owned();
    }
    let mut s = text.to_owned();
    while !s.is_empty() {
        s.pop();
        let t = format!("{}…", s.trim_end());
        if draw::width(&t) as i32 <= w {
            return t;
        }
    }
    String::new()
}

/// The lines of `part`; `grid`: apps as icon cells too.
pub fn view(part: Part, grid: bool) -> Element<Launcher, Msg> {
    Element::new(move |ctx| {
        let v: Rc<RefCell<View>> = Rc::new(RefCell::new(View { grid, hint: part == Part::Favs, ..Default::default() }));
        let mut f = Frame::default();
        f.set_frame(FrameType::NoBox);
        {
            let v = v.clone();
            f.draw(move |f| {
                if let Ok(v) = v.try_borrow() {
                    paint(&v, f.x(), f.y(), f.w(), f.h());
                }
            });
        }
        let emit = ctx.emitter();
        {
            let v = v.clone();
            // No borrow across FLTK calls (they can run the event loop).
            f.handle(move |f, ev| {
                let p = (app::event_x() - f.x(), app::event_y() - f.y());
                let me = f.as_base_widget();
                match ev {
                    Event::Enter | Event::Move => {
                        let h = v.borrow().at(f.w(), p);
                        v.borrow_mut().hover.set(h, &me);
                        true
                    }
                    Event::Leave => {
                        v.borrow_mut().hover.set(None, &me);
                        true
                    }
                    Event::MouseWheel => {
                        let dy = match app::event_dy() {
                            app::MouseWheel::Down => 3 * ROW,
                            app::MouseWheel::Up => -3 * ROW,
                            _ => 0,
                        };
                        v.borrow_mut().scroll_by(f.w(), f.h(), dy);
                        repaint(&mut me.clone());
                        true
                    }
                    Event::Push => {
                        let (hit, rect) = {
                            let s = v.borrow();
                            let hit = s.at(f.w(), p);
                            (hit, hit.and_then(|i| s.layout(f.w()).get(i).copied()))
                        };
                        if app::event_mouse_button() == MouseButton::Right {
                            if let (Some(i), Some(mut rect)) = (hit, rect) {
                                let app_i = v.borrow().lines[i].shown.app();
                                // A row's menu drops down at the pointer.
                                if matches!(v.borrow().lines[i].shown, Shown::Row(_)) {
                                    rect = (p.0 - 1, rect.1, 2, rect.3);
                                }
                                if let Some(a) = app_i {
                                    emit(Msg::OpenMenuAt(a, rect, part));
                                }
                            }
                            return true;
                        }
                        let mut s = v.borrow_mut();
                        s.pressed = hit.map(|i| (i, p.1));
                        s.scrolling = None;
                        if hit.is_none() {
                            s.scrolling = Some(p.1);
                        }
                        true
                    }
                    Event::Drag => {
                        let mut s = v.borrow_mut();
                        if let Some(y0) = s.scrolling {
                            s.scroll_by(f.w(), f.h(), y0 - p.1);
                            s.scrolling = Some(p.1);
                            drop(s);
                            repaint(&mut me.clone());
                        } else if let Some((_, y0)) = s.pressed {
                            // Moved: a scroll (touch), not a click.
                            if (p.1 - y0).abs() > 8 {
                                s.pressed = None;
                                s.scrolling = Some(p.1);
                            }
                        }
                        true
                    }
                    Event::Released => {
                        let click = {
                            let mut s = v.borrow_mut();
                            s.scrolling = None;
                            s.pressed.take().filter(|&(i, _)| s.at(f.w(), p) == Some(i)).map(|(i, _)| i)
                        };
                        if let Some(i) = click {
                            let idx = v.borrow().lines[i].idx;
                            emit(Msg::Launch(idx));
                        }
                        true
                    }
                    _ => false,
                }
            });
        }
        let mut w = f.as_base_widget();
        ctx.bind(move |l: &Launcher| {
            let lines: Vec<Line> = l
                .shown
                .iter()
                .enumerate()
                .filter(|(_, s)| match part {
                    Part::Main => true,
                    Part::Favs => matches!(s, Shown::Fav(_)),
                    Part::Apps => !matches!(s, Shown::Fav(_)),
                })
                .map(|(idx, &shown)| match shown.app().and_then(|i| l.apps.get(i)) {
                    Some(a) => Line {
                        idx,
                        shown,
                        name: a.name.clone(),
                        detail: [&a.generic, &a.comment].into_iter().find(|d| !d.is_empty() && **d != a.name).cloned().unwrap_or_default(),
                        icon: a.icon.clone(),
                    },
                    None => Line { idx, shown, name: String::new(), detail: String::new(), icon: String::new() },
                })
                .collect();
            let mut s = v.borrow_mut();
            if s.lines == lines && s.sel == l.sel {
                return;
            }
            if s.lines != lines {
                s.lines = lines;
                s.scroll = 0;
                s.hover.clear();
            }
            s.sel = l.sel;
            if let Some(i) = l.sel.and_then(|sel| s.lines.iter().position(|x| x.idx == sel)) {
                s.reveal(w.w(), w.h(), i);
            }
            drop(s);
            repaint(&mut w);
        });
        f.as_base_widget()
    })
}

/// A thin vertical line (between the split layout's sides).
pub fn divider() -> Element<Launcher, Msg> {
    canvas(|_: &Launcher| (), |_: &(), x, y, _w, h, t: &Theme| {
        draw::set_draw_color(t.border);
        draw::draw_line(x, y + 4, x, y + h - 4);
    })
}

#[derive(Default)]
struct Chips {
    /// Category indexes shown after "All".
    cats: Vec<usize>,
    selected: Option<usize>,
    hover: heroui::hover::HoverFade,
    scroll: i32,
}

impl Chips {
    /// (category or None for "All", x, width), scrolled.
    fn layout(&self) -> Vec<(Option<usize>, i32, i32)> {
        let t = heroui::theme::current();
        draw::set_font(t.font(), t.font_size - 1);
        let mut x = 0;
        std::iter::once(None)
            .chain(self.cats.iter().map(|&c| Some(c)))
            .map(|c| {
                let w = draw::width(c.map_or("All", |c| CATEGORIES[c].0)).ceil() as i32 + 24;
                let r = (c, x - self.scroll, w);
                x += w + 6;
                r
            })
            .collect()
    }

    fn width(&self) -> i32 {
        let s = self.scroll;
        self.layout().last().map_or(0, |&(_, x, w)| x + s + w)
    }
}

/// The category buttons ("All", "Games", "Internet"...): one shows only
/// its apps. Scrolls sideways (wheel) when they don't all fit.
pub fn categories() -> Element<Launcher, Msg> {
    Element::new(|ctx| {
        let v: Rc<RefCell<Chips>> = Rc::default();
        let mut f = Frame::default();
        f.set_frame(FrameType::NoBox);
        {
            let v = v.clone();
            f.draw(move |f| {
                let Ok(v) = v.try_borrow() else { return };
                let t = heroui::theme::current();
                draw::push_clip(f.x(), f.y(), f.w(), f.h());
                for (k, (c, x, w)) in v.layout().into_iter().enumerate() {
                    let on = c == v.selected;
                    let a = v.hover.amount(k);
                    let bg = if on { t.accent } else { mix(t.surface_alt, t.accent, 0.25 * a) };
                    draw::set_draw_color(bg);
                    let (cx, cy, ch) = (f.x() + x, f.y() + 2, f.h() - 4);
                    draw::draw_rounded_rectf(cx, cy, w, ch, t.radius.min(ch / 2));
                    draw::set_font(t.font(), t.font_size - 1);
                    draw::set_draw_color(if on { t.accent_text } else { t.text });
                    draw::draw_text2(c.map_or("All", |c| CATEGORIES[c].0), cx, cy, w, ch, Align::Center);
                }
                draw::pop_clip();
            });
        }
        let emit = ctx.emitter();
        {
            let v = v.clone();
            f.handle(move |f, ev| {
                let px = app::event_x() - f.x();
                let me = f.as_base_widget();
                let at = |v: &Chips| v.layout().into_iter().position(|(_, x, w)| px >= x && px < x + w);
                match ev {
                    Event::Enter | Event::Move => {
                        let h = at(&v.borrow());
                        v.borrow_mut().hover.set(h, &me);
                        true
                    }
                    Event::Leave => {
                        v.borrow_mut().hover.set(None, &me);
                        true
                    }
                    Event::MouseWheel => {
                        let d = match app::event_dy() {
                            app::MouseWheel::Down => 60,
                            app::MouseWheel::Up => -60,
                            _ => 0,
                        } + match app::event_dx() {
                            app::MouseWheel::Right => 60,
                            app::MouseWheel::Left => -60,
                            _ => 0,
                        };
                        let mut s = v.borrow_mut();
                        let max = (s.width() - f.w()).max(0);
                        s.scroll = (s.scroll + d).clamp(0, max);
                        drop(s);
                        repaint(&mut me.clone());
                        true
                    }
                    Event::Push => true,
                    Event::Released => {
                        let pick = {
                            let s = v.borrow();
                            at(&s).map(|k| s.layout()[k].0)
                        };
                        if let Some(c) = pick {
                            emit(Msg::Category(c));
                        }
                        true
                    }
                    _ => false,
                }
            });
        }
        let mut w = f.as_base_widget();
        ctx.bind(move |l: &Launcher| {
            let mut s = v.borrow_mut();
            if s.cats != l.cats || s.selected != l.category {
                s.cats = l.cats.clone();
                s.selected = l.category;
                // Keep the chosen one in view.
                let lay = s.layout();
                if let Some(&(_, x, cw)) = lay.iter().find(|(c, _, _)| *c == l.category) {
                    if x < 0 {
                        s.scroll += x;
                    } else if x + cw > w.w() {
                        s.scroll += x + cw - w.w();
                    }
                }
                drop(s);
                repaint(&mut w);
            }
        });
        f.as_base_widget()
    })
}

/// A small dim bold label above a section.
pub fn section(text: &'static str) -> Element<Launcher, Msg> {
    canvas(|_: &Launcher| (), move |_: &(), x, y, w, h, t: &Theme| {
        draw::set_font(t.bold_font(), t.font_size - 1);
        draw::set_draw_color(t.text_dim);
        draw::draw_text2(text, x + 6, y, w - 12, h, Align::Left | Align::Inside);
    })
}
