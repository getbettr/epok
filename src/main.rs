use std::{collections::VecDeque, sync::Arc};

use clap::Parser;
use futures::StreamExt;
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

    let mut operator = Operator::new(&opts.executor);

    /*
      There has to be a better way to debounce than this...

      Each time the channel receives an Op a fresh sleeper is initialized
      and thrown in the `sleepers` vector.

      When the Op channel stalls the sleeper future will get selected
      and so trigger the `debounce` branch.

      When no sleepers remain we just give `select!` a really long-ass sleeping
      future which virtually guarantees that the op channel gets selected next
      and the cycle repeats.
    */
    let deb = async {
        let mut sleepers = Vec::new();
        loop {
            select! {
                Some(op) = rx.recv() => {
                    info!("received op {:?}", &op);

                    ops.lock().await.push_back(op);
                    sleepers.clear();
                    sleepers.push(sleep(DEBOUNCE_TIMEOUT));
                }
                _ = sleepers.pop().unwrap_or_else(|| sleep(Duration::MAX)) => {
                    info!("debounce: computing new state and calling operator.reconcile()");

                    let old_state = state.clone();

                    let mut ops = ops.lock().await;
                    while let Some(op) = ops.pop_front() {
                        op.apply(&mut state);
                    }

                    if let Err(x) = operator.reconcile(&state, &old_state) {
                        warn!("error during reconcile: {:?}", x)
                    }
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

async fn proc_ev<T>(ev: Event<T>, tx: &Sender<Op>)
where
    Ops: From<Event<T>>,
{
    for op in Ops::from(ev).0 {
        tx.send(op).await.unwrap();
    }
}
