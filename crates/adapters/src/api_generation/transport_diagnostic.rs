//! Observational categories, never a remote execution/billing verdict. No
//! reqwest Display/source text, URL, headers, or response bytes are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportStage {
    Connect,
    ConnectTimeout,
    RequestSend,
    RequestTimeout,
    BodyRead,
    BodyTimeout,
}
impl TransportStage {
    pub(super) fn request_error(error: &reqwest::Error) -> Self {
        match (error.is_connect(), error.is_timeout()) {
            (true, true) => Self::ConnectTimeout,
            (true, false) => Self::Connect,
            (false, true) => Self::RequestTimeout,
            (false, false) => Self::RequestSend,
        }
    }
}
#[cfg(test)]
mod tests;
