use crate::{
    backend::Backend,
    res::{ExternalPort, Service, ServiceExternalPort},
    ANNOTATION,
};
use k8s_openapi::api::core::v1::Service as CoreService;
use std::str::FromStr;
use tracing::{debug, error, info};

pub struct Operator<B> {
    backend: B,
    interfaces: Vec<String>,
}

impl<B> Operator<B> {
    pub fn new(backend: B, interfaces: Vec<String>) -> Self {
        Self {
            backend,
            interfaces,
        }
    }
}

impl<B> Operator<B>
where
    B: Backend,
{
    pub fn apply(&mut self, s: &CoreService) -> anyhow::Result<()> {
        let svc = Service::from(s);

        if let Some(anno) = s.metadata.clone().annotations {
            if anno.contains_key(ANNOTATION) {
                info!("service changed: {:?}", &svc);
                if let Ok(ep) = ExternalPort::from_str(&anno[ANNOTATION]) {
                    for interface in self.interfaces.iter() {
                        let sep = ServiceExternalPort {
                            external_port: ep,
                            service: svc.with_iface(interface),
                        };
                        self.backend.upsert(&sep)?;
                    }
                } else {
                    error!(
                        "invalid annotation format '{}' for {:?}",
                        &anno[ANNOTATION], &svc
                    );
                }
            } else {
                // extraneous delete, but better safe than sorry
                debug!("missing annotation for: {:?} -> delete", &svc);
                for interface in self.interfaces.iter() {
                    self.backend.delete(&svc.with_iface(interface))?;
                }
            }
        } else {
            debug!("ignoring {:?} {reason}", &svc, reason = "no annotation");
        }
        Ok(())
    }

    pub fn delete(&mut self, s: &CoreService) -> anyhow::Result<()> {
        let svc = Service::from(s);
        info!("service deleted: {:?}", &svc);
        for interface in self.interfaces.iter() {
            self.backend.delete(&svc.with_iface(interface))?;
        }
        Ok(())
    }
}
