use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    str::FromStr,
};

use anyhow::anyhow;

use crate::ANNOTATION;
use super::Error;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortSpec {
    pub host_port: u16,
    pub dest_port: u16,
    pub proto: Proto,
}

impl PortSpec {
    pub fn new_tcp(host_port: u16, dest_port: u16) -> Self {
        Self { host_port, dest_port, proto: Proto::Tcp }
    }
}

impl Display for PortSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "{}::{}::{:?}",
            self.host_port, self.dest_port, self.proto
        ))
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalPorts {
    pub specs: Vec<PortSpec>,
}

impl TryFrom<&BTreeMap<String, String>> for ExternalPorts {
    type Error = Error;

    fn try_from(anno: &BTreeMap<String, String>) -> Result<Self, Self::Error> {
        if anno.contains_key(ANNOTATION) {
            let annotation = anno[ANNOTATION].to_owned();
            anno[ANNOTATION].parse().map_err(|e| Error::AnnotationParseError {
                inner: e,
                annotation,
            })
        } else {
            Ok(Self::default())
        }
    }
}

impl FromStr for ExternalPorts {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ports = s
            .split(',')
            .map(|x| {
                let parts = x.split(':').collect::<Vec<_>>();
                match parts.len() {
                    pl @ 2..=3 => Ok(PortSpec {
                        host_port: parts[0].parse()?,
                        dest_port: parts[1].parse()?,
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

        Ok(Self { specs: ports.into_iter().map(|res| res.unwrap()).collect() })
    }
}
