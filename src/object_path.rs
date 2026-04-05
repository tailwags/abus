#[derive(Debug)]
#[repr(transparent)]
pub struct ObjectPath {
    pub inner: String,
}
