use std::sync::Arc;

use backoff::{backoff::Backoff, ExponentialBackoff};
use clap::Parser;
use futures::StreamExt;
use kube::{
    api::ListParams,
    runtime::{
        utils::StreamBackoff,
        watcher,
        watcher::{Error as WatchError, Event},
    },
    Api, Client,
};
use tokio::{
    select,
    sync::{mpsc, mpsc::Sender, Mutex},
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;

use epok::{debounce::Debounce, *};

struct App<B: Backend> {
    state: State,
    operator: Operator<B>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);

    let app = &Arc::new(Mutex::new(App {
        state: State::default().with_interfaces(
            opts.interfaces
                .split(',')
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
        operator: Operator::new(IptablesBackend::new(opts.executor, opts.batch_opts)),
    }));

    let kube_client = Client::try_default().await?;

    let (op_sender, op_receiver) = mpsc::channel(OP_CHANNEL_SIZE);

    let service_watcher = StreamBackoff::new(
        watcher(
            Api::<CoreService>::all(kube_client.clone()),
            ListParams::default(),
        ),
        backoff(),
    )
    .for_each(|event| process_event(event, &op_sender));

    let node_watcher = StreamBackoff::new(
        watcher(
            Api::<CoreNode>::all(kube_client.clone()),
            ListParams::default(),
        ),
        backoff(),
    )
    .for_each(|event| process_event(event, &op_sender));

    let debouncer = Debounce::new(ReceiverStream::new(op_receiver), DEBOUNCE_TIMEOUT).for_each(
        |mut op_queue| async move {
            let mut app = app.lock().await;

            let prev_state = app.state.clone();
            while let Some(op) = op_queue.pop_front() {
                op.apply(&mut app.state);
            }

            if let Err(e) = app.operator.reconcile(&app.state, &prev_state) {
                warn!("error during reconcile: {:?}", e)
            }
        },
    );

    select! {
        _ = service_watcher => warn!("service watcher exited"),
        _ = node_watcher => warn!("node watcher exited"),
        _ = debouncer => warn!("debouncer exited"),
    };
    Ok(())
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

async fn process_event<T>(ev: Result<Event<T>, WatchError>, tx: &Sender<Op>)
where
    Ops: From<Event<T>>,
{
    match ev {
        Ok(inner) => {
            for op in Ops::from(inner) {
                tx.send(op).await.expect("send failed");
            }
        }
        Err(e) => {
            warn!("error during list: {:?}", e)
        }
    }
}
