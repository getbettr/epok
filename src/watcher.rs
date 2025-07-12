use std::{
    fmt::{Debug, Display},
    time::Duration,
};

use backon::ExponentialBuilder;
use k8s_openapi::serde::de::DeserializeOwned;
use kube::{
    runtime::{
        utils::StreamBackoff,
        watcher,
        watcher::{Config, Error, ExponentialBackoff},
    },
    Api, Client, Resource as CoreResource,
};
use tokio_stream::{Stream, StreamExt};

use crate::{Ops, Resource};

pub fn watch<T>(client: Client) -> impl Stream<Item = Result<Ops, Error>>
where
    T: CoreResource + DeserializeOwned + Clone + Debug + Send + 'static,
    <T as CoreResource>::DynamicType: Default,
    Resource: TryFrom<T>,
    <Resource as TryFrom<T>>::Error: Display,
{
    StreamBackoff::new(
        watcher(Api::<T>::all(client), Config::default()),
        backoff(),
    )
    .map(|ev| ev.map(Ops::from))
}

fn backoff() -> ExponentialBackoff {
    ExponentialBuilder::new()
        .with_min_delay(Duration::from_millis(800))
        .with_max_delay(Duration::from_secs(30))
        .with_factor(1.0)
        .with_total_delay(Some(Duration::from_secs(60)))
        .without_max_times()
        .into()
}
