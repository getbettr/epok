use cmd_lib::run_fun;
use itertools::iproduct;
use sha256::digest;

use crate::*;

impl Executor {
    fn run_fun(&self, cmd: &str) -> anyhow::Result<String> {
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

type Interface = String;

#[derive(Debug)]
struct Rule<'a> {
    node: &'a Node,
    service: &'a Service,
    interface: &'a Interface,
    num_nodes: usize,
}

impl<'a> Rule<'a> {
    fn iptables_args(&self) -> String {
        let prob: f64 = (1f64) / (self.num_nodes as f64);
        let input = format!(
            "-i {interface} -p tcp --dport {host_port}",
            interface = self.interface,
            host_port = self.service.external_port.host_port,
        );
        let stat = format!(
            "-m statistic --mode random --probability {prob:.3}",
            prob = prob
        );
        let comment = format!(
            "-m comment --comment 'epok_hash: {}; epok_svc: {}'",
            self.id(),
            self.svc_id(),
        );
        let jump = format!(
            "-j DNAT --to-destination {node_addr}:{node_port}",
            node_addr = self.node.addr,
            node_port = self.service.external_port.node_port,
        );
        format!(
            "PREROUTING {input} -m state --state NEW {stat} {comment} {jump}",
            input = input,
            stat = stat,
            comment = comment,
            jump = jump,
        )
    }

    fn svc_id(&self) -> String {
        digest(format!(
            "{}::{}::{}::{}",
            self.service.namespace,
            self.service.name,
            self.service.external_port.host_port,
            self.service.external_port.node_port,
        ))
    }

    fn id(&self) -> String {
        digest(format!(
            "{}::{}::{}::{}",
            self.node.addr,
            self.svc_id(),
            self.interface,
            self.num_nodes
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

    pub fn operate(&mut self, new_state: &State, old_state: &State) -> Result<(), anyhow::Error> {
        let (added, removed) = new_state.diff(old_state);
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }

        info!("added state: {:?}", &added);
        info!("removed state: {:?}", &removed);

        self.rule_state = self
            .executor
            .run_fun("sudo iptables-save -t nat | grep epok_hash")
            .unwrap_or_else(|_| "".to_owned());

        // Case 1: same node set
        if new_state.nodes == old_state.nodes {
            self.apply_rules(make_rules(&State {
                nodes: new_state.nodes.clone(),
                ..added
            }))?;

            let removed_state = State {
                nodes: new_state.nodes.clone(),
                ..removed
            };
            let removed_rules = make_rules(&removed_state);
            return self.cleanup(|&app| removed_rules.iter().any(|r| app.contains(&r.svc_id())));
        }

        // Case 2: node added/removed => full cycle
        let new_rules = make_rules(new_state);
        let new_hashes = new_rules.iter().map(|x| x.id()).collect::<Vec<_>>();
        self.apply_rules(new_rules)?;

        self.cleanup(|&app| new_hashes.iter().all(|r| !app.contains(r)))
    }

    fn apply_rules(&self, rules: Vec<Rule>) -> Result<(), anyhow::Error> {
        for rule in rules {
            if !self.rule_state.contains(&rule.id()) {
                let cmd = format!("sudo iptables -w -t nat -A {}", rule.iptables_args());
                self.executor.run_fun(&cmd)?;
            } else {
                info!("skipping existing rule with hash: {}", rule.id());
            }
        }
        Ok(())
    }

    fn cleanup<P>(&self, pred: P) -> Result<(), anyhow::Error>
    where
        P: FnMut(&&str) -> bool,
    {
        for appended in self.rule_state.lines().filter(pred) {
            self.executor.run_fun(&append_to_delete(appended))?;
        }
        Ok(())
    }
}

fn make_rules(state: &State) -> Vec<Rule> {
    let mut rules = Vec::new();
    let num_nodes = state.nodes.len();
    for (node, service, interface) in iproduct!(&state.nodes, &state.services, &state.interfaces) {
        rules.push(Rule {
            node,
            service,
            interface,
            num_nodes,
        })
    }
    rules
}

fn append_to_delete(rule: &str) -> String {
    let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
    rule_parts.remove(0);
    format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "))
}
