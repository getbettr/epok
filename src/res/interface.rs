use std::any::TypeId;

use crate::ResourceLike;

pub type Interface = String;

impl ResourceLike for Interface {
    fn id(&self) -> String {
        self.to_owned()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Interface>()
    }

    fn is_active(&self) -> bool {
        true
    }
}
