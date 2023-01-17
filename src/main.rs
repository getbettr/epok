use std::sync::Arc;

use backoff::{backoff::Backoff, ExponentialBackoff};
use clap::Parser;
use kube::{
    api::ListParams,
    runtime::{utils::StreamBackoff, watcher},
    Api, Client,
};
use tokio::{sync::Mutex, time::Duration};
use tokio_stream::StreamExt;

use epok::*;

struct App<B: Backend> {
    state: State,
    operator: Operator<B>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);

    let local_ip = opts
        .external_interface
        .as_ref()
        .map(|iface| get_ip(iface, &opts.executor));

    let mut interfaces = opts
        .interfaces
        .split(',')
        .map(|i_name| {
            let mut iface = Interface::new(i_name);
            if let Some(ext_if) = &opts.external_interface {
                if ext_if.as_str() == i_name {
                    iface = iface.external();
                }
            }
            iface
        })
        .collect::<Vec<_>>();

    info!("{:?}", &interfaces);

    if local_ip.is_some() {
        interfaces.push(Interface::new("lo"));
    }

    let app = &Arc::new(Mutex::new(App {
        state: State::default().with(interfaces),
        operator: Operator::new(IptablesBackend::new(
            opts.executor,
            opts.batch_opts,
            local_ip,
        )),
    }));

    let kube_client = Client::try_default().await?;

    let service_watcher = StreamBackoff::new(
        watcher(
            Api::<CoreService>::all(kube_client.clone()),
            ListParams::default(),
        ),
        backoff(),
    )
    .map(|ev| ev.map(Ops::from));

    let node_watcher = StreamBackoff::new(
        watcher(
            Api::<CoreNode>::all(kube_client.clone()),
            ListParams::default(),
        ),
        backoff(),
    )
    .map(|ev| ev.map(Ops::from));

    let mut debouncer = Box::pin(
        Debounce::new(service_watcher.merge(node_watcher), OP_DEBOUNCE_TIMEOUT)
            .with_capacity(OP_DEBOUNCE_CAPACITY),
    );

    while let Some(op_batch) = debouncer.next().await {
        let mut app = app.lock().await;

        let prev_state = app.state.clone();
        let ops = op_batch.into_iter().flat_map(|ops| match ops {
            Ok(inner) => inner,
            Err(e) => {
                warn!("error during listing: {:?}", e);
                Ops(Vec::new())
            }
        });
        apply(ops, &mut app.state);

        if let Err(e) = app.operator.reconcile(&app.state, &prev_state) {
            warn!("error during reconcile: {:?}", e)
        }
    }
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

fn get_ip<I: AsRef<str>>(interface: I, executor: &Executor) -> String {
    let interface = interface.as_ref();
    executor
        .run_fun(format!(
            "ip -f inet addr show {interface} | sed -En -e 's/.*inet ([0-9.]+).*/\\1/p'"
        ))
        .unwrap_or_else(|_| panic!("could not get IPv4 address of interface {interface}"))
}
