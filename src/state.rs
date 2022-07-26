use itertools::Itertools;
use kube::runtime::watcher::Event;
use std::{any::TypeId, collections::BTreeSet, ops::Sub, vec::IntoIter};

use crate::*;

#[derive(Clone, Default, Debug)]
pub struct State {
    resources: BTreeSet<Resource>,
}

impl State {
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }
}

impl Sub for &State {
    type Output = State;

    fn sub(self, rhs: Self) -> Self::Output {
        State {
            resources: &self.resources - &rhs.resources,
        }
    }
}

impl State {
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

    pub fn get<R: 'static>(&self) -> BTreeSet<R>
    where
        Resource: TryInto<R>,
        R: Ord,
    {
        self.resources
            .clone()
            .into_iter()
            .flat_map(Resource::try_into)
            .collect()
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

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
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
            Event::Deleted(obj) => {
                Resource::try_from(obj).map(|res| vec![Op::ResourceRemove(res.id())])
            }
        };
        Ops(ops.unwrap_or_default())
    }
}
