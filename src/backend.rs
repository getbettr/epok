use crate::{
    res::{Service, ServiceExternalPort},
    Executor,
};
use cmd_lib::run_fun;
use tracing::info;

pub trait Backend {
    fn upsert(&mut self, sep: &ServiceExternalPort) -> anyhow::Result<()>;
    fn delete(&mut self, svc: &Service) -> anyhow::Result<()>;
}

#[derive(Default, Debug)]
pub struct MemoryBackend {
    state: String,
}

impl Backend for MemoryBackend {
    fn upsert(&mut self, sep: &ServiceExternalPort) -> anyhow::Result<()> {
        let full_hash = sep.id();
        if self.state.lines().any(|l| l.contains(&full_hash)) {
            return Ok(());
        }

        self.delete(&sep.service)?;

        self.state = self.state.to_owned()
            + &format!(
                "\nsrc: {}, dest: {}, id: {}",
                sep.external_port.host_port, sep.external_port.node_port, full_hash
            );
        Ok(())
    }

    fn delete(&mut self, svc: &Service) -> anyhow::Result<()> {
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

pub struct IptablesBackend {
    node_ip: String,
    executor: Executor,
}

impl IptablesBackend {
    pub fn new(node_ip: &str, executor: Executor) -> Self {
        Self {
            node_ip: node_ip.to_owned(),
            executor,
        }
    }

    fn make_rule<S: AsRef<str>>(&self, sep: &ServiceExternalPort, comment: S) -> String {
        let cmd = format!(
            "sudo iptables -w -t nat -A PREROUTING -i {} -p tcp -m tcp --dport {} -m comment --comment '{}' -j DNAT --to-destination {}:{}",
            sep.service.iface.as_ref().unwrap(),
            sep.external_port.host_port,
            comment.as_ref(),
            &self.node_ip,
            sep.external_port.node_port
        );
        cmd
    }
}

impl Backend for IptablesBackend {
    fn upsert(&mut self, sep: &ServiceExternalPort) -> anyhow::Result<()> {
        let state = self.executor.run_fun("sudo iptables-save -t nat")?;
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
            "EpokRule{{ host_port: {}, node_port: {}, hash: {} }}",
            sep.external_port.host_port, sep.external_port.node_port, full_hash
        );
        info!("appending rules for {:?}", &sep);
        let cmd = self.make_rule(sep, comment);
        self.executor.run_fun(&cmd)?;
        Ok(())
    }

    fn delete(&mut self, svc: &Service) -> anyhow::Result<()> {
        let svc_hash = svc.id();
        if let Ok(rules) = self
            .executor
            .run_fun(&format!("sudo iptables-save -t nat | grep {}", svc_hash))
        {
            info!("deleting rules for {:?}", &svc);
            for rule in rules.lines() {
                let mut rule_parts = rule.split(' ').collect::<Vec<_>>();
                rule_parts.remove(0);
                let cmd = format!("sudo iptables -w -t nat -D {}", rule_parts.join(" "));
                self.executor.run_fun(&cmd)?;
            }
        }
        Ok(())
    }
}
