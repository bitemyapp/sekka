use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_kit::widget::{Event, EventCx, LayoutCx, Status, Widget};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct Clock {
    count: Arc<AtomicUsize>,
}
impl Widget for Clock {
    fn name(&self) -> &'static str {
        "clock"
    }
    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let started = cx.state::<bool>();
        if !*started {
            *started = true;
            cx.set_timer(7, Duration::from_millis(600));
            cx.request_tick(Duration::ZERO);
        }
        bc.constrain(Size::new(100., 100.))
    }
    fn hit(&self) -> bool {
        true
    }
    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Timer(7) => {
                self.count.fetch_add(1, Ordering::Relaxed);
                cx.set_timer(7, Duration::from_millis(600));
            }
            Event::PointerDown { .. } => cx.set_timer(7, Duration::from_millis(600)),
            Event::Tick => {
                self.count.fetch_add(100, Ordering::Relaxed);
            }
            _ => return Status::Ignored,
        }
        Status::Handled
    }
}
struct Other {
    count: Arc<AtomicUsize>,
}
impl Widget for Other {
    fn name(&self) -> &'static str {
        "clock"
    }
    fn event(&mut self, _: &mut EventCx, e: &Event) -> Status {
        if matches!(e, Event::Timer(_)) {
            self.count.fetch_add(10000, Ordering::Relaxed);
        }
        Status::Ignored
    }
}
#[derive(Default)]
struct App {
    window: WindowKey,
    count: Arc<AtomicUsize>,
    mode: u8,
}
#[derive(Clone)]
enum Msg {
    Mode(u8),
    Hide,
    Show,
    Close,
}
impl Program for App {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("test.timers", "Timers", "Timers")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Timers"));
    }
    fn update(&mut self, m: Msg, cx: &mut Cx<Msg>) {
        match m {
            Msg::Mode(m) => self.mode = m,
            Msg::Hide => cx.hide_window(self.window),
            Msg::Show => cx.show_window(self.window),
            Msg::Close => cx.close_window(self.window),
        }
    }
    fn view(&self, _: WindowKey) -> Element<Msg> {
        match self.mode {
            0 => Element::new(Clock { count: self.count.clone() }, vec![]).key(1),
            1 => Element::new(Other { count: self.count.clone() }, vec![]).key(1),
            3 => Element::new(Clock { count: self.count.clone() }, vec![]),
            4 => Element::new(Other { count: self.count.clone() }, vec![]),
            _ => text("removed").into(),
        }
    }
}

#[test]
fn unkeyed_same_name_type_replacement_ends_state_and_timer_lifetime() {
    let mut h = Harness::new(App { mode: 3, ..App::default() });
    h.advance(Duration::ZERO);
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100);
    let stable = h.platform.timer_tokens();
    for _ in 0..100 {
        h.send(Msg::Mode(3));
        h.advance(Duration::ZERO);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100, "same-type unkeyed rebuilds retain state");
    assert_eq!(h.platform.timer_tokens(), stable, "same-type unkeyed rebuilds retain their timer");
    h.send(Msg::Mode(4));
    assert_eq!(h.platform.pending_timers(), 0, "same name, different concrete type ends the timer lifetime");
    for token in &stable {
        h.deliver_timer(*token);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100);
    h.send(Msg::Mode(3));
    h.advance(Duration::ZERO);
    assert_eq!(h.platform.pending_timers(), 1, "returning Clock starts with fresh state");
    assert_eq!(h.program().count.load(Ordering::Relaxed), 200);
    for token in stable {
        h.deliver_timer(token);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), 200, "the old lifetime cannot deliver a timer");
    h.advance(Duration::from_millis(600));
    assert_eq!(h.program().count.load(Ordering::Relaxed), 201);
    h.send(Msg::Close);
    assert_eq!(h.platform.pending_timers(), 0);
}
#[test]
fn keyed_timers_are_bounded_suspend_and_reject_stale_node_callbacks() {
    let mut h = Harness::new(App::default());
    let w = h.window();
    h.advance(Duration::ZERO);
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100, "layout tick is not delayed by the cursor timer");
    let stable = h.platform.timer_tokens();
    for _ in 0..100 {
        h.send(Msg::Mode(0));
        h.advance(Duration::ZERO);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100, "same-type keyed rebuilds retain state");
    assert_eq!(h.platform.timer_tokens(), stable, "same-type keyed rebuilds retain their original timer");
    let old = h.platform.timer_tokens();
    for _ in 0..1000 {
        h.click(w, Point::new(20., 20.));
    }
    assert_eq!(h.platform.pending_timers(), 1);
    for t in old {
        h.deliver_timer(t);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100);
    h.advance(Duration::from_millis(599));
    assert_eq!(h.program().count.load(Ordering::Relaxed), 100);
    h.advance(Duration::from_millis(1));
    assert_eq!(h.program().count.load(Ordering::Relaxed), 101);
    h.send(Msg::Hide);
    assert_eq!(h.platform.pending_timers(), 0);
    h.advance(Duration::from_secs(60));
    assert_eq!(h.program().count.load(Ordering::Relaxed), 101);
    h.send(Msg::Show);
    assert_eq!(h.platform.pending_timers(), 1);
    let old = h.platform.timer_tokens();
    h.send(Msg::Mode(1));
    assert_eq!(h.platform.pending_timers(), 0);
    for t in old {
        h.deliver_timer(t);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), 101, "same key, different widget type is a new lifetime");
    h.send(Msg::Mode(0));
    h.advance(Duration::ZERO);
    assert_eq!(h.platform.pending_timers(), 1, "same-key Clock type reappears with fresh state");
    assert_eq!(h.program().count.load(Ordering::Relaxed), 201);
    h.send(Msg::Mode(2));
    h.send(Msg::Mode(0));
    h.advance(Duration::ZERO);
    let before = h.program().count.load(Ordering::Relaxed);
    let old = h.platform.timer_tokens();
    h.send(Msg::Mode(2));
    h.send(Msg::Mode(0));
    h.advance(Duration::ZERO);
    for t in old {
        h.deliver_timer(t);
    }
    assert_eq!(h.program().count.load(Ordering::Relaxed), before + 100);
    h.send(Msg::Close);
    assert_eq!(h.platform.pending_timers(), 0);
}
