use cmd_lib::run_fun;

use crate::{logging::*, Batch, BatchOpts, Executor};

type Result = anyhow::Result<()>;

impl Executor {
    pub fn run_fun<S: AsRef<str>>(&self, cmd: S) -> anyhow::Result<String> {
        fn inner(this: &Executor, cmd: &str) -> anyhow::Result<String> {
            debug!("running command: {cmd}");
            match this {
                Executor::Local => Ok(run_fun!(sh -c "$cmd")?),
                Executor::Ssh(ssh_host) => {
                    let (host, port, key) =
                        (&ssh_host.host, ssh_host.port, &ssh_host.key_path);
                    Ok(run_fun!(ssh -p $port -i $key $host "$cmd")?)
                }
            }
        }
        inner(self, cmd.as_ref())
    }

    pub fn run_commands(
        &self,
        commands: impl Iterator<Item = String>,
        batch_opts: &BatchOpts,
    ) -> Result {
        if batch_opts.batch_commands {
            let sep = "; ".to_owned();
            let batch = Batch::new(commands, batch_opts.batch_size, &sep);
            for command in batch {
                self.run_fun(command)?;
            }
        } else {
            for command in commands {
                self.run_fun(command)?;
            }
        }
        Ok(())
    }
}
