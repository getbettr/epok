use clap::{crate_authors, crate_description, Parser};
use epok::*;

#[derive(Parser, Debug)]
#[clap(about = crate_description!(), author = crate_authors!("\n"))]
pub struct Opts {
    #[clap(flatten)]
    pub batch_opts: BatchOpts,

    #[clap(subcommand)]
    pub executor: Executor,
}

fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    let backend = IptablesBackend::new(opts.executor, opts.batch_opts, None);
    let operator = Operator::new(backend);

    warn!("deleting all rules");
    operator.cleanup()
}
