use crate::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    VarIntTooLong,
    VarIntOverflow,
    FrameLengthNegative,
    FrameLengthTooLarge,
    StringLengthNegative,
    StringTooLong,
    InvalidUtf8,
    UnexpectedEof,
    TrailingBytes,
    UnknownPacketId { state: State, id: i32 },
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProtocolError::VarIntTooLong => write!(f, "varint too long"),
            ProtocolError::VarIntOverflow => write!(f, "varint overflow"),
            ProtocolError::FrameLengthNegative => write!(f, "frame length negative"),
            ProtocolError::FrameLengthTooLarge => write!(f, "frame length too large"),
            ProtocolError::StringLengthNegative => write!(f, "string length negative"),
            ProtocolError::StringTooLong => write!(f, "string too long"),
            ProtocolError::InvalidUtf8 => write!(f, "invalid utf8"),
            ProtocolError::UnexpectedEof => write!(f, "unexpected eof"),
            ProtocolError::TrailingBytes => write!(f, "trailing bytes"),
            ProtocolError::UnknownPacketId { state, id } => {
                write!(f, "unknown packet id {id} in state {state:?}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}
