use std::{any::TypeId, collections::BTreeSet, ops::Sub, vec::IntoIter};

use itertools::Itertools;
use kube::runtime::watcher::Event;

use crate::{Resource, ResourceLike};

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct State {
    resources: BTreeSet<Resource>,
}

impl State {
    pub fn is_empty(&self) -> bool { self.resources.is_empty() }

    pub fn diff(&self, prev_state: &Self) -> (Self, Self) {
        let added = self - prev_state;
        let removed = prev_state - self;
        (added, removed)
    }

    pub fn with<R: 'static>(self, other: impl IntoIterator<Item = R>) -> Self
    where
        Resource: From<R>,
        R: ResourceLike,
    {
        let r_type = TypeId::of::<R>();
        let resources = self
            .resources
            .into_iter()
            .filter(|r| ResourceLike::type_id(r) != r_type)
            .merge(other.into_iter().map(Resource::from))
            .collect();
        Self { resources }
    }

    pub fn get<R>(&self) -> BTreeSet<R>
    where
        Resource: TryInto<R>,
        R: ResourceLike + Ord,
    {
        self.resources
            .clone()
            .into_iter()
            .flat_map(Resource::try_into)
            .collect()
    }
}

impl Sub for &State {
    type Output = State;

    fn sub(self, rhs: Self) -> Self::Output {
        State { resources: &self.resources - &rhs.resources }
    }
}

pub fn apply<I>(ops: I, state: &mut State)
where
    I: IntoIterator<Item = Op>,
{
    for op in ops.into_iter() {
        op.apply(state);
    }
}

#[derive(Debug)]
pub enum Op {
    ResourceAdd(Resource),
    ResourceRemove(String),
}

pub struct Ops(pub Vec<Op>);

impl IntoIterator for Ops {
    type Item = Op;
    type IntoIter = IntoIter<Op>;

    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}

impl Op {
    pub fn apply(&self, state: &mut State) {
        match self {
            Op::ResourceAdd(res) => {
                state.resources.insert(res.to_owned());
            }
            Op::ResourceRemove(res_id) => {
                state.resources.retain(|res| res.id() != *res_id);
            }
        }
    }
}

impl<C> From<Event<C>> for Ops
where
    Resource: TryFrom<C>,
{
    fn from(event: Event<C>) -> Self {
        let ops = match event {
            Event::Applied(obj) => Resource::try_from(obj).map(|res| {
                let mut ret = vec![Op::ResourceRemove(res.id())];
                if res.is_active() {
                    ret.push(Op::ResourceAdd(res))
                }
                ret
            }),
            Event::Restarted(objs) => Ok(objs
                .into_iter()
                .filter_map(|o| Resource::try_from(o).ok())
                .filter(Resource::is_active)
                .map(Op::ResourceAdd)
                .collect()),
            Event::Deleted(obj) => Resource::try_from(obj)
                .map(|res| vec![Op::ResourceRemove(res.id())]),
        };
        Ops(ops.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExternalPort, Node, Service};

    #[test]
    fn apply_one() {
        let svc = Service {
            name: "foo".into(),
            namespace: "bar".into(),
            external_port: ExternalPort::Spec {
                host_port: 123,
                node_port: 456,
            },
            is_internal: false,
        };
        let mut state = State::default();

        let ops = Ops::from(Event::Applied(svc.clone()));
        apply(ops, &mut state);

        assert!(!state.is_empty());
        assert!(state.get::<Service>().contains(&svc));
    }

    #[test]
    fn apply_remove_one() {
        let svc = Service {
            name: "foo".into(),
            namespace: "bar".into(),
            external_port: ExternalPort::Spec {
                host_port: 123,
                node_port: 456,
            },
            is_internal: false,
        };
        let mut state = State::default();

        let ops = Ops::from(Event::Applied(svc.clone()))
            .into_iter()
            .chain(Ops::from(Event::Deleted(svc)));
        apply(ops, &mut state);

        assert!(state.is_empty());
    }

    #[test]
    fn restart_many_apply_one() {
        let svcs = vec![
            Service {
                name: "foo".into(),
                namespace: "bar".into(),
                external_port: ExternalPort::Spec {
                    host_port: 123,
                    node_port: 456,
                },
                is_internal: false,
            },
            Service {
                name: "baz".into(),
                namespace: "quux".into(),
                external_port: ExternalPort::Spec {
                    host_port: 12321,
                    node_port: 45654,
                },
                is_internal: false,
            },
        ];

        let applied = Service {
            name: "foo".into(),
            namespace: "bar".into(),
            external_port: ExternalPort::Spec {
                host_port: 333,
                node_port: 444,
            },
            is_internal: false,
        };
        let mut state = State::default();

        let ops = Ops::from(Event::Restarted(svcs.clone()))
            .into_iter()
            .chain(Ops::from(Event::Applied(applied.clone())));
        apply(ops, &mut state);

        assert!(!state.is_empty());
        assert!(!state.get::<Service>().contains(&svcs[0]));
        assert!(state.get::<Service>().contains(&applied));
    }

    #[test]
    fn with_diff() {
        let svc1 = Service {
            name: "foo".into(),
            namespace: "bar".into(),
            external_port: ExternalPort::Spec {
                host_port: 333,
                node_port: 444,
            },
            is_internal: false,
        };

        let svc2 = Service {
            name: "foo".into(),
            namespace: "bar".into(),
            external_port: ExternalPort::Spec {
                host_port: 123,
                node_port: 321,
            },
            is_internal: false,
        };

        let node1 = Node {
            name: "node0".into(),
            addr: "1.2.3.4".into(),
            is_active: true,
        };

        let prev = State::default().with([node1.clone()]).with([svc1.clone()]);

        let cur = prev.clone().with([svc2.clone()]).with(Vec::<Node>::new());

        let (added, removed) = cur.diff(&prev);
        assert_eq!(added, State::default().with([svc2]));
        assert_eq!(removed, State::default().with([svc1]).with([node1]));
    }
}
