//! A canvas's parts: when only they change, only they are drawn again.

use std::hash::{Hash, Hasher};

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

/// A chart of bars, one of them lit.
#[derive(Clone)]
struct Bars {
    values: Vec<u32>,
    lit: Option<usize>,
}

impl Hash for Bars {
    // The lit bar is a part: it's left out.
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.values.hash(h);
    }
}

fn bar(i: usize) -> Rect {
    Rect::new(10.0 + i as f64 * 30.0, 10.0, 30.0 + i as f64 * 30.0, 90.0)
}

#[derive(Default)]
struct Chart {
    bars: Vec<u32>,
    lit: Option<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Light(Option<usize>),
    Grow,
}

impl Program for Chart {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.CanvasTest", "Chart", "Chart")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.bars = vec![3, 5, 2, 8];
        cx.open_window(WindowSpec::new("Chart").size(200.0, 100.0));
    }

    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Light(i) => self.lit = i,
            Msg::Grow => self.bars[0] += 1,
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let state = Bars { values: self.bars.clone(), lit: self.lit };
        aizome_kit::canvas(state, |c, _b, s: &Bars, t| {
            for i in 0..s.values.len() {
                c.fill_rect(bar(i), if s.lit == Some(i) { t.focus() } else { t.selection() });
            }
        })
        .parts(|s: &Bars, _size| s.lit.map(|i| (bar(i), i as u64)).into_iter().collect())
        .into()
    }
}

#[test]
fn only_parts_that_change_are_drawn_again() {
    let mut h = Harness::new(Chart::default());
    let w = h.window();
    let _ = h.render(w, 1.0);
    h.take_damage(w);

    h.send(Msg::Light(Some(1)));
    assert_eq!(h.take_damage(w), vec![bar(1)]);

    // The light moves: where it was and where it is.
    h.send(Msg::Light(Some(3)));
    let mut d = h.take_damage(w);
    d.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    assert_eq!(d, vec![bar(1), bar(3)]);

    h.send(Msg::Light(None));
    assert_eq!(h.take_damage(w), vec![bar(3)]);

    // What the state hashes changes it all.
    h.send(Msg::Grow);
    let d = h.take_damage(w);
    assert!(d.iter().any(|r| r.width() >= 200.0), "{d:?}");
}
