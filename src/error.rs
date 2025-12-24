// pub struct Error {
//     pub kind: ErrorKind,
//     pub message: String,
// }
//
// impl Error {
//     pub fn new(kind: ErrorKind, message: String) -> Self {
//         Self { kind, message }
//     }
//
//     pub fn kind(&self) -> &ErrorKind {
//         &self.kind
//     }
//
//     pub fn message(&self) -> &str {
//         &self.message
//     }
// }

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("Assertion error: {msg}")]
    AssertionError { msg: String },

    #[error("Missing config with key \"{key}\"")]
    MissingConfig { key: String },
}

pub enum AppErrorKind {
    AppError(AppError),
}
