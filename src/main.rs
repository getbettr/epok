use clap::Parser;
use kube::Client;
use tokio_stream::StreamExt;
use epok::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    debug!("parsed options: {opts:?}");

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

    info!("{interfaces:?}");

    if local_ip.is_some() {
        interfaces.push(Interface::new("lo"));
    }

    let mut state = State::default().with(interfaces);

    let operator = Operator::new(IptablesBackend::new(
        opts.executor,
        opts.batch_opts,
        local_ip,
    ));

    let kube_client = Client::try_default().await?;
    let (services, nodes) = (
        watch::<CoreService>(kube_client.clone()),
        watch::<CoreNode>(kube_client),
    );

    let mut debounced = Debounce::boxed(services.merge(nodes));

    while let Some(op_batch) = debounced.next().await {
        let prev_state = state.clone();
        let ops = op_batch.into_iter().flat_map(|ops| match ops {
            Ok(inner) => inner,
            Err(e) => {
                warn!("error during listing: {e:?}");
                Ops(Vec::new())
            }
        });
        apply(ops, &mut state);

        if let Err(e) = operator.reconcile(&state, &prev_state) {
            warn!("error during reconcile: {e:?}")
        }
    }
    Ok(())
}

fn get_ip<I: AsRef<str>>(interface: I, executor: &Executor) -> String {
    let interface = interface.as_ref();
    executor
        .run_fun(format!(
            "ip -f inet addr show {interface} | sed -En -e 's/.*inet ([0-9.]+).*/\\1/p'"
        ))
        .unwrap_or_else(|_| panic!("could not get IPv4 address of interface {interface}"))
}
