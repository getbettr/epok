use std::str::FromStr;

use anyhow::anyhow;
use kube::ResourceExt;

use crate::{CoreService, ANNOTATION};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalPort {
    Spec { host_port: u16, node_port: u16 },
    Absent,
}

impl TryFrom<CoreService> for ExternalPort {
    type Error = anyhow::Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        let anno = cs.annotations();
        if anno.contains_key(ANNOTATION) {
            anno[ANNOTATION].parse()
        } else {
            Ok(ExternalPort::Absent)
        }
    }
}

impl FromStr for ExternalPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(':').collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(Self::Spec {
                host_port: parts[0].parse()?,
                node_port: parts[1].parse()?,
            }),
            _ => Err(anyhow!("failed to parse annotation")),
        }
    }
}
