use std::any::TypeId;

use crate::ResourceLike;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Interface {
    pub name: String,
    pub is_external: bool,
}

impl Interface {
    pub fn new<N: AsRef<str>>(name: N) -> Self {
        Self {
            name: name.as_ref().to_owned(),
            is_external: false,
        }
    }

    pub fn external(self) -> Self {
        Self {
            name: self.name,
            is_external: true,
        }
    }
}

impl ResourceLike for Interface {
    fn id(&self) -> String {
        self.name.to_owned()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Interface>()
    }

    fn is_active(&self) -> bool {
        true
    }
}
