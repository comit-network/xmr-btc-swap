mod alice;
mod bob;

pub trait Persist {
    fn persist(&self);
}
