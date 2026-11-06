// P6 owner-mismatch. Every row into an owner state sets its owner (the
// press, a capture from any state, the drags-only press with its
// `beneath`), and the release goes to that owner (refused at run time; the
// structs hold a placeholder owner before any entry, and a row that forgot
// `to.owner = ..` would compile).
use impl_finny::pointer::{DragsOnlyPressed, Pointer, Pressed};
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    let at = Point::new(0, 0);
    let ups = |out: &Vec<Effect>| -> Vec<Node> {
        out.iter().filter_map(|e| if let Effect::Up { node, .. } = e { Some(*node) } else { None }).collect()
    };

    m.handle(Event::Press { at, button: Button::Left, clicks: 1, hit: Hit::on(5) }, &mut out);
    assert_eq!((m.state(), m.inside().get_state::<Pressed>().owner), (StateName::Pressed, Node(5)));
    m.handle(Event::Release { at, button: Button::Left, hit: Hit::on(5) }, &mut out);
    assert_eq!(ups(&out), vec![Node(5)]);

    out.clear();
    m.handle(Event::Capture(Node(7)), &mut out);
    assert_eq!((m.state(), m.inside().get_state::<Pressed>().owner), (StateName::Pressed, Node(7)));
    m.handle(Event::Release { at, button: Button::Left, hit: Hit::on(5) }, &mut out);
    assert_eq!(ups(&out), vec![Node(7)]);

    out.clear();
    m.handle(Event::Press { at, button: Button::Left, clicks: 1, hit: Hit::edge(2, 9) }, &mut out);
    let p = m.inside().get_state::<DragsOnlyPressed>();
    assert_eq!((m.state(), p.edge, p.beneath), (StateName::DragsOnlyPressed, Node(2), Some(Node(9))));
    m.handle(Event::Capture(Node(4)), &mut out);
    assert_eq!((m.state(), m.inside().get_state::<Pressed>().owner), (StateName::Pressed, Node(4)));
    m.handle(Event::Release { at, button: Button::Left, hit: Hit::edge(2, 9) }, &mut out);
    assert_eq!(ups(&out), vec![Node(4)]);
}
