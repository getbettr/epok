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

struct App<'a> {
    op_queue: VecDeque<Op>,
    state: State,
    operator: &'a mut Operator<'a>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");
    print_startup_string();

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);

    let kube_client = Client::try_default().await?;

    let (op_sender, mut op_receiver) = mpsc::channel(1);

    let service_watcher = watcher(
        Api::<CoreService>::all(kube_client.clone()),
        ListParams::default(),
    )
    .for_each(|event| async { App::process_event(event.unwrap(), &op_sender).await });

    let node_watcher = watcher(
        Api::<CoreNode>::all(kube_client.clone()),
        ListParams::default(),
    )
    .for_each(|event| async { App::process_event(event.unwrap(), &op_sender).await });

    let mut operator = Operator::new(&opts.executor);

    let app_arc = Arc::new(Mutex::new(App {
        op_queue: VecDeque::new(),
        state: State::empty_with_interfaces(
            opts.interfaces
                .split(',')
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
        operator: &mut operator,
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
                        App::reconcile(Arc::clone(&app_arc)).await;
                    } else {
                        sleepers.clear();
                        sleepers.push(sleep(DEBOUNCE_TIMEOUT));
                    }
                }
                _ = sleepers.pop().unwrap_or_else(|| sleep(Duration::MAX)) => {
                    App::reconcile(Arc::clone(&app_arc)).await;
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

impl<'a> App<'a> {
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

    async fn process_event<T>(ev: Event<T>, tx: &Sender<Op>)
    where
        Ops: From<Event<T>>,
    {
        for op in Ops::from(ev).iter() {
            tx.send(op).await.expect("send failed");
        }
    }

    async fn reconcile(app_arc: Arc<Mutex<Self>>) {
        info!("calling operator.reconcile()");

        let mut this = app_arc.lock().await;

        let old_state = this.get();
        let new_state = this.reduce();

        if let Err(x) = this.operator.reconcile(&new_state, &old_state) {
            warn!("error during reconcile: {:?}", x)
        }
    }
}
