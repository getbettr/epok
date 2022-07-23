use clap::Parser;

use epok::*;

#[derive(Parser, Debug)]
#[clap(about = "External port operator for Kubernetes", author = AUTHOR)]
pub struct Opts {
    #[clap(flatten)]
    pub batch_opts: BatchOpts,

    #[clap(subcommand)]
    pub executor: Executor,
}

fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    let backend = IptablesBackend::new(opts.executor, opts.batch_opts);
    let mut operator = Operator::new(backend);

    warn!("deleting all rules");

    operator.cleanup()
}
