use cmd_lib::run_fun;
use itertools::iproduct;
use sha256::digest;

use crate::*;

type Result = anyhow::Result<()>;

pub trait Backend {
    fn read_state(&mut self);
    fn apply_rules(&mut self, rules: impl Iterator<Item = BaseRule>) -> Result;
    fn delete_rules<P>(&mut self, pred: P) -> Result
    where
        P: FnMut(&&str) -> bool;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BaseRule {
    pub node: Node,
    pub service: Service,
    pub interface: Interface,
    pub num_nodes: usize,
    pub node_index: usize,
}

impl BaseRule {
    pub fn rule_id(&self) -> String {
        let mut rule_id = digest(format!(
            "{}::{}::{}::{}::{}",
            self.service_id(),
            self.node.addr,
            self.num_nodes,
            self.node_index,
            self.interface,
        ));
        rule_id.truncate(16);
        rule_id
    }

    pub fn service_id(&self) -> String {
        let mut svc_hash = digest(self.service.fqn());
        svc_hash.truncate(16);
        let mut port_hash = match self.service.external_port {
            ExternalPort::Spec {
                host_port,
                node_port,
            } => {
                format!("::{}", digest(format!("{}::{}", host_port, node_port)))
            }
            ExternalPort::Absent => "".to_string(),
        };
        port_hash.truncate(16);
        format!("{}{}", svc_hash, port_hash)
    }
}

pub struct Operator<B: Backend> {
    backend: B,
}

impl<B: Backend> Operator<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn reconcile(&mut self, state: &State, prev_state: &State) -> Result {
        let (added, removed) = state.diff(prev_state);
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }

        info!("added state: {:?}", &added);
        info!("removed state: {:?}", &removed);

        self.backend.read_state();

        // Case 1: same node set
        if state.nodes == prev_state.nodes {
            let removed_service_ids = make_rules(&removed.with_nodes(state.nodes.clone()))
                .iter()
                .map(|rule| rule.service_id())
                .collect::<Vec<_>>();

            self.backend
                .apply_rules(make_rules(&added.with_nodes(state.nodes.clone())).into_iter())?;

            return self.backend.delete_rules(|&rule| {
                removed_service_ids
                    .iter()
                    .any(|service_id| rule.contains(service_id))
            });
        }

        // Case 2: node added/removed => full cycle
        let new_rules = make_rules(state);

        let new_rule_ids = new_rules
            .iter()
            .map(|rule| rule.rule_id())
            .collect::<Vec<_>>();

        self.backend.apply_rules(new_rules.into_iter())?;

        self.backend.delete_rules(|&rule| {
            new_rule_ids
                .iter()
                .all(|new_rule_id| !rule.contains(new_rule_id))
        })
    }

    pub fn cleanup(&mut self) -> Result {
        self.backend.read_state();
        self.backend.delete_rules(|_| true)
    }
}

fn make_rules(state: &State) -> Vec<BaseRule> {
    let num_nodes = state.nodes.len();
    iproduct!(
        state.nodes.iter().enumerate(),
        &state.services,
        &state.interfaces
    )
    .map(|((node_index, node), service, interface)| BaseRule {
        node: node.to_owned(),
        service: service.to_owned(),
        interface: interface.to_owned(),
        num_nodes,
        node_index,
    })
    .collect()
}

impl Executor {
    pub fn run_fun<S: AsRef<str>>(&self, cmd: S) -> anyhow::Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct TestBackend {
        rules: Rc<RefCell<Vec<BaseRule>>>,
    }

    impl Backend for TestBackend {
        fn read_state(&mut self) {
            // noop: we keep state in memory
        }

        fn apply_rules(&mut self, rules: impl Iterator<Item = BaseRule>) -> Result {
            let mut my_rules = self.rules.borrow_mut();
            for rule in rules {
                my_rules.push(rule);
            }
            Ok(())
        }

        fn delete_rules<P>(&mut self, mut pred: P) -> Result
        where
            P: FnMut(&&str) -> bool,
        {
            let mut my_rules = self.rules.borrow_mut();
            my_rules.retain(|r| !pred(&format!("{} {}", r.rule_id(), r.service_id()).as_str()));
            Ok(())
        }
    }

    #[test]
    fn test_trivial() {
        let backend = TestBackend::default();
        let check = backend.rules.clone();
        let mut operator = Operator::new(backend);

        let res = operator.reconcile(&State::default(), &State::default());

        assert!(res.is_ok());
        assert!(check.borrow().is_empty());
    }

    #[test]
    fn it_replaces_svc_on_port_change() {
        let backend = TestBackend::default();
        let rules = backend.rules.clone();
        let mut operator = Operator::new(backend);

        let state0 = empty_state();

        let state1 = state0
            .clone()
            .with_services([service_with_ep(ExternalPort::Spec {
                host_port: 123,
                node_port: 456,
            })]);
        operator.reconcile(&state1, &state0).unwrap();

        let _rules = rules.borrow();
        assert_eq!(&_rules.len(), &1);
        assert_eq!(
            &_rules[0].service.external_port,
            &ExternalPort::Spec {
                host_port: 123,
                node_port: 456
            }
        );
        drop(_rules);

        let state2 = state1
            .clone()
            .with_services([service_with_ep(ExternalPort::Spec {
                host_port: 1234,
                node_port: 456,
            })]);
        operator.reconcile(&state2, &state1).unwrap();

        let _rules = rules.borrow();
        assert_eq!(_rules.len(), 1);
        assert_eq!(
            _rules[0].service.external_port,
            ExternalPort::Spec {
                host_port: 1234,
                node_port: 456
            }
        );
    }

    #[test]
    fn it_deletes_all_rules_when_no_nodes_left() {
        let backend = TestBackend::default();
        let rules = backend.rules.clone();
        let mut operator = Operator::new(backend);

        let state0 = empty_state();

        let state1 = state0
            .clone()
            .with_services([service_with_ep(ExternalPort::Spec {
                host_port: 123,
                node_port: 456,
            })]);
        operator.reconcile(&state1, &state0).unwrap();

        let state2 = state1.clone().with_nodes([]);
        operator.reconcile(&state2, &state1).unwrap();

        assert_eq!(rules.borrow().len(), 0);
    }

    #[test]
    fn it_handles_service_remove_node_add_correctly() {
        let backend = TestBackend::default();
        let rules = backend.rules.clone();
        let mut operator = Operator::new(backend);

        let state0 = empty_state();
        let state1 = state0.clone().with_services([
            service_with_ep(ExternalPort::Spec {
                host_port: 123,
                node_port: 456,
            }),
            service_with_ep(ExternalPort::Spec {
                host_port: 789,
                node_port: 654,
            }),
        ]);
        operator.reconcile(&state1, &state0).unwrap();

        // add a node, remove a service
        let state2 = state1
            .clone()
            .with_nodes([
                Node {
                    name: "foo".to_string(),
                    addr: "bar".to_string(),
                    is_active: true,
                },
                Node {
                    name: "foo_two".to_string(),
                    addr: "bar_two".to_string(),
                    is_active: true,
                },
            ])
            .with_services([service_with_ep(ExternalPort::Spec {
                host_port: 789,
                node_port: 654,
            })]);
        operator.reconcile(&state2, &state1).unwrap();

        let _rules = rules.borrow();
        assert_eq!(_rules.len(), 2);
        assert!(_rules.iter().all(|x| x.service.external_port
            == ExternalPort::Spec {
                host_port: 789,
                node_port: 654
            }))
    }

    fn empty_state() -> State {
        State::default()
            .with_interfaces(vec!["eth0".to_owned()])
            .with_nodes([Node {
                name: "foo".to_string(),
                addr: "bar".to_string(),
                is_active: true,
            }])
    }

    fn service_with_ep(external_port: ExternalPort) -> Service {
        Service {
            name: "foo".to_string(),
            namespace: "bar".to_string(),
            external_port,
        }
    }
}
