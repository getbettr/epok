use std::{fmt::Debug, sync::Arc};
use std::collections::VecDeque;

use clap::Parser;
use futures::StreamExt;
use k8s_openapi::api::core::v1::{Node as CoreNode, Service as CoreService};
use kube::{
    api::ListParams,
    runtime::{watcher, watcher::Event},
    Api, Client,
};
use tokio::{
    select,
    sync::{mpsc, mpsc::Sender, Mutex},
    time::{sleep, Duration},
};

use epok::*;

async fn proc_ev<T>(ev: Event<T>, tx: &Sender<Op>)
where
    Ops: From<Event<T>>,
{
    for op in Ops::from(ev).0 {
        tx.send(op).await.unwrap();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");
    print_startup_string();

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);

    let kc = Client::try_default().await?;

    let (tx, mut rx) = mpsc::channel(1);

    let svc_watcher = watcher(Api::<CoreService>::all(kc.clone()), ListParams::default())
        .for_each(|event| async { proc_ev(event.unwrap(), &tx).await });
    let node_watcher = watcher(Api::<CoreNode>::all(kc.clone()), ListParams::default())
        .for_each(|event| async { proc_ev(event.unwrap(), &tx).await });

    let ops = Arc::new(Mutex::new(VecDeque::new()));

    let mut state = State::empty_with_interfaces(
        opts.interfaces
            .split(',')
            .map(String::from)
            .collect::<Vec<_>>(),
    );

    // there has to be a better way to debounce than this...
    let deb = async {
        let mut sleepers = Vec::new();
        loop {
            select! {
                Some(op) = rx.recv() => {
                    info!("received op {:?}", &op);
                    ops.lock().await.push_back(op);
                    sleepers.push(sleep(DEBOUNCE_TIMEOUT));
                }
                _ = sleepers.pop().unwrap_or_else(|| sleep(Duration::MAX)) => {
                    info!("debounce timeout; changing state...");
                    let old_state = state.clone();
                    let mut ops = ops.lock().await;
                    while let Some(op) = ops.pop_front() {
                        op.apply(&mut state);
                    }
                    info!("state diff: {:?}", state.diff(old_state));
                }
            }
        }
    };

    select! {
        _ = svc_watcher => warn!("service watcher exited"),
        _ = node_watcher => warn!("node watcher exited"),
        _ = deb => warn!("debouncer exited"),
    };
    Ok(())
}
