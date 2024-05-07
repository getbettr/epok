use std::{cell::RefCell, collections::HashMap};

use itertools::iproduct;
use sha256::digest;

use crate::{
    logging::*, Error, ExternalPorts, Interface, Node, Pod, PortSpec, Proto,
    ResourceLike, Result, Service, State,
};

pub trait Backend {
    fn read_state(&mut self);
    fn apply_rules(
        &mut self,
        rules: impl IntoIterator<Item = Rule>,
    ) -> Result<()>;
    fn delete_rules<P>(&mut self, pred: P) -> Result<()>
    where
        P: FnMut(&&str) -> bool;
    fn config_hash(&self) -> String;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Rule {
    pub dest_addr: String,
    pub allow_range: Option<String>,
    pub port_spec: PortSpec,
    pub interface: Interface,
    pub nth: usize,
    pub out_of: usize,
    pub comment: Option<String>,
    pub rule_hash: String,
}

impl Rule {
    pub fn rule_id(&self, config_hash: &str) -> String {
        format!("{config_hash}::{}", self.rule_hash)
    }
}

pub struct Operator<B> {
    backend: RefCell<B>,
}

impl<B: Backend> Operator<B> {
    pub fn new(backend: B) -> Self { Self { backend: RefCell::new(backend) } }

    pub fn reconcile(&self, state: &State, prev_state: &State) -> Result<()> {
        let (added, removed) = state.diff(prev_state);
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }

        info!("added state: {added:?}");
        info!("removed state: {removed:?}");

        let mut backend = self.backend.borrow_mut();
        backend.read_state();

        // Case 1: nuclear option - node or interface changed => full cycle
        if state.get::<Node>() != prev_state.get::<Node>()
            || state.get::<Interface>() != prev_state.get::<Interface>()
        {
            let mut new_rules = make_rules(state);
            new_rules.extend(make_pod_rules(state));

            let new_rule_ids = new_rules
                .iter()
                .map(|r| r.rule_id(&backend.config_hash()))
                .collect::<Vec<_>>();

            info!("new rule ids: {new_rule_ids:?}");
            info!("config hash: {}", backend.config_hash());

            backend
                .apply_rules(new_rules)
                .map_err(|e| Error::OperatorError(Box::new(e)))?;

            return backend
                .delete_rules(|&rule| {
                    new_rule_ids
                        .iter()
                        .all(|new_rule_id| !rule.contains(new_rule_id))
                })
                .map_err(|e| Error::OperatorError(Box::new(e)));
        }

        // Case 2: service changes
        if state.get::<Service>() != prev_state.get::<Service>() {
            let removed_service_ids = removed
                .get::<Service>()
                .iter()
                .map(|s| s.service_hash())
                .collect::<Vec<_>>();

            backend
                .apply_rules(make_rules(
                    &state.clone().with(added.get::<Service>()),
                ))
                .map_err(|e| Error::OperatorError(Box::new(e)))?;

            backend
                .delete_rules(|&rule| {
                    removed_service_ids
                        .iter()
                        .any(|service_id| rule.contains(service_id))
                })
                .map_err(|e| Error::OperatorError(Box::new(e)))?;
        }

        // Case 3: pod change => semi-nuclear because one pod might have
        // different indexes in different (source_port, protocol) collections,
        // so it's safer to re-create all rules
        if state.get::<Pod>() != prev_state.get::<Pod>() {
            let new_rules = make_pod_rules(state);

            let new_rule_ids = new_rules
                .iter()
                .map(|r| r.rule_id(&backend.config_hash()))
                .collect::<Vec<_>>();

            backend
                .apply_rules(new_rules)
                .map_err(|e| Error::OperatorError(Box::new(e)))?;

            backend
                .delete_rules(|&rule| {
                    rule.contains("pod::")
                        && new_rule_ids
                            .iter()
                            .all(|new_rule_id| !rule.contains(new_rule_id))
                })
                .map_err(|e| Error::OperatorError(Box::new(e)))?;
        }

        Ok(())
    }

    pub fn cleanup(&self) -> Result<()> {
        let mut backend = self.backend.borrow_mut();
        backend.read_state();
        backend
            .delete_rules(|_| true)
            .map_err(|e| Error::OperatorError(Box::new(e)))
    }
}

fn make_rules(state: &State) -> Vec<Rule> {
    let num_nodes = state.get::<Node>().len();
    let mut rules = Vec::new();

    iproduct!(
        state.get::<Node>().iter().enumerate(),
        &state.get::<Service>(),
        &state.get::<Interface>()
    )
    .for_each(|((node_index, node), service, interface)| {
        if interface.is_external && service.is_internal {
            return;
        }

        for spec in &service.external_ports.specs {
            let dest_addr = node.addr.to_owned();
            let nth = node_index;
            let out_of = num_nodes;

            let mut rule_hash = digest(format!(
                "{}::{}::{}::{}::{}",
                dest_addr, nth, out_of, interface.name, interface.is_external,
            ));
            rule_hash.truncate(16);
            let rule_hash =
                format!("service::{}::{}", service.service_hash(), rule_hash);

            rules.push(Rule {
                dest_addr,
                allow_range: service.allow_range.to_owned(),
                port_spec: spec.to_owned(),
                interface: interface.to_owned(),
                nth,
                out_of,
                comment: Some(format!(
                    "service: {}; node: {}",
                    service.fqn(),
                    node.name
                )),
                rule_hash,
            })
        }
    });
    rules
}

fn make_pod_rules(state: &State) -> Vec<Rule> {
    let mut pod_map = HashMap::<(u16, Proto), Vec<Pod>>::new();

    state.get::<Pod>().iter().filter(|p| p.is_active()).for_each(|p| {
        p.external_ports.specs.iter().for_each(|s| {
            let mut new_p = p.clone();
            new_p.external_ports = ExternalPorts { specs: vec![s.clone()] };
            pod_map.entry((s.host_port, s.proto)).or_default().push(new_p)
        })
    });

    let mut rules = Vec::new();

    for interface in state.get::<Interface>() {
        pod_map.values().for_each(|pods| {
            let out_of = pods.len();
            pods.iter().enumerate().for_each(|(nth, pod)| {
                let dest_addr = pod.addr.to_owned();

                let mut rule_hash = digest(format!(
                    "{}::{}::{}::{}::{}",
                    dest_addr,
                    nth,
                    out_of,
                    interface.name,
                    interface.is_external,
                ));
                rule_hash.truncate(16);
                let rule_hash =
                    format!("pod::{}::{}", pod.pod_hash(), rule_hash);

                rules.push(Rule {
                    dest_addr,
                    allow_range: None,
                    port_spec: pod.external_ports.specs[0].to_owned(),
                    interface: interface.to_owned(),
                    nth,
                    out_of,
                    comment: Some(format!(
                        "pod: {}; namespace: {}",
                        pod.name, pod.namespace
                    )),
                    rule_hash,
                })
            })
        });
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{res::Proto, ExternalPorts, PortSpec};

    #[derive(Default)]
    struct TestBackend {
        rules: Vec<Rule>,
    }

    impl Operator<TestBackend> {
        fn get_rules(&self) -> Vec<Rule> {
            self.backend.borrow().rules.clone()
        }
    }

    impl Backend for TestBackend {
        fn read_state(&mut self) {
            // noop: we keep state in memory
        }

        fn apply_rules(
            &mut self,
            rules: impl IntoIterator<Item = Rule>,
        ) -> Result<()> {
            for rule in rules {
                self.rules.push(rule);
            }
            Ok(())
        }

        fn delete_rules<P>(&mut self, mut pred: P) -> Result<()>
        where
            P: FnMut(&&str) -> bool,
        {
            let config_hash = self.config_hash().to_owned();
            self.rules.retain(|r| {
                !pred(
                    &r.rule_id(&config_hash).as_str(), // &format!("{} {}", r.rule_id(&config_hash), r.service_id())
                                                       //     .as_str(),
                )
            });
            Ok(())
        }

        fn config_hash(&self) -> String { "<default>".to_owned() }
    }

    #[test]
    fn test_trivial() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let res = operator.reconcile(&State::default(), &State::default());
        assert!(res.is_ok());

        let rules = operator.get_rules();
        assert!(rules.is_empty());
    }

    #[test]
    fn it_replaces_svc_on_port_change() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state();

        let state1 = state0.clone().with([single_port_service(123, 456)]);
        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].port_spec, single_port_spec(123, 456));

        let state2 = state1.clone().with([single_port_service(1234, 456)]);
        operator.reconcile(&state2, &state1).unwrap();

        let rules = dbg!(operator.get_rules());
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].port_spec, single_port_spec(1234, 456));
    }

    #[test]
    fn it_replaces_svc_on_internal_change() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([Interface::new("eth0").external()]);

        let svc = single_port_service(123, 456);
        let state1 = state0.clone().with([svc.clone()]);
        operator.reconcile(&state1, &state0).unwrap();

        // A normal service should get a rule even for external interfaces
        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].port_spec, single_port_spec(123, 456));

        // However once the service goes "internal" the rule should be gone
        let state2 = state1.clone().with([svc.internal()]);
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 0);

        // When we make the interface internal again, the rule should pop up
        let state3 = state2.clone().with([Interface::new("eth0")]);
        operator.reconcile(&state3, &state2).unwrap();
        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].port_spec, single_port_spec(123, 456));
    }

    #[test]
    fn it_deletes_all_rules_when_no_nodes_left() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state();

        let state1 = state0.clone().with([single_port_service(123, 456)]);
        operator.reconcile(&state1, &state0).unwrap();

        let state2 = state1.clone().with(Vec::<Node>::new());
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn it_handles_service_remove_node_add_correctly() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state();
        let state1 = state0.clone().with([
            single_port_service(123, 456),
            single_port_service(789, 654),
        ]);
        operator.reconcile(&state1, &state0).unwrap();

        // add a node, remove a service
        let state2 = state1
            .clone()
            .with([
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
            .with([single_port_service(789, 654)]);
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 2);
        assert!(rules
            .iter()
            .all(|x| x.port_spec == single_port_spec(789, 654)));
    }

    #[test]
    fn it_removes_services() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([single_port_service(123, 456)]);
        operator.reconcile(&state0, &empty_state()).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].port_spec, single_port_spec(123, 456));

        let state1 = state0.clone().with(Vec::<Node>::new());
        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn it_supports_multiple_ports() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([single_port_service(123, 456)]);
        operator.reconcile(&state0, &empty_state()).unwrap();

        let state1 = state0.clone().with([service_with_ep(ExternalPorts {
            specs: vec![
                PortSpec::new_tcp(123, 456),
                PortSpec::new_tcp(321, 654),
            ],
        })]);
        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].port_spec, PortSpec::new_tcp(123, 456),);
        assert_eq!(rules[1].port_spec, PortSpec::new_tcp(321, 654),);
    }

    #[test]
    fn it_supports_udp() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([single_port_service(123, 456)]);
        operator.reconcile(&state0, &empty_state()).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].port_spec, single_port_spec(123, 456));

        let state1 = state0.clone().with([service_with_ep(ExternalPorts {
            specs: vec![PortSpec {
                host_port: 123,
                dest_port: 456,
                proto: Proto::Udp,
            }],
        })]);

        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);

        assert_eq!(
            rules[0].port_spec,
            PortSpec { host_port: 123, dest_port: 456, proto: Proto::Udp },
        );
    }

    fn empty_state() -> State {
        State::default().with(vec![Interface::new("eth0")]).with([Node {
            name: "foo".to_string(),
            addr: "bar".to_string(),
            is_active: true,
        }])
    }

    fn single_external_port(host_port: u16, node_port: u16) -> ExternalPorts {
        ExternalPorts { specs: vec![single_port_spec(host_port, node_port)] }
    }

    fn single_port_spec(host_port: u16, dest_port: u16) -> PortSpec {
        PortSpec::new_tcp(host_port, dest_port)
    }

    fn single_port_service(host_port: u16, node_port: u16) -> Service {
        service_with_ep(single_external_port(host_port, node_port))
    }

    fn service_with_ep(external_ports: ExternalPorts) -> Service {
        Service {
            name: "foo".to_string(),
            namespace: "bar".to_string(),
            external_ports,
            is_internal: false,
            allow_range: None,
        }
    }
}
