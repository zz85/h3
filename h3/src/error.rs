//! HTTP/3 Error types

use std::fmt;

use crate::{frame, proto, qpack};

type Cause = Box<dyn std::error::Error + Send + Sync>;

/// A general error that can occur when handling the HTTP/3 protocol.
pub struct Error {
    inner: Box<ErrorImpl>,
}

/// An HTTP/3 "application error code".
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct Code(u64);

struct ErrorImpl {
    kind: Kind,
    cause: Option<Cause>,
}

enum Kind {
    Application {
        code: Code,
        reason: Option<Box<str>>,
    },
    Transport,
}

// ===== impl Code =====

macro_rules! codes {
    (
        $(
            $(#[$docs:meta])*
            ($num:expr, $name:ident);
        )+
    ) => {
        impl Code {
        $(
            $(#[$docs])*
            pub const $name: Code = Code($num);
        )+
        }

        impl fmt::Debug for Code {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.0 {
                $(
                    $num => f.write_str(stringify!($name)),
                )+
                    other => write!(f, "{:#x}", other),
                }
            }
        }
    }
}

codes! {
    /// No error. This is used when the connection or stream needs to be
    /// closed, but there is no error to signal.
    (0x100, H3_NO_ERROR);

    /// Peer violated protocol requirements in a way that does not match a more
    /// specific error code, or endpoint declines to use the more specific
    /// error code.
    (0x101, H3_GENERAL_PROTOCOL_ERROR);

    /// An internal error has occurred in the HTTP stack.
    (0x102, H3_INTERNAL_ERROR);

    /// The endpoint detected that its peer created a stream that it will not
    /// accept.
    (0x103, H3_STREAM_CREATION_ERROR);

    /// A stream required by the HTTP/3 connection was closed or reset.
    (0x104, H3_CLOSED_CRITICAL_STREAM);

    /// A frame was received that was not permitted in the current state or on
    /// the current stream.
    (0x105, H3_FRAME_UNEXPECTED);

    /// A frame that fails to satisfy layout requirements or with an invalid
    /// size was received.
    (0x106, H3_FRAME_ERROR);

    /// The endpoint detected that its peer is exhibiting a behavior that might
    /// be generating excessive load.
    (0x107, H3_EXCESSIVE_LOAD);

    /// A Stream ID or Push ID was used incorrectly, such as exceeding a limit,
    /// reducing a limit, or being reused.
    (0x108, H3_ID_ERROR);

    /// An endpoint detected an error in the payload of a SETTINGS frame.
    (0x109, H3_SETTINGS_ERROR);

    /// No SETTINGS frame was received at the beginning of the control stream.
    (0x10a, H3_MISSING_SETTINGS);

    /// A server rejected a request without performing any application
    /// processing.
    (0x10b, H3_REQUEST_REJECTED);

    /// The request or its response (including pushed response) is cancelled.
    (0x10c, H3_REQUEST_CANCELLED);

    /// The client's stream terminated without containing a fully-formed
    /// request.
    (0x10d, H3_REQUEST_INCOMPLETE);

    /// An HTTP message was malformed and cannot be processed.
    (0x10e, H3_MESSAGE_ERROR);

    /// The TCP connection established in response to a CONNECT request was
    /// reset or abnormally closed.
    (0x10f, H3_CONNECT_ERROR);

    /// The requested operation cannot be served over HTTP/3. The peer should
    /// retry over HTTP/1.1.
    (0x110, H3_VERSION_FALLBACK);

    /// The decoder failed to interpret an encoded field section and is not
    /// able to continue decoding that field section.
    (0x200, QPACK_DECOMPRESSION_FAILED);

    /// The decoder failed to interpret an encoder instruction received on the
    /// encoder stream.
    (0x201, QPACK_ENCODER_STREAM_ERROR);

    /// The encoder failed to interpret a decoder instruction received on the
    /// decoder stream.
    (0x202, QPACK_DECODER_STREAM_ERROR);
}

impl Code {
    pub(crate) fn with_reason<S: Into<Box<str>>>(self, reason: S) -> Error {
        Error::new(Kind::Application {
            code: self,
            reason: Some(reason.into()),
        })
    }

    pub(crate) fn with_cause<E: Into<Cause>>(self, cause: E) -> Error {
        Error::from(self).with_cause(cause)
    }
}

impl From<Code> for u64 {
    fn from(code: Code) -> u64 {
        code.0
    }
}

// ===== impl Error =====

impl Error {
    fn new(kind: Kind) -> Self {
        Error {
            inner: Box::new(ErrorImpl { kind, cause: None }),
        }
    }

    pub(crate) fn transport<E: Into<Cause>>(cause: E) -> Self {
        Error::transport_(cause.into())
    }

    fn transport_(cause: Cause) -> Self {
        Error::new(Kind::Transport).with_cause(cause)
    }

    pub(crate) fn with_cause<E: Into<Cause>>(mut self, cause: E) -> Self {
        self.inner.cause = Some(cause.into());
        self
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_struct("h3::Error");

        match self.inner.kind {
            Kind::Application { code, ref reason } => {
                builder.field("code", &code);
                if let Some(reason) = reason {
                    builder.field("reason", reason);
                }
            }
            Kind::Transport => {
                #[derive(Debug)]
                struct Transport;

                builder.field("kind", &Transport);
            }
        }

        if let Some(ref cause) = self.inner.cause {
            builder.field("cause", cause);
        }

        builder.finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner.kind {
            Kind::Application { code, ref reason } => {
                if let Some(reason) = reason {
                    write!(f, "application error: {}", reason)
                } else {
                    write!(f, "application error {:?}", code)
                }
            }
            Kind::Transport => f.write_str("quic transport error"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.cause.as_ref().map(|e| &**e as _)
    }
}

impl From<Code> for Error {
    fn from(code: Code) -> Error {
        Error::new(Kind::Application { code, reason: None })
    }
}

impl From<qpack::EncoderError> for Error {
    fn from(_e: qpack::EncoderError) -> Self {
        Code::QPACK_ENCODER_STREAM_ERROR.into() //.with_cause(e)
    }
}

impl From<qpack::DecoderError> for Error {
    fn from(_e: qpack::DecoderError) -> Self {
        Code::QPACK_DECODER_STREAM_ERROR.into() //.with_cause(e)
    }
}

impl From<proto::headers::Error> for Error {
    fn from(_e: proto::headers::Error) -> Self {
        Code::H3_MESSAGE_ERROR.into() //.with_cause(e)
    }
}

impl From<frame::Error> for Error {
    fn from(e: frame::Error) -> Self {
        match e {
            frame::Error::Quic(_e) => todo!(),
            frame::Error::Proto(_e) => Code::H3_FRAME_ERROR.into(),
            frame::Error::UnexpectedEnd => {
                Code::H3_FRAME_ERROR.with_reason("received incomplete frame")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use std::mem;

    #[test]
    fn test_size_of() {
        assert_eq!(mem::size_of::<Error>(), mem::size_of::<usize>());
    }
}
