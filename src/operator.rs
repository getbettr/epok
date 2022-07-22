use std::cmp::Reverse;

use cmd_lib::run_fun;
use itertools::{iproduct, Itertools};
use sha256::digest;

use crate::*;

type Result = anyhow::Result<()>;

impl Executor {
    fn run_fun<S: AsRef<str>>(&self, cmd: S) -> anyhow::Result<String> {
        let cmd = cmd.as_ref();
        debug!("running command: {}", &cmd);
        match self {
            Executor::Local => Ok(run_fun!(sh -c "$cmd")?),
            Executor::Ssh(ssh_host) => {
                let (host, port, key) = (&ssh_host.host, ssh_host.port, &ssh_host.key_path);
                Ok(run_fun!(ssh -p $port -i $key $host "$cmd")?)
            }
        }
    }
}

#[derive(Debug)]
struct Rule<'a> {
    node: &'a Node,
    service: &'a Service,
    interface: &'a Interface,
    num_nodes: usize,
    node_index: usize,
}

impl<'a> Rule<'a> {
    fn iptables_args(&self) -> String {
        let (host_port, node_port) = self.service.get_ports().expect("invalid service");

        let input = format!(
            "-i {interface} -p tcp --dport {host_port} -m state --state NEW",
            interface = self.interface,
            host_port = host_port,
        );
        let balance = match self.node_index {
            i if i == 0 => "".to_owned(),
            i => format!("-m statistic --mode nth --every {} --packet 0", i + 1),
        };
        let comment = format!(
            "-m comment --comment 'service: {}; node: {}; {}: {}; {}: {}'",
            self.service.fqn(),
            self.node.name,
            RULE_MARKER,
            self.rule_id(),
            SERVICE_MARKER,
            self.service_id(),
        );
        let jump = format!(
            "-j DNAT --to-destination {node_addr}:{node_port}",
            node_addr = self.node.addr,
            node_port = node_port,
        );
        format!(
            "PREROUTING {input} {balance} {comment} {jump}",
            input = input,
            balance = balance,
            comment = comment,
            jump = jump,
        )
    }

    fn rule_id(&self) -> String {
        let (host_port, node_port) = self.service.get_ports().expect("invalid service");

        digest(format!(
            "{}::{}::{}::{}::{}::{}::{}",
            self.service_id(),
            self.node.addr,
            self.num_nodes,
            self.node_index,
            self.interface,
            host_port,
            node_port,
        ))
    }

    fn service_id(&self) -> String {
        digest(self.service.fqn())
    }
}

pub struct Operator {
    executor: Executor,
    rule_state: String,
    batch_opts: BatchOpts,
}

impl Operator {
    pub fn new(executor: Executor, batch_opts: BatchOpts) -> Self {
        Self {
            executor,
            rule_state: Default::default(),
            batch_opts,
        }
    }

    pub fn reconcile(&mut self, new_state: &State, old_state: &State) -> Result {
        let (added, removed) = new_state.diff(old_state);
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }

        info!("added state: {:?}", &added);
        info!("removed state: {:?}", &removed);

        self.read_rule_state();

        // Case 1: same node set
        if new_state.nodes == old_state.nodes {
            self.apply_rules(make_rules(&State {
                nodes: new_state.nodes.clone(),
                ..added
            }))?;

            let removed_service_ids = make_rules(&State {
                nodes: new_state.nodes.clone(),
                ..removed
            })
            .map(|rule| rule.service_id())
            .collect::<Vec<_>>();

            return self.delete_rules(|&rule| {
                removed_service_ids
                    .iter()
                    .any(|service_id| rule.contains(service_id))
            });
        }

        // Case 2: node added/removed => full cycle
        let new_rules = make_rules(new_state).collect::<Vec<_>>();

        let new_rule_ids = new_rules
            .iter()
            .map(|rule| rule.rule_id())
            .collect::<Vec<_>>();

        self.apply_rules(new_rules.into_iter())?;

        self.delete_rules(|&rule| {
            new_rule_ids
                .iter()
                .all(|new_rule_id| !rule.contains(new_rule_id))
        })
    }

    pub fn cleanup(&mut self) -> Result {
        self.read_rule_state();
        self.delete_rules(|_| true)
    }

    fn read_rule_state(&mut self) {
        self.rule_state = self
            .executor
            .run_fun(format!("sudo iptables-save -t nat | grep {}", RULE_MARKER))
            .unwrap_or_else(|_| "".to_owned());
    }

    fn apply_rules<'a>(&'a self, rules: impl Iterator<Item = Rule<'a>>) -> Result {
        self.run_commands(
            rules
                .filter(|rule| !self.rule_state.contains(&rule.rule_id()))
                .map(|rule| format!("sudo iptables -w -t nat -A {}", rule.iptables_args())),
        )?;
        Ok(())
    }

    fn delete_rules<P>(&self, pred: P) -> Result
    where
        P: FnMut(&&str) -> bool,
    {
        self.run_commands(self.rule_state.lines().filter(pred).map(append_to_delete))
    }

    fn run_commands(&self, commands: impl Iterator<Item = String>) -> Result {
        if self.batch_opts.batch_commands {
            let sep = "; ".to_owned();
            let batch = Batch::new(commands, self.batch_opts.batch_size, &sep);
            for command in batch {
                self.executor.run_fun(command)?;
            }
        } else {
            for command in commands {
                self.executor.run_fun(command)?;
            }
        }
        Ok(())
    }
}

fn make_rules(state: &State) -> impl Iterator<Item = Rule> {
    let num_nodes = state.nodes.len();
    iproduct!(
        state.nodes.iter().enumerate(),
        &state.services,
        &state.interfaces
    )
    .map(|((node_index, node), service, interface)| Rule {
        node,
        service,
        interface,
        num_nodes,
        node_index,
    })
    .sorted_unstable_by_key(|r| Reverse(r.node_index))
}

fn append_to_delete(rule: &str) -> String {
    let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
    rule_parts.remove(0);
    format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "))
}
