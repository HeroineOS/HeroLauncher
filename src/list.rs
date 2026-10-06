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
/// The selection highlight's glide: quick, a hint of bounce.
const SEL_SPRING: heroui::anim::Spring = heroui::anim::Spring { response: 0.24, damping: 0.82 };

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
    /// Scrolled by (glides, flicks).
    scroll: heroui::anim::Scroller,
    /// The selection highlight (x, y, w, h in content coordinates): it
    /// glides from line to line.
    sel_at: [heroui::anim::Tween; 4],
    sel_shown: bool,
    /// The widget, for repainting while things move.
    me: Option<heroui::fltk::widget::Widget>,
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
        let s = self.scroll.pos();
        out.into_iter().map(|(x, y, w, h)| (x, y - s, w, h)).collect()
    }

    fn content_h(&self, w: i32) -> i32 {
        let s = self.scroll.pos();
        self.layout(w).last().map_or(0, |&(_, y, _, h)| y + s + h)
    }

    fn max_scroll(&self, w: i32, h: i32) -> f64 {
        (self.content_h(w) - h).max(0) as f64
    }

    fn redraw(&self) -> impl FnMut() + 'static {
        let me = self.me.clone();
        move || {
            if let Some(mut m) = me.clone() {
                if !m.was_deleted() {
                    repaint(&mut m);
                }
            }
        }
    }

    /// Moves the selection highlight to line `i` (gliding, or at once).
    fn aim_sel(&mut self, w: i32, i: Option<usize>, glide: bool) {
        let rect = i.and_then(|i| self.layout(w).get(i).copied());
        let Some((x, y, cw, ch)) = rect else {
            self.sel_shown = false;
            return;
        };
        let target = [x as f64, (y + self.scroll.pos()) as f64, cw as f64, ch as f64];
        let glide = glide && self.sel_shown;
        for (t, v) in self.sel_at.iter().zip(target) {
            if glide {
                t.spring_to(v, SEL_SPRING, self.redraw());
            } else {
                t.set(v);
            }
        }
        self.sel_shown = true;
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
        // Relative to where the scroll is heading, so key repeats add up.
        let y = y + self.scroll.pos() - self.scroll.target().round() as i32;
        let target = self.scroll.target();
        let max = self.max_scroll(w, h);
        if y < 0 {
            self.scroll.scroll_to(target + (y - 4) as f64, max, self.redraw());
        } else if y + ch > h {
            self.scroll.scroll_to(target + (y + ch - h + 4) as f64, max, self.redraw());
        }
    }
}

fn paint(v: &View, x: i32, y: i32, w: i32, h: i32) {
    let t = heroui::theme::current();
    draw::push_clip(x, y, w, h);
    let r = t.radius.min(10);
    let layout = v.layout(w);
    // Hover backgrounds, then the gliding selection over them.
    for (i, &(lx, ly, lw, lh)) in layout.iter().enumerate() {
        let a = v.hover.amount(i);
        if a > 0.0 && v.lines.get(i).is_some_and(|l| v.sel != Some(l.idx)) && ly + lh >= 0 && ly <= h {
            draw::set_draw_color(mix(t.background, t.surface_alt, a));
            draw::draw_rounded_rectf(x + lx, y + ly, lw, lh, r);
        }
    }
    if v.sel_shown && v.sel.is_some() {
        // At rest it sits on its line (the layout may have changed size).
        if let Some(&(lx, ly, lw, lh)) = v.lines.iter().position(|l| v.sel == Some(l.idx)).and_then(|i| layout.get(i)) {
            let real = [lx as f64, (ly + v.scroll.pos()) as f64, lw as f64, lh as f64];
            for (t, r) in v.sel_at.iter().zip(real) {
                if t.velocity() == 0.0 && t.target() == t.get() && (t.get() - r).abs() > 0.5 {
                    t.set(r);
                }
            }
        }
        let [sx, sy, sw, sh] = v.sel_at.each_ref().map(|t| t.get());
        let sy = sy - v.scroll.pos() as f64;
        heroui::fx::fill_rounded(x as f64 + sx, y as f64 + sy, sw, sh, r as f64, mix(t.surface_alt, t.accent, 0.35), 1.0);
    }
    for (l, (lx, ly, lw, lh)) in v.lines.iter().zip(layout) {
        if ly + lh < 0 || ly > h {
            continue;
        }
        let (lx, ly) = (x + lx, y + ly);
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
        let by = y + (h - bh) * v.scroll.pos() / (total - h).max(1);
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
        v.borrow_mut().me = Some(f.as_base_widget());
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
                        let s = v.borrow();
                        s.scroll.wheel(dy as f64, s.max_scroll(f.w(), f.h()), s.redraw());
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
                        s.scroll.press();
                        if hit.is_none() {
                            s.scrolling = Some(p.1);
                        }
                        true
                    }
                    Event::Drag => {
                        let mut s = v.borrow_mut();
                        if let Some(y0) = s.scrolling {
                            let max = s.max_scroll(f.w(), f.h());
                            s.scroll.drag_to((s.scroll.pos() + y0 - p.1) as f64, max);
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
                            if s.scrolling.take().is_some() {
                                // A flick keeps it going.
                                s.scroll.release(s.max_scroll(f.w(), f.h()), s.redraw());
                            }
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
            // The highlight glides when only the selection moved; new
            // results place it at once.
            let glide = s.lines == lines;
            if !glide {
                s.lines = lines;
                s.scroll.set(0.0);
                s.hover.clear();
            }
            s.sel = l.sel;
            let i = l.sel.and_then(|sel| s.lines.iter().position(|x| x.idx == sel));
            if let Some(i) = i {
                s.reveal(w.w(), w.h(), i);
            }
            s.aim_sel(w.w(), i, glide);
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
    /// Scrolled sideways by (glides).
    scroll: heroui::anim::Scroller,
    /// The chosen chip's highlight: its left edge and width (unscrolled),
    /// sliding from chip to chip.
    pill: [heroui::anim::Tween; 2],
    placed: bool,
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
                let r = (c, x - self.scroll.pos(), w);
                x += w + 6;
                r
            })
            .collect()
    }

    fn width(&self) -> i32 {
        let s = self.scroll.pos();
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
                let (cy, ch) = (f.y() + 2, f.h() - 4);
                let rad = t.radius.min(ch / 2);
                let lay = v.layout();
                for (k, &(_, x, w)) in lay.iter().enumerate() {
                    let a = v.hover.amount(k);
                    draw::set_draw_color(mix(t.surface_alt, t.accent, 0.25 * a));
                    draw::draw_rounded_rectf(f.x() + x, cy, w, ch, rad);
                }
                // The highlight slides between chips (sub-pixel).
                let (px, pw) = (v.pill[0].get() - v.scroll.pos() as f64, v.pill[1].get());
                if v.placed {
                    heroui::fx::fill_rounded(f.x() as f64 + px, cy as f64, pw, ch as f64, rad as f64, t.accent, 1.0);
                }
                draw::set_font(t.font(), t.font_size - 1);
                for &(c, x, w) in &lay {
                    // Text under the highlight takes its color as it passes.
                    let cover = if v.placed { ((px + pw).min((x + w) as f64) - px.max(x as f64)) / w as f64 } else { 0.0 };
                    draw::set_draw_color(mix(t.text, t.accent_text, cover.clamp(0.0, 1.0) as f32));
                    draw::draw_text2(c.map_or("All", |c| CATEGORIES[c].0), f.x() + x, cy, w, ch, Align::Center);
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
                        let s = v.borrow();
                        let max = (s.width() - f.w()).max(0) as f64;
                        let mut me = me.clone();
                        s.scroll.wheel(d as f64, max, move || repaint(&mut me));
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
                let same_chips = s.cats == l.cats && s.placed;
                s.cats = l.cats.clone();
                s.selected = l.category;
                let lay = s.layout();
                if let Some(&(_, x, cw)) = lay.iter().find(|(c, _, _)| *c == l.category) {
                    // The highlight slides there (or starts there).
                    let left = (x + s.scroll.pos()) as f64;
                    for (t, v) in s.pill.iter().zip([left, cw as f64]) {
                        if same_chips {
                            let mut w2 = w.clone();
                            t.spring_to(v, SEL_SPRING, move || repaint(&mut w2));
                        } else {
                            t.set(v);
                        }
                    }
                    s.placed = true;
                    // Keep the chosen one in view.
                    let max = (s.width() - w.w()).max(0) as f64;
                    let target = s.scroll.target();
                    let x = x + s.scroll.pos() - target.round() as i32;
                    let mut w2 = w.clone();
                    if x < 0 {
                        s.scroll.scroll_to(target + x as f64, max, move || repaint(&mut w2));
                    } else if x + cw > w.w() {
                        s.scroll.scroll_to(target + (x + cw - w.w()) as f64, max, move || repaint(&mut w2));
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
