use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use anyhow::anyhow;
use kube::ResourceExt;

use crate::{CoreService, ANNOTATION};
use super::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortSpec {
    pub host_port: u16,
    pub node_port: u16,
    pub proto: Proto,
}

impl PortSpec {
    pub fn new_tcp(host_port: u16, node_port: u16) -> Self {
        Self { host_port, node_port, proto: Proto::Tcp }
    }
}

impl Display for PortSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "{}::{}::{:?}",
            self.host_port, self.node_port, self.proto
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalPort {
    Specs(Vec<PortSpec>),
    Absent,
}

impl TryFrom<CoreService> for ExternalPort {
    type Error = Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        let anno = cs.annotations();
        if anno.contains_key(ANNOTATION) {
            let annotation = anno[ANNOTATION].to_owned();
            anno[ANNOTATION].parse().map_err(|e| Error::ServiceParseError {
                inner: e,
                annotation,
                service_id: format!(
                    "{}/{}",
                    cs.namespace().unwrap_or_default(),
                    cs.name_any()
                ),
            })
        } else {
            Ok(ExternalPort::Absent)
        }
    }
}

impl FromStr for ExternalPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ports = s
            .split(',')
            .map(|x| {
                let parts = x.split(':').collect::<Vec<_>>();
                match parts.len() {
                    pl @ 2..=3 => Ok(PortSpec {
                        host_port: parts[0].parse()?,
                        node_port: parts[1].parse()?,
                        proto: {
                            if pl == 3 && parts[2] == "udp" {
                                Proto::Udp
                            } else {
                                Proto::Tcp
                            }
                        },
                    }),
                    _ => Err(anyhow!("unexpected number of annotation parts")),
                }
            })
            .collect::<Vec<_>>();

        if ports.is_empty() {
            return Err(anyhow!("malformed port spec"));
        }

        if ports.iter().any(|port| port.is_err()) {
            return Err(anyhow!("malformed port spec"));
        }

        Ok(Self::Specs(ports.into_iter().map(|res| res.unwrap()).collect()))
    }
}
