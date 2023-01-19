use std::str::FromStr;

use anyhow::anyhow;
use kube::ResourceExt;

use crate::{CoreService, ANNOTATION};
use super::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalPort {
    Spec { host_port: u16, node_port: u16 },
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
        let parts = s.split(':').collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(Self::Spec {
                host_port: parts[0].parse()?,
                node_port: parts[1].parse()?,
            }),
            _ => Err(anyhow!("unexpected number of annotation parts")),
        }
    }
}
