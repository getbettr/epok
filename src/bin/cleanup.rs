use clap::Parser;

use epok::*;

#[derive(Parser, Debug)]
#[clap(about = built_info::PKG_DESCRIPTION, author = AUTHOR)]
pub struct Opts {
    #[clap(flatten)]
    pub batch_opts: BatchOpts,

    #[clap(subcommand)]
    pub executor: Executor,
}

fn main() -> anyhow::Result<()> {
    initialize_logging("EPOK_LOG_LEVEL");

    let opts = Opts::parse();
    let mut operator = Operator::new(opts.executor, opts.batch_opts);

    warn!("deleting all rules");

    operator.cleanup()
}
