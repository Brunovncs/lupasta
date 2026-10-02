//! A display list: what one frame draws, in window px and paint order. Built during render from
//! the animator and the overlays, then painted by a single canvas element.

use super::text::{FontFit, paint_line};
use gpui::{App, Bounds, Hsla, PathBuilder, Window, fill, point, px, quad, size};
use lupasta::palette::Rgb;
use lupasta::router::{Point, Step};

pub fn hsla(c: Rgb, alpha: f32) -> Hsla {
    let mut h: Hsla = gpui::rgb(c.to_u32()).into();
    h.a = alpha.clamp(0.0, 1.0);
    h
}

pub enum Item {
    Rect { x: f32, y: f32, w: f32, h: f32, color: Hsla },
    /// One line of text with its row top at (x, y).
    Text { x: f32, y: f32, text: String, color: Hsla },
    /// A rounded-elbow connector.
    Wire { start: Point, steps: Vec<Step>, width: f32, color: Hsla },
    Dot { x: f32, y: f32, r: f32, color: Hsla },
}

#[derive(Default)]
pub struct Frame {
    pub items: Vec<Item>,
}

impl Frame {
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Hsla) {
        self.items.push(Item::Rect { x, y, w, h, color });
    }

    pub fn text(&mut self, x: f32, y: f32, text: impl Into<String>, color: Hsla) {
        let text = text.into();
        if !text.is_empty() {
            self.items.push(Item::Text { x, y, text, color });
        }
    }
}

/// Everything painting needs besides the items.
pub struct Style {
    pub fit: FontFit,
    pub zoom: f32,
    pub cell: f32,
    pub row_h: f32,
}

pub fn paint(frame: Frame, style: &Style, window: &mut Window, cx: &mut App) {
    for item in frame.items {
        match item {
            Item::Rect { x, y, w, h, color } => {
                window.paint_quad(fill(Bounds::new(point(px(x), px(y)), size(px(w), px(h))), color));
            }
            Item::Text { x, y, text, color } => {
                paint_line(window, cx, &style.fit, x, y, style.zoom, style.cell, style.row_h, &text, color);
            }
            Item::Wire { start, steps, width, color } => {
                let mut b = PathBuilder::stroke(px(width));
                b.move_to(point(px(start.0), px(start.1)));
                for s in steps {
                    match s {
                        Step::Line((x, y)) => b.line_to(point(px(x), px(y))),
                        Step::Quad { ctrl, to } => b.curve_to(point(px(to.0), px(to.1)), point(px(ctrl.0), px(ctrl.1))),
                    }
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, color);
                }
            }
            Item::Dot { x, y, r, color } => {
                let b = Bounds::new(point(px(x - r), px(y - r)), size(px(2.0 * r), px(2.0 * r)));
                window.paint_quad(quad(b, px(r), color, px(0.0), gpui::transparent_black(), Default::default()));
            }
        }
    }
}
