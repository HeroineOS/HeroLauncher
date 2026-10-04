//! The list under the search field: favorites as a grid of icons with
//! names, then every app as rows (icon, name, what it is); or, while
//! searching, the matches. One widget drawing every line, with one event
//! handler (cheap however many apps there are).

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

use crate::{Launcher, Msg, Shown};

const ROW: i32 = 42;
const HEADER: i32 = 30;
const CELL: (i32, i32) = (92, 86);

type Rect = (i32, i32, i32, i32);

/// What a line shows (copied from the state when it changes).
#[derive(Clone, PartialEq)]
struct Line {
    shown: Shown,
    name: String,
    detail: String,
    icon: String,
}

#[derive(Default)]
struct View {
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
            match l.shown {
                Shown::Fav(_) => {
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
        let selected = v.sel == Some(i);
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
            Shown::Fav(_) => {
                let s = 40;
                app_icon(lx + (lw - s) / 2, ly + 10, s);
                draw::set_font(t.font(), (t.font_size - 2).max(9));
                draw::set_draw_color(t.text);
                draw::draw_text2(&fit(&l.name, lw - 8), lx, ly + 10 + s + 6, lw, t.font_size, Align::Center | Align::Inside);
            }
            Shown::Row(_) => {
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

pub fn view() -> Element<Launcher, Msg> {
    Element::new(|ctx| {
        let v: Rc<RefCell<View>> = Rc::default();
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
                            if let (Some(i), Some(rect)) = (hit, rect) {
                                let app_i = v.borrow().lines[i].shown.app();
                                if let Some(a) = app_i {
                                    emit(Msg::OpenMenuAt(a, rect));
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
                            emit(Msg::Launch(i));
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
                .map(|&shown| match shown.app().and_then(|i| l.apps.get(i)) {
                    Some(a) => Line {
                        shown,
                        name: a.name.clone(),
                        detail: if a.generic.is_empty() { a.comment.clone() } else { a.generic.clone() },
                        icon: a.icon.clone(),
                    },
                    None => Line { shown, name: String::new(), detail: String::new(), icon: String::new() },
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
            if let Some(i) = l.sel {
                s.reveal(w.w(), w.h(), i);
            }
            drop(s);
            repaint(&mut w);
        });
        f.as_base_widget()
    })
}
