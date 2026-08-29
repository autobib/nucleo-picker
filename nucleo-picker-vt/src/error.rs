use std::{error::Error as StdError, fmt};

use nucleo_picker::{error::PickError, event::PickerStatus};

#[derive(Debug)]
pub(crate) enum FailureContext {
    LastStatus(PickerStatus),
    RequestedDimensions((u16, u16)),
    Checkpoint(String),
}

#[derive(Debug)]
pub enum ErrorKind {
    Timeout,
    Disconnected,
    Picker(PickError),
    Terminal(libghostty_vt::error::Error),
    Inspection(String),
}

#[derive(Debug)]
pub struct Error {
    context: Vec<FailureContext>,
    kind: ErrorKind,
}

impl ErrorKind {
    pub(crate) fn with_context(self, context: impl IntoIterator<Item = FailureContext>) -> Error {
        Error {
            context: context.into_iter().collect(),
            kind: self,
        }
    }

    pub(crate) fn with_driver_context(
        self,
        last_status: Option<&PickerStatus>,
        requested_dimensions: (u16, u16),
    ) -> Error {
        let mut context = Vec::with_capacity(2);
        if let Some(status) = last_status {
            context.push(FailureContext::LastStatus(status.clone()));
        }
        context.push(FailureContext::RequestedDimensions(requested_dimensions));
        self.with_context(context)
    }
}

impl Error {
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub(crate) fn with_context(mut self, context: FailureContext) -> Self {
        self.context.push(context);
        self
    }

    pub(crate) fn with_checkpoint(self, checkpoint: impl Into<String>) -> Self {
        self.with_context(FailureContext::Checkpoint(checkpoint.into()))
    }
}

impl fmt::Display for FailureContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LastStatus(status) => write!(f, "last status: {status:?}"),
            Self::RequestedDimensions((cols, rows)) => {
                write!(f, "requested dimensions: {cols}x{rows}")
            }
            Self::Checkpoint(checkpoint) => write!(f, "checkpoint: {checkpoint}"),
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("operation timed out"),
            Self::Disconnected => f.write_str("driver disconnected"),
            Self::Picker(error) => write!(f, "picker failed: {error}"),
            Self::Terminal(error) => write!(f, "terminal failed: {error}"),
            Self::Inspection(message) => write!(f, "inspection failed: {message}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)?;
        for context in &self.context {
            write!(f, "\n  {context}")?;
        }
        Ok(())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.kind {
            ErrorKind::Picker(error) => Some(error),
            ErrorKind::Terminal(error) => Some(error),
            ErrorKind::Timeout | ErrorKind::Disconnected | ErrorKind::Inspection(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::*;

    #[test]
    fn context_is_formatted_as_an_indented_chain() -> Result<(), Box<dyn StdError>> {
        let error = ErrorKind::Timeout
            .with_context([FailureContext::RequestedDimensions((60, 16))])
            .with_checkpoint("initial")
            .with_checkpoint("nested");

        assert_eq!(
            error.to_string(),
            "operation timed out\n  requested dimensions: 60x16\n  checkpoint: initial\n  checkpoint: nested"
        );
        Ok(())
    }
}
