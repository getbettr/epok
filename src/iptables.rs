use cmd_lib::{run_cmd, run_fun};
use tracing::info;

use crate::{ServiceExternalPort, SimpleService};

pub trait IptablesBackend {
    fn upsert(&mut self, sep: &ServiceExternalPort) -> anyhow::Result<()>;
    fn delete(&mut self, svc: &SimpleService) -> anyhow::Result<()>;
}

#[derive(Default, Debug)]
pub struct MemoryBackend {
    state: String,
}

impl IptablesBackend for MemoryBackend {
    fn upsert(&mut self, sep: &ServiceExternalPort) -> anyhow::Result<()> {
        let full_hash = sep.id();
        if self.state.lines().any(|l| l.contains(&full_hash)) {
            return Ok(());
        }

        self.delete(&sep.service)?;

        self.state = self.state.to_owned()
            + &format!(
                "\nsrc: {}, dest: {}, id: {}",
                sep.external_port.src, sep.external_port.nodeport, full_hash
            );
        Ok(())
    }

    fn delete(&mut self, svc: &SimpleService) -> anyhow::Result<()> {
        let svc_hash = svc.id();

        self.state = self
            .state
            .lines()
            .filter(|&l| !l.contains(&svc_hash))
            .map(String::from)
            .collect::<Vec<_>>()
            .join("\n");

        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct RealBackend {
    iface: String,
    node_ip: String,
}

impl RealBackend {
    pub fn new(iface: &str, node_ip: &str) -> Self {
        Self {
            iface: iface.to_owned(),
            node_ip: node_ip.to_owned(),
        }
    }
}

impl IptablesBackend for RealBackend {
    fn upsert(&mut self, sep: &ServiceExternalPort) -> anyhow::Result<()> {
        let state = run_fun!(sudo iptables-save -t nat)?;
        let full_hash = sep.id();

        // already there
        if state.lines().any(|l| l.contains(&full_hash)) {
            info!("rule for {:?} already good", &sep);
            return Ok(());
        }

        // delete rules that match the service
        self.delete(&sep.service)?;

        // insert a new rule
        let comment = format!(
            "src: {}, dest: {}, hash: {}",
            sep.external_port.src, sep.external_port.nodeport, full_hash
        );
        let (iface, dport, n_ip, n_port) = (
            &self.iface,
            sep.external_port.src,
            &self.node_ip,
            sep.external_port.nodeport,
        );
        info!("appending rules for {:?}", &sep);
        run_cmd!(
            sudo iptables -w -t nat -A PREROUTING -i $iface -p tcp -m tcp --dport $dport
            -m comment --comment $comment -j DNAT --to-destination $n_ip:$n_port
        )?;
        Ok(())
    }

    fn delete(&mut self, svc: &SimpleService) -> anyhow::Result<()> {
        let svc_hash = svc.id();
        if let Ok(rules) = run_fun!(sudo iptables-save -t nat | grep $svc_hash) {
            info!("deleting rules for {:?}", &svc);
            for rule in rules.lines() {
                let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
                rule_parts.remove(0);
                let cmd = format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "));
                run_cmd!(sh -c "$cmd")?;
            }
        }
        Ok(())
    }
}
