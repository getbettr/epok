use std::cmp::Reverse;

use itertools::Itertools;

use crate::{
    Backend, BatchOpts, Error, Executor, Result, Rule, RULE_MARKER,
    SERVICE_MARKER,
};

pub struct IptablesBackend {
    executor: Executor,
    batch_opts: BatchOpts,
    rule_state: String,
    local_ip: Option<String>,
}

impl Backend for IptablesBackend {
    fn read_state(&mut self) {
        self.rule_state = self
            .executor
            .run_fun(format!("sudo iptables-save -t nat | grep {RULE_MARKER}"))
            .unwrap_or_else(|_| "".to_owned());
    }

    fn apply_rules(
        &mut self,
        rules: impl IntoIterator<Item = Rule>,
    ) -> Result<()> {
        self.executor
            .run_commands(
                rules
                    .into_iter()
                    .filter(|rule| !self.rule_state.contains(&rule.rule_id()))
                    .sorted_unstable_by_key(|r| Reverse(r.node_index))
                    .map(|rule| {
                        let statement =
                            iptables_statement(&rule, &self.local_ip);
                        format!("sudo iptables -w -t nat -A {statement}")
                    }),
                &self.batch_opts,
            )
            .map_err(|e| Error::BackendError(Box::new(e)))?;
        Ok(())
    }

    fn delete_rules<P>(&mut self, pred: P) -> Result<()>
    where
        P: FnMut(&&str) -> bool,
    {
        self.executor
            .run_commands(
                self.rule_state.lines().filter(pred).map(append_to_delete),
                &self.batch_opts,
            )
            .map_err(|e| Error::BackendError(Box::new(e)))
    }
}

impl IptablesBackend {
    pub fn new(
        executor: Executor,
        batch_opts: BatchOpts,
        local_ip: Option<String>,
    ) -> Self {
        Self { executor, batch_opts, rule_state: Default::default(), local_ip }
    }
}

fn append_to_delete(rule: &str) -> String {
    let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
    rule_parts.remove(0);
    format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "))
}

fn iptables_statement(rule: &Rule, local_ip: &Option<String>) -> String {
    let (host_port, node_port) =
        rule.service.get_ports().expect("invalid service");
    let d_ip = match local_ip {
        None => "".to_owned(),
        Some(ip) => format!("-d {ip}", ip = ip),
    };
    let (chain, selector) = match rule.interface.name.as_str() {
        "lo" => (
            "OUTPUT",
            format!(
                "-o lo -p tcp -d {local_ip} --dport {host_port} -m state --state NEW",
                local_ip = local_ip
                    .as_ref()
                    .expect("should not have a local rule without local IP")
            ),
        ),
        _ => (
            "PREROUTING",
            format!(
                "-i {interface} -p tcp {d_ip} --dport {host_port} -m state --state NEW",
                interface = rule.interface.name,
            ),
        ),
    };
    let balance = match rule.node_index {
        i if i == 0 => "".to_owned(),
        i => format!("-m statistic --mode nth --every {} --packet 0", i + 1),
    };
    let comment = format!(
        "-m comment --comment 'service: {}; node: {}; {RULE_MARKER}: {}; {SERVICE_MARKER}: {}'",
        rule.service.fqn(),
        rule.node.name,
        rule.rule_id(),
        rule.service_id(),
    );
    let jump = format!(
        "-j DNAT --to-destination {node_addr}:{node_port}",
        node_addr = rule.node.addr,
    );
    format!("{chain} {selector} {balance} {comment} {jump}")
}
