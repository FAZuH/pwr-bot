use std::any::Any;

pub trait Event: Any + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
}
