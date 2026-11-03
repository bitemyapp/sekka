//! Observed routes, independent of the saved output preference. A link
//! disappearing is not itself loss: WirePlumber also recreates links when
//! moving a stream. Keep its last destination until a replacement links.
use std::collections::{BTreeMap, BTreeSet};

use super::{AudioDevice, spa};

#[derive(Default)]
pub(super) struct Routing {
    pub sinks: BTreeMap<u32, AudioDevice>,
    pub cards: BTreeMap<u32, (u32, i32)>,
    pub links: BTreeMap<u32, (u32, u32)>,
    pub stream: Option<u32>,
    pub actual: BTreeSet<u32>,
}

impl Routing {
    pub fn linked(&mut self) {
        let Some(stream) = self.stream else { return };
        let mut seen = BTreeSet::new();
        let mut pending = vec![stream];
        while let Some(node) = pending.pop() {
            if seen.insert(node) {
                pending.extend(self.links.values().filter(|(from, _)| *from == node).map(|(_, to)| *to));
            }
        }
        let actual: BTreeSet<_> = seen.into_iter().filter(|id| self.sinks.contains_key(id)).collect();
        if !actual.is_empty() {
            self.actual = actual;
        }
    }

    pub fn remove(&mut self, id: u32) -> bool {
        if self.links.remove(&id).is_some() {
            self.linked();
        }
        self.cards.remove(&id);
        self.sinks.remove(&id);
        // Do not replace `actual` on link removal: the sink's removal can
        // follow its links, and a temporary unlink is also a normal move.
        self.actual.remove(&id)
    }

    pub fn reset(&mut self) {
        self.stream = None;
        self.actual.clear();
    }

    pub fn devices(&self) -> Vec<AudioDevice> {
        self.actual.iter().filter_map(|id| self.sinks.get(id).cloned()).collect()
    }
}

/// Commit EnumRoute and Route together at a core round trip. Policy can
/// select the replacement route before the corresponding availability
/// notification reaches us. Test the *previous* active route against the
/// new availability, before accepting the replacement.
#[derive(Default)]
pub(super) struct Routes {
    pub active: BTreeMap<i32, i32>,
    pub pending_active: BTreeMap<i32, i32>,
    pub unavailable: BTreeSet<i32>,
}

impl Routes {
    pub fn commit(&mut self) -> BTreeSet<i32> {
        let lost = self
            .active
            .iter()
            .filter(|(_, route)| self.unavailable.contains(route))
            .map(|(device, _)| *device)
            .collect();
        self.active.append(&mut self.pending_active);
        lost
    }

    pub fn param(&mut self, kind: spa::param::ParamType, pod: &spa::pod::Pod) {
        use spa::pod::Value;
        use spa::sys::*;
        let Ok((_, Value::Object(object))) =
            spa::pod::deserialize::PodDeserializer::deserialize_any_from(pod.as_bytes())
        else {
            return;
        };
        let int = |key| {
            object.properties.iter().find_map(|p| match p.value {
                Value::Int(value) if p.key == key => Some(value),
                _ => None,
            })
        };
        let id = |key| {
            object.properties.iter().find_map(|p| match p.value {
                Value::Id(value) if p.key == key => Some(value.0),
                _ => None,
            })
        };
        if id(SPA_PARAM_ROUTE_direction) != Some(SPA_DIRECTION_OUTPUT) {
            return;
        }
        let Some(index) = int(SPA_PARAM_ROUTE_index) else { return };
        if let Some(available) = id(SPA_PARAM_ROUTE_available) {
            if available == SPA_PARAM_AVAILABILITY_no {
                self.unavailable.insert(index);
            } else {
                self.unavailable.remove(&index);
            }
        }
        if kind == spa::param::ParamType::Route
            && let Some(device) = int(SPA_PARAM_ROUTE_device)
        {
            self.pending_active.insert(device, index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(routes: &mut Routes, active: bool, index: i32, available: u32) {
        use spa::pod::{Object, Property, Value};
        use spa::sys::*;
        let kind = if active { spa::param::ParamType::Route } else { spa::param::ParamType::EnumRoute };
        let value = Value::Object(Object {
            type_: SPA_TYPE_OBJECT_ParamRoute,
            id: kind.as_raw(),
            properties: vec![
                Property::new(SPA_PARAM_ROUTE_index, Value::Int(index)),
                Property::new(SPA_PARAM_ROUTE_direction, Value::Id(spa::utils::Id(SPA_DIRECTION_OUTPUT))),
                Property::new(SPA_PARAM_ROUTE_device, Value::Int(0)),
                Property::new(SPA_PARAM_ROUTE_available, Value::Id(spa::utils::Id(available))),
            ],
        });
        let bytes = spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &value)
            .unwrap()
            .0
            .into_inner();
        routes.param(kind, spa::pod::Pod::from_bytes(&bytes).unwrap());
    }

    fn graph() -> Routing {
        let mut graph = Routing::default();
        for id in [10, 20] {
            graph.sinks.insert(id, AudioDevice { name: id.to_string(), description: id.to_string() });
        }
        graph.stream = Some(1);
        graph.links.insert(100, (1, 10));
        graph.linked();
        graph
    }

    #[test]
    fn unlink_then_sink_removal_is_loss_but_an_unrelated_sink_is_not() {
        let mut graph = graph();
        assert!(!graph.remove(100));
        graph.linked();
        assert!(!graph.remove(20));
        assert!(graph.remove(10));
    }

    #[test]
    fn moving_then_removing_the_old_sink_does_not_pause() {
        for new_link_first in [false, true] {
            let mut graph = graph();
            if !new_link_first {
                graph.remove(100);
            }
            graph.links.insert(101, (1, 20));
            graph.linked();
            if new_link_first {
                graph.remove(100);
            }
            assert!(!graph.remove(10));
            assert!(graph.remove(20));
        }
    }

    #[test]
    fn loss_is_detected_before_a_fallback_link_and_through_filters() {
        let mut graph = graph();
        graph.links.insert(100, (1, 2));
        graph.links.insert(102, (2, 10));
        graph.linked();
        assert!(graph.remove(10));
        graph.links.insert(101, (2, 20));
        graph.linked();
        assert_eq!(graph.actual, BTreeSet::from([20]));
    }

    #[test]
    fn route_replacement_does_not_hide_the_old_routes_loss() {
        use spa::sys::*;
        for availability_first in [true, false] {
            let mut routes = Routes::default();
            route(&mut routes, true, 7, SPA_PARAM_AVAILABILITY_yes);
            assert!(routes.commit().is_empty());
            if availability_first {
                route(&mut routes, false, 7, SPA_PARAM_AVAILABILITY_no);
            }
            route(&mut routes, true, 8, SPA_PARAM_AVAILABILITY_yes);
            if !availability_first {
                route(&mut routes, false, 7, SPA_PARAM_AVAILABILITY_no);
            }
            assert_eq!(routes.commit(), BTreeSet::from([0]));
            assert!(routes.commit().is_empty());
        }
        let mut routes = Routes::default();
        route(&mut routes, true, 7, SPA_PARAM_AVAILABILITY_unknown);
        assert!(routes.commit().is_empty());
        route(&mut routes, true, 8, SPA_PARAM_AVAILABILITY_yes);
        assert!(routes.commit().is_empty(), "requested change between available routes");
    }

    #[test]
    fn unplugging_an_inactive_route_does_not_interrupt_the_active_one() {
        use spa::sys::*;
        let mut routes = Routes::default();
        route(&mut routes, true, 7, SPA_PARAM_AVAILABILITY_yes);
        routes.commit();
        route(&mut routes, false, 8, SPA_PARAM_AVAILABILITY_no);
        assert!(routes.commit().is_empty());
        route(&mut routes, false, 7, SPA_PARAM_AVAILABILITY_no);
        assert_eq!(routes.commit(), BTreeSet::from([0]));
    }
}
