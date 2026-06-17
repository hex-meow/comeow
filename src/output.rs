//! Result type and human-friendly formatting for command outcomes.

use std::fmt;

use canopen_sdo::asynch::AsyncSdoError;
use canopen_sdo::{SdoAbortCode, SdoError};

/// The outcome of executing a single command.
#[derive(Debug, Clone)]
pub enum CmdResult {
    /// A write or `set`/`sleep` succeeded with nothing to print.
    Ok,
    /// A read produced a value (already formatted, e.g. `0x1018:01 = 1234`).
    Value(String),
    /// An informational line (e.g. `default node = 16`, help text).
    Info(String),
    /// The server or client aborted the transfer.
    Abort(SdoAbortCode),
    /// A CAN transport / I/O error.
    Io(String),
    /// Any other command error (parse, encode, protocol).
    Error(String),
}

impl CmdResult {
    pub fn from_async_err(e: AsyncSdoError) -> Self {
        match e {
            AsyncSdoError::Sdo(SdoError::ServerAborted(c))
            | AsyncSdoError::Sdo(SdoError::ClientAborted(c)) => CmdResult::Abort(c),
            AsyncSdoError::Sdo(other) => CmdResult::Error(other.to_string()),
            AsyncSdoError::Io(io) => CmdResult::Io(io.to_string()),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, CmdResult::Abort(_) | CmdResult::Io(_) | CmdResult::Error(_))
    }
}

impl fmt::Display for CmdResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CmdResult::Ok => write!(f, "OK"),
            CmdResult::Value(s) | CmdResult::Info(s) => write!(f, "{s}"),
            CmdResult::Abort(c) => write!(f, "ABORT {c} — {}", abort_help(*c)),
            CmdResult::Io(s) => write!(f, "I/O error: {s}"),
            CmdResult::Error(s) => write!(f, "error: {s}"),
        }
    }
}

/// Plain-English description for the common CiA 301 abort codes.
pub fn abort_help(code: SdoAbortCode) -> &'static str {
    use SdoAbortCode::*;
    match code {
        ToggleBitNotAlternated => "toggle bit not alternated",
        ProtocolTimeout => "no response from node within the SDO timeout",
        InvalidCommandSpecifier => "client/server command specifier not valid",
        OutOfMemory => "out of memory on the server",
        UnsupportedAccess => "unsupported access to this object",
        ReadWriteOnly => "attempt to read a write-only object",
        WriteReadOnly => "attempt to write a read-only object",
        ObjectDoesNotExist => "object does not exist in the dictionary",
        NotMappable => "object cannot be mapped to a PDO",
        PdoLengthExceeded => "mapped objects exceed the PDO length",
        ParameterIncompatibility => "general parameter incompatibility",
        InternalIncompatibility => "general internal incompatibility in the device",
        HardwareError => "access failed due to a hardware error",
        DataTypeLengthMismatch => "data type / length of service parameter does not match",
        DataTypeLengthHigh => "data type / length too high",
        DataTypeLengthLow => "data type / length too low",
        SubindexDoesNotExist => "sub-index does not exist",
        InvalidValue => "invalid value for the parameter",
        ValueTooHigh => "value of the parameter written too high",
        ValueTooLow => "value of the parameter written too low",
        MaxLessThanMin => "maximum value is less than minimum value",
        ResourceNotAvailable => "resource not available (SDO connection)",
        General => "general error",
        StorageError => "data cannot be transferred or stored",
        StorageLocalControl => "data cannot be stored due to local control",
        StorageDeviceState => "data cannot be stored due to the device state",
        NoObjectDictionary => "object dictionary not present or dynamic generation failed",
        NoData => "no data available",
        InvalidBlockSize | InvalidSequenceNumber | CrcError => "SDO block-transfer error",
        Unknown(_) => "unknown / non-standard abort code",
    }
}
