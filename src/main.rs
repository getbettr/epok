use std::{collections::VecDeque, sync::Arc};

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
    time::{sleep, Duration},
};

use epok::*;

struct App {
    op_queue: VecDeque<Op>,
    state: State,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);

    let kube_client = Client::try_default().await?;

    let (op_sender, mut op_receiver) = mpsc::channel(1);

    let service_watcher = StreamBackoff::new(
        watcher(
            Api::<CoreService>::all(kube_client.clone()),
            ListParams::default(),
        ),
        backoff(),
    )
    .for_each(|event| async { App::process_event(event, &op_sender).await });

    let node_watcher = StreamBackoff::new(
        watcher(
            Api::<CoreNode>::all(kube_client.clone()),
            ListParams::default(),
        ),
        backoff(),
    )
    .for_each(|event| async { App::process_event(event, &op_sender).await });

    let backend = IptablesBackend::new(opts.executor, opts.batch_opts);
    let operator = Operator::new(backend);

    let operator_arc = Arc::new(Mutex::new(operator));

    let app_arc = Arc::new(Mutex::new(App {
        op_queue: VecDeque::new(),
        state: State::default().with_interfaces(
            opts.interfaces
                .split(',')
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
    }));

    /*
      There has to be a better way to debounce than this...

      Each time the channel receives an Op a fresh sleeper is initialized
      and thrown in the `sleepers` vector.

      When the Op channel stalls the sleeper future will get selected
      and so trigger the `debounce` branch.

      When no sleepers remain we just give `select!` a really long-ass sleeping
      future which virtually guarantees that the Op channel gets selected next
      and the cycle repeats.
    */
    let debouncer = async {
        let mut sleepers = Vec::new();
        loop {
            select! {
                Some(op) = op_receiver.recv() => {
                    info!("received op {:?}", &op);

                    let mut app = app_arc.lock().await;
                    app.queue(op);

                    if app.is_full() {
                        App::reconcile(Arc::clone(&app_arc), Arc::clone(&operator_arc)).await;
                    } else {
                        sleepers.clear();
                        sleepers.push(sleep(DEBOUNCE_TIMEOUT));
                    }
                }
                _ = sleepers.pop().unwrap_or_else(|| sleep(Duration::MAX)) => {
                    App::reconcile(Arc::clone(&app_arc), Arc::clone(&operator_arc)).await;
                }
            }
        }
    };

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

impl App {
    fn queue(&mut self, op: Op) {
        self.op_queue.push_back(op);
    }

    fn is_full(&self) -> bool {
        self.op_queue.len() >= MAX_OP_QUEUE_SIZE
    }

    fn get(&self) -> State {
        self.state.clone()
    }

    fn reduce(&mut self) -> State {
        while let Some(op) = self.op_queue.pop_front() {
            op.apply(&mut self.state);
        }
        self.get()
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

    async fn reconcile(
        app_arc: Arc<Mutex<Self>>,
        operator_arc: Arc<Mutex<Operator<IptablesBackend>>>,
    ) {
        let mut this = app_arc.lock().await;

        let prev_state = this.get();
        let state = this.reduce();

        tokio::task::spawn(async move {
            let mut operator = operator_arc.lock().await;
            info!("calling operator.reconcile()");
            if let Err(x) = operator.reconcile(&state, &prev_state) {
                warn!("error during reconcile: {:?}", x)
            }
        });
    }
}
