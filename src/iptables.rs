use std::cmp::Reverse;

use itertools::Itertools;
use sha256::digest;

use crate::{
    operator::{rule_id, service_id},
    res::Proto,
    Backend, BatchOpts, Error, Executor, PortSpec, Result, Rule, RULE_MARKER,
    SERVICE_MARKER,
};

pub struct IptablesBackend {
    executor: Executor,
    batch_opts: BatchOpts,
    rule_state: String,
    local_ip: Option<String>,
    extra_ips: Option<String>,
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
                    .filter(|rule| {
                        !self
                            .rule_state
                            .contains(&rule_id(rule, &self.config_hash()))
                    })
                    .sorted_unstable_by_key(|r| Reverse(r.node_index))
                    .flat_map(|rule| {
                        rule.service
                            .external_ports
                            .specs
                            .iter()
                            .map(|port_spec| {
                                self.iptables_statement(
                                    port_spec,
                                    &rule,
                                    &self.local_ip,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .map(|stmt| format!("sudo iptables -w -t nat -A {stmt}")),
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

    fn config_hash(&self) -> String {
        let mut config_hash =
            digest(format!("{:?}::{:?}", self.local_ip, self.extra_ips));
        config_hash.truncate(32);
        config_hash
    }
}

impl IptablesBackend {
    pub fn new(
        executor: Executor,
        batch_opts: BatchOpts,
        local_ip: Option<String>,
        extra_ips: Option<String>,
    ) -> Self {
        Self {
            executor,
            batch_opts,
            rule_state: Default::default(),
            local_ip,
            extra_ips,
        }
    }

    fn iptables_statement(
        &self,
        port_spec: &PortSpec,
        rule: &Rule,
        local_ip: &Option<String>,
    ) -> String {
        let (host_port, node_port) =
            (port_spec.host_port, port_spec.node_port);
        let d_ip = match local_ip {
            None => "".to_owned(),
            Some(ip) => format!("-d {ip}"),
        };
        let s_range = match &rule.service.allow_range {
            None => "".to_owned(),
            Some(s) => format!("-s {s}"),
        };
        let (proto, state) = match &port_spec.proto {
            Proto::Tcp => ("-p tcp", "-m state --state NEW"),
            Proto::Udp => ("-p udp", ""),
        };

        let (chain, selector) = match rule.interface.name.as_str() {
            "lo" => (
                "OUTPUT",
                format!(
                    "-o lo -d {local_ip} {proto} --dport {host_port} {state}",
                    local_ip = local_ip
                        .as_ref()
                        .expect("should not have a local rule without local IP")
                ),
            ),
            _ => (
                "PREROUTING",
                format!(
                    "-i {interface} {s_range} {d_ip} {proto} --dport {host_port} {state}",
                    interface = rule.interface.name,
                ),
            ),
        };
        let balance = match rule.node_index {
            0 => "".to_owned(),
            i => {
                format!("-m statistic --mode nth --every {} --packet 0", i + 1)
            }
        };
        let comment = format!(
            "-m comment --comment 'service: {}; node: {}; {RULE_MARKER}: {}; {SERVICE_MARKER}: {}'",
            rule.service.fqn(),
            rule.node.name,
            rule_id(rule, &self.config_hash()),
            service_id(rule),
        );
        let jump = format!(
            "-j DNAT --to-destination {node_addr}:{node_port}",
            node_addr = rule.node.addr,
        );
        format!("{chain} {selector} {balance} {comment} {jump}")
    }
}

fn append_to_delete(rule: &str) -> String {
    let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
    rule_parts.remove(0);
    format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "))
}
