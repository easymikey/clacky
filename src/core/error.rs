//! The error type every session call returns.

use std::{error, fmt, io};

use crate::render::messages::Messages;

#[derive(Debug)]
#[non_exhaustive]
/// Why a question or an output call failed or was backed out of.
pub enum Error {
    /// Ctrl-C.
    Interrupted,
    /// Esc, or Ctrl-D on an empty line.
    Cancelled,
    /// Ended from outside, so [`CancelAsNone`](crate::core::ask::CancelAsNone)
    /// leaves it an error: a [`Stop`](crate::terminal::abort::Stop) was called
    /// or a timeout ran out.
    Aborted,
    /// The input ended before `question` was answered.
    NoInput {
        /// The label of the question.
        question: String,
    },
    /// Reading from or writing to the terminal failed.
    Terminal(io::Error),
    /// The terminal could not hide what is typed, so a secret would show.
    EchoNotHidden(io::Error),
    /// A choice prompt was given no options.
    NoChoices,
    /// A piped, given or default answer to `question` failed its check;
    /// `reason` is the check's text.
    Refused {
        /// The label of the question.
        question: String,
        /// Why the answer was refused.
        reason: String,
    },
    /// `question` has no default, and the session answers by defaults.
    NoDefault {
        /// The label of the question.
        question: String,
    },
    /// A spinner is running, so no question can be asked.
    Busy,
    /// Two choices of a key question use the same key.
    DuplicateKey(char),
}

impl Error {
    /// True for [`Interrupted`](Self::Interrupted) and
    /// [`Cancelled`](Self::Cancelled), where the person backed out; false for
    /// every failure.
    #[must_use]
    pub const fn is_cancel(&self) -> bool {
        match self {
            Self::Interrupted | Self::Cancelled => true,
            Self::Aborted
            | Self::NoInput { .. }
            | Self::NoChoices
            | Self::NoDefault { .. }
            | Self::Busy
            | Self::DuplicateKey(_)
            | Self::Terminal(_)
            | Self::EchoNotHidden(_)
            | Self::Refused { .. } => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&Messages::english().error(self))
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Terminal(cause) | Self::EchoNotHidden(cause) => Some(cause),
            Self::Interrupted
            | Self::Cancelled
            | Self::Aborted
            | Self::NoInput { .. }
            | Self::DuplicateKey(_)
            | Self::NoChoices
            | Self::NoDefault { .. }
            | Self::Busy
            | Self::Refused { .. } => None,
        }
    }
}

/// The result of a session call.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use std::{error::Error as _, io};

    use rstest::rstest;

    use super::Error;

    fn broken() -> io::Error {
        io::Error::other("pipe closed")
    }

    #[rstest]
    #[case::interrupted(Error::Interrupted, "interrupted")]
    #[case::cancelled(Error::Cancelled, "cancelled")]
    #[case::aborted(Error::Aborted, "aborted")]
    #[case::no_input(
        Error::NoInput { question: "Email".to_owned() },
        "no input: stdin ended before Email was given"
    )]
    #[case::no_choices(Error::NoChoices, "nothing to choose from")]
    #[case::no_default(
        Error::NoDefault { question: "Email".to_owned() },
        "Email: no default to answer with"
    )]
    #[case::busy(Error::Busy, "a spinner is running, stop it before asking")]
    #[case::duplicate_key(Error::DuplicateKey('a'), "two choices use the key a")]
    #[case::terminal(Error::Terminal(broken()), "the terminal stopped responding")]
    #[case::echo(
        Error::EchoNotHidden(broken()),
        "could not hide the input on this terminal, so the secret would be visible"
    )]
    #[case::invalid(
        Error::Refused { question: "Email".to_owned(), reason: "not an address".to_owned() },
        "Email: not an address"
    )]
    fn error_says_what_happened(#[case] error: Error, #[case] text: &str) {
        assert_eq!(error.to_string(), text);
    }

    #[rstest]
    #[case::terminal(Error::Terminal(broken()), Some("pipe closed"))]
    #[case::echo(Error::EchoNotHidden(broken()), Some("pipe closed"))]
    #[case::cancelled(Error::Cancelled, None)]
    #[case::aborted(Error::Aborted, None)]
    #[case::no_input(Error::NoInput { question: String::new() }, None)]
    fn only_io_errors_have_a_source(#[case] error: Error, #[case] cause: Option<&str>) {
        assert_eq!(error.source().map(ToString::to_string).as_deref(), cause);
    }

    #[rstest]
    #[case::interrupted(Error::Interrupted, true)]
    #[case::cancelled(Error::Cancelled, true)]
    #[case::aborted(Error::Aborted, false)]
    #[case::no_input(Error::NoInput { question: String::new() }, false)]
    #[case::no_choices(Error::NoChoices, false)]
    #[case::no_default(Error::NoDefault { question: String::new() }, false)]
    #[case::busy(Error::Busy, false)]
    #[case::duplicate_key(Error::DuplicateKey('a'), false)]
    #[case::terminal(Error::Terminal(broken()), false)]
    #[case::echo(Error::EchoNotHidden(broken()), false)]
    #[case::invalid(
        Error::Refused { question: String::new(), reason: String::new() },
        false
    )]
    fn is_cancel_is_true_only_when_the_person_backed_out(
        #[case] error: Error,
        #[case] cancel: bool,
    ) {
        assert_eq!(error.is_cancel(), cancel);
    }
}
