use itertools::Itertools;
use std::cmp::Reverse;

use crate::{Backend, BatchOpts, Executor, Rule, RULE_MARKER, SERVICE_MARKER};

type Result = anyhow::Result<()>;

pub struct IptablesBackend {
    executor: Executor,
    batch_opts: BatchOpts,
    rule_state: String,
}

impl Backend for IptablesBackend {
    fn read_state(&mut self) {
        self.rule_state = self
            .executor
            .run_fun(format!("sudo iptables-save -t nat | grep {}", RULE_MARKER))
            .unwrap_or_else(|_| "".to_owned());
    }

    fn apply_rules(&mut self, rules: impl IntoIterator<Item = Rule>) -> Result {
        self.executor.run_commands(
            rules
                .into_iter()
                .filter(|rule| !self.rule_state.contains(&rule.rule_id()))
                .sorted_unstable_by_key(|r| Reverse(r.node_index))
                .map(|rule| format!("sudo iptables -w -t nat -A {}", iptables_args(&rule))),
            &self.batch_opts,
        )?;
        Ok(())
    }

    fn delete_rules<P>(&mut self, pred: P) -> Result
    where
        P: FnMut(&&str) -> bool,
    {
        self.executor.run_commands(
            self.rule_state.lines().filter(pred).map(append_to_delete),
            &self.batch_opts,
        )
    }
}

impl IptablesBackend {
    pub fn new(executor: Executor, batch_opts: BatchOpts) -> Self {
        Self {
            executor,
            batch_opts,
            rule_state: Default::default(),
        }
    }
}

fn append_to_delete(rule: &str) -> String {
    let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
    rule_parts.remove(0);
    format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "))
}

fn iptables_args(rule: &Rule) -> String {
    let (host_port, node_port) = rule.service.get_ports().expect("invalid service");

    let input = format!(
        "-i {interface} -p tcp --dport {host_port} -m state --state NEW",
        interface = rule.interface,
        host_port = host_port,
    );
    let balance = match rule.node_index {
        i if i == 0 => "".to_owned(),
        i => format!("-m statistic --mode nth --every {} --packet 0", i + 1),
    };
    let comment = format!(
        "-m comment --comment 'service: {}; node: {}; {}: {}; {}: {}'",
        rule.service.fqn(),
        rule.node.name,
        RULE_MARKER,
        rule.rule_id(),
        SERVICE_MARKER,
        rule.service_id(),
    );
    let jump = format!(
        "-j DNAT --to-destination {node_addr}:{node_port}",
        node_addr = rule.node.addr,
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
