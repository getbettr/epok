use cmd_lib::run_fun;
use itertools::iproduct;
use sha256::digest;

use crate::*;

type Result = anyhow::Result<()>;

impl Executor {
    fn run_fun<S: AsRef<str>>(&self, cmd: S) -> anyhow::Result<String> {
        let cmd = cmd.as_ref();
        match self {
            Executor::Local => Ok(run_fun!(sh -c "$cmd")?),
            Executor::Ssh(ssh_host) => {
                let (host, port, key) = (
                    ssh_host.host.clone(),
                    ssh_host.port,
                    ssh_host.key_path.clone(),
                );
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
    prob: f64,
}

impl<'a> Rule<'a> {
    fn iptables_args(&self) -> String {
        let input = format!(
            "-i {interface} -p tcp --dport {host_port} -m state --state NEW",
            interface = self.interface,
            host_port = self.service.external_port.host_port,
        );
        let stat = format!(
            "-m statistic --mode random --probability {prob:.10}",
            prob = self.prob
        );
        let comment = format!(
            "-m comment --comment '{}: {}; {}: {}'",
            RULE_MARKER,
            self.rule_id(),
            SERVICE_MARKER,
            self.service_id(),
        );
        let jump = format!(
            "-j DNAT --to-destination {node_addr}:{node_port}",
            node_addr = self.node.addr,
            node_port = self.service.external_port.node_port,
        );
        format!(
            "PREROUTING {input} {stat} {comment} {jump}",
            input = input,
            stat = stat,
            comment = comment,
            jump = jump,
        )
    }

    fn rule_id(&self) -> String {
        digest(format!(
            "{}::{}::{}::{}",
            self.node.addr,
            self.service_id(),
            self.interface,
            self.prob
        ))
    }

    fn service_id(&self) -> String {
        digest(format!(
            "{}::{}::{}::{}",
            self.service.namespace,
            self.service.name,
            self.service.external_port.host_port,
            self.service.external_port.node_port,
        ))
    }
}

pub struct Operator<'a> {
    executor: &'a Executor,
    rule_state: String,
}

impl<'a> Operator<'a> {
    pub fn new(executor: &'a Executor) -> Self {
        Self {
            executor,
            rule_state: Default::default(),
        }
    }

    pub fn reconcile(&mut self, new_state: &State, old_state: &State) -> Result {
        let (added, removed) = new_state.diff(old_state);
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }

        info!("added state: {:?}", &added);
        info!("removed state: {:?}", &removed);

        self.rule_state = self
            .executor
            .run_fun(format!("sudo iptables-save -t nat | grep {}", RULE_MARKER))
            .unwrap_or_else(|_| "".to_owned());

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
            .iter()
            .map(|rule| rule.service_id())
            .collect::<Vec<_>>();

            return self.cleanup(|&rule| {
                removed_service_ids
                    .iter()
                    .any(|service_id| rule.contains(service_id))
            });
        }

        // Case 2: node added/removed => full cycle
        let new_rules = make_rules(new_state);

        let new_rule_ids = new_rules
            .iter()
            .map(|rule| rule.rule_id())
            .collect::<Vec<_>>();

        self.apply_rules(new_rules)?;

        self.cleanup(|&rule| {
            new_rule_ids
                .iter()
                .all(|new_rule_id| !rule.contains(new_rule_id))
        })
    }

    fn apply_rules(&self, rules: Vec<Rule>) -> Result {
        for rule in rules {
            if !self.rule_state.contains(&rule.rule_id()) {
                self.executor.run_fun(format!(
                    "sudo iptables -w -t nat -A {args}",
                    args = rule.iptables_args(),
                ))?;
            } else {
                info!("skipping existing rule with id: {}", rule.rule_id());
            }
        }
        Ok(())
    }

    fn cleanup<P>(&self, pred: P) -> Result
    where
        P: FnMut(&&str) -> bool,
    {
        for appended in self.rule_state.lines().filter(pred) {
            self.executor.run_fun(append_to_delete(appended))?;
        }
        Ok(())
    }
}

fn make_rules(state: &State) -> Vec<Rule> {
    let prob: f64 = (1f64) / (state.nodes.len() as f64);
    iproduct!(&state.nodes, &state.services, &state.interfaces)
        .map(|(node, service, interface)| Rule {
            node,
            service,
            interface,
            prob,
        })
        .collect()
}

fn append_to_delete(rule: &str) -> String {
    let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
    rule_parts.remove(0);
    format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "))
}
