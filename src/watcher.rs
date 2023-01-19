use std::{
    fmt::{Debug, Display},
    time::Duration,
};

use backoff::{backoff::Backoff, ExponentialBackoff};
use k8s_openapi::serde::de::DeserializeOwned;
use kube::{
    api::ListParams,
    runtime::{utils::StreamBackoff, watcher, watcher::Error},
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
        watcher(Api::<T>::all(client), ListParams::default()),
        backoff(),
    )
    .map(|ev| ev.map(Ops::from))
}

fn backoff() -> impl Backoff + Send + Sync {
    ExponentialBackoff {
        initial_interval: Duration::from_millis(800),
        max_interval: Duration::from_secs(30),
        randomization_factor: 1.0,
        multiplier: 2.0,
        max_elapsed_time: Some(Duration::from_secs(60)),
        ..ExponentialBackoff::default()
    }
}
