use kube::runtime::watcher::Event;
use std::{collections::BTreeSet, ops::Sub};

use crate::*;

pub type Interface = String;

#[derive(Clone, Default, Debug)]
pub struct State {
    pub interfaces: Vec<Interface>,
    pub services: BTreeSet<Service>,
    pub nodes: BTreeSet<Node>,
}

impl State {
    pub fn empty_with_interfaces(interfaces: Vec<Interface>) -> Self {
        Self {
            interfaces,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty() && self.nodes.is_empty()
    }
}

impl Sub for &State {
    type Output = State;

    fn sub(self, rhs: Self) -> Self::Output {
        State {
            interfaces: self.interfaces.clone(),
            services: &self.services - &rhs.services,
            nodes: &self.nodes - &rhs.nodes,
        }
    }
}

impl State {
    pub fn diff(&self, old_state: &Self) -> (Self, Self) {
        let added = self - old_state;
        let removed = old_state - self;
        (added, removed)
    }
}

#[derive(Debug, Clone)]
pub enum Op {
    NodeAdd(Node),
    NodeRemove(String),
    ServiceAdd(Service),
    ServiceRemove(String),
}

impl Op {
    pub fn apply(&self, state: &mut State) {
        match self {
            Op::NodeAdd(node) => {
                state.nodes.insert(node.to_owned());
            }
            Op::NodeRemove(node_name) => state.nodes.retain(|x| &x.name != node_name),
            Op::ServiceAdd(service) => {
                state.services.insert(service.to_owned());
            }
            Op::ServiceRemove(svc_fqn) => {
                state.services.retain(|s| &s.fqn() != svc_fqn);
            }
        }
    }
}

pub struct Ops(pub Vec<Op>);

impl Ops {
    pub fn iter(self) -> impl Iterator<Item = Op> {
        self.0.into_iter()
    }
}

impl From<Event<CoreService>> for Ops {
    fn from(event: Event<CoreService>) -> Self {
        let ops = match event {
            Event::Applied(obj) => Service::try_from(&obj)
                .map(|svc| vec![Op::ServiceRemove(svc.fqn()), Op::ServiceAdd(svc)]),
            Event::Restarted(objs) => Ok(objs
                .iter()
                .filter_map(|o| Service::try_from(o).ok())
                .map(Op::ServiceAdd)
                .collect()),
            Event::Deleted(obj) => {
                Service::try_from(&obj).map(|svc| vec![Op::ServiceRemove(svc.fqn())])
            }
        };
        Ops(ops.unwrap_or_default())
    }
}

impl From<Event<CoreNode>> for Ops {
    fn from(event: Event<CoreNode>) -> Self {
        let ops = match event {
            Event::Applied(obj) => Node::try_from(&obj)
                .map(|node| vec![Op::NodeRemove(node.name.to_owned()), Op::NodeAdd(node)]),
            Event::Restarted(objs) => Ok(objs
                .iter()
                .filter_map(|o| Node::try_from(o).ok())
                .map(Op::NodeAdd)
                .collect()),
            Event::Deleted(obj) => Node::try_from(&obj).map(|node| vec![Op::NodeRemove(node.name)]),
        };
        Ops(ops.unwrap_or_default())
    }
}
