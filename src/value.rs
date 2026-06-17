//! CiA 309 datatype tokens: encode values for writes, format bytes for reads.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValueError {
    #[error("unknown datatype `{0}` (try: b u8 u16 u32 u64 i8 i16 i32 i64 r32 r64 vs hex)")]
    UnknownType(String),
    #[error("invalid boolean `{0}` (use 0/1, true/false, on/off)")]
    Bool(String),
    #[error("invalid integer `{0}`: {1}")]
    Int(String, String),
    #[error("value {0} out of range for {1}")]
    Range(String, &'static str),
    #[error("invalid float `{0}`: {1}")]
    Float(String, String),
    #[error("invalid hex bytes `{0}`: {1}")]
    Hex(String, String),
}

/// How an integer value should be rendered when read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radix {
    Dec,
    Hex,
}

/// The CiA 309 datatype tokens we support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    VisibleString,
    /// Raw bytes, written/printed as space-separated hex. Also covers the
    /// CiA 309 octet/unicode/domain types (`os`/`us`/`d`) on the wire.
    HexBytes,
}

/// Every datatype token we accept (used for parsing, completion, and
/// "still-typing" prefix detection in the live validator).
pub const TYPE_TOKENS: &[&str] = &[
    "b", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "x8", "x16", "x32", "x64", "r32",
    "r64", "vs", "hex",
];

impl DataType {
    /// Parse a datatype token, returning the type and the radix to print
    /// integers in (the `x*` tokens select hex display).
    pub fn parse_token(tok: &str) -> Result<(DataType, Radix), ValueError> {
        use DataType::*;
        use Radix::*;
        Ok(match tok {
            "b" | "bool" => (Bool, Dec),
            "u8" => (U8, Dec),
            "u16" => (U16, Dec),
            "u32" => (U32, Dec),
            "u64" => (U64, Dec),
            "x8" => (U8, Hex),
            "x16" => (U16, Hex),
            "x32" => (U32, Hex),
            "x64" => (U64, Hex),
            "i8" => (I8, Dec),
            "i16" => (I16, Dec),
            "i32" => (I32, Dec),
            "i64" => (I64, Dec),
            "r32" | "f32" => (F32, Dec),
            "r64" | "f64" => (F64, Dec),
            "vs" | "string" => (VisibleString, Dec),
            "hex" | "os" | "us" | "d" => (HexBytes, Dec),
            other => return Err(ValueError::UnknownType(other.to_string())),
        })
    }
}

/// Encode a textual value into little-endian wire bytes for an SDO write.
pub fn encode(ty: DataType, s: &str) -> Result<Vec<u8>, ValueError> {
    use DataType::*;
    Ok(match ty {
        Bool => match s {
            "1" | "true" | "on" | "True" => vec![1],
            "0" | "false" | "off" | "False" => vec![0],
            _ => return Err(ValueError::Bool(s.to_string())),
        },
        U8 => (parse_u(s, u8::MAX as u64, "u8")? as u8).to_le_bytes().to_vec(),
        U16 => (parse_u(s, u16::MAX as u64, "u16")? as u16).to_le_bytes().to_vec(),
        U32 => (parse_u(s, u32::MAX as u64, "u32")? as u32).to_le_bytes().to_vec(),
        U64 => parse_u(s, u64::MAX, "u64")?.to_le_bytes().to_vec(),
        I8 => (parse_i(s, i8::MIN as i64, i8::MAX as i64, "i8")? as i8).to_le_bytes().to_vec(),
        I16 => (parse_i(s, i16::MIN as i64, i16::MAX as i64, "i16")? as i16).to_le_bytes().to_vec(),
        I32 => (parse_i(s, i32::MIN as i64, i32::MAX as i64, "i32")? as i32).to_le_bytes().to_vec(),
        I64 => parse_i(s, i64::MIN, i64::MAX, "i64")?.to_le_bytes().to_vec(),
        F32 => s
            .parse::<f32>()
            .map_err(|e| ValueError::Float(s.to_string(), e.to_string()))?
            .to_le_bytes()
            .to_vec(),
        F64 => s
            .parse::<f64>()
            .map_err(|e| ValueError::Float(s.to_string(), e.to_string()))?
            .to_le_bytes()
            .to_vec(),
        // CANopen visible strings are not NUL-terminated.
        VisibleString => strip_quotes(s).as_bytes().to_vec(),
        HexBytes => parse_hex_bytes(s)?,
    })
}

/// Format raw little-endian bytes from an SDO read using the given type.
pub fn format(ty: DataType, radix: Radix, raw: &[u8]) -> String {
    use DataType::*;
    match ty {
        Bool => {
            if le_u(raw) != 0 {
                "true (1)".into()
            } else {
                "false (0)".into()
            }
        }
        U8 | U16 | U32 | U64 => {
            let v = le_u(raw);
            match radix {
                Radix::Hex => format!("0x{v:X}"),
                Radix::Dec => v.to_string(),
            }
        }
        I8 => (raw.first().copied().unwrap_or(0) as i8).to_string(),
        I16 => i16::from_le_bytes(fixed::<2>(raw)).to_string(),
        I32 => i32::from_le_bytes(fixed::<4>(raw)).to_string(),
        I64 => i64::from_le_bytes(fixed::<8>(raw)).to_string(),
        F32 => f32::from_le_bytes(fixed::<4>(raw)).to_string(),
        F64 => f64::from_le_bytes(fixed::<8>(raw)).to_string(),
        VisibleString => String::from_utf8_lossy(raw).trim_end_matches('\0').to_string(),
        HexBytes => hex_join(raw),
    }
}

/// Format read bytes when no datatype was given: raw hex plus a small-int hint.
pub fn format_raw(raw: &[u8]) -> String {
    let hex = hex_join(raw);
    if raw.is_empty() {
        return "<empty>".into();
    }
    if raw.len() <= 4 {
        let v = le_u(raw);
        format!("{} bytes: {hex}  (u{}=0x{v:X}, {v})", raw.len(), raw.len() * 8)
    } else {
        format!("{} bytes: {hex}", raw.len())
    }
}

// ---------- helpers ----------

/// Parse an unsigned integer (decimal, or `0x`/`0b`/`0o` prefixed), range-checked.
pub fn parse_u(s: &str, max: u64, ty: &'static str) -> Result<u64, ValueError> {
    let v = crate::command::parse_int_u64(s)
        .map_err(|e| ValueError::Int(s.to_string(), e.to_string()))?;
    if v > max {
        return Err(ValueError::Range(s.to_string(), ty));
    }
    Ok(v)
}

fn parse_i(s: &str, min: i64, max: i64, ty: &'static str) -> Result<i64, ValueError> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let mag = crate::command::parse_int_u64(body)
        .map_err(|e| ValueError::Int(s.to_string(), e.to_string()))? as i128;
    let v = if neg { -mag } else { mag };
    if v < min as i128 || v > max as i128 {
        return Err(ValueError::Range(s.to_string(), ty));
    }
    Ok(v as i64)
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, ValueError> {
    // Accept "DE AD BE EF", "de ad", or a contiguous "deadbeef".
    let compact: String = s.split_whitespace().collect();
    let compact = compact.strip_prefix("0x").unwrap_or(&compact);
    if compact.len() % 2 != 0 {
        return Err(ValueError::Hex(
            s.to_string(),
            "need an even number of hex digits".into(),
        ));
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    let bytes = compact.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let pair = &compact[i..i + 2];
        let b = u8::from_str_radix(pair, 16)
            .map_err(|e| ValueError::Hex(s.to_string(), e.to_string()))?;
        out.push(b);
        i += 2;
    }
    Ok(out)
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn le_u(raw: &[u8]) -> u64 {
    let mut b = [0u8; 8];
    let n = raw.len().min(8);
    b[..n].copy_from_slice(&raw[..n]);
    u64::from_le_bytes(b)
}

fn fixed<const N: usize>(raw: &[u8]) -> [u8; N] {
    let mut b = [0u8; N];
    let n = raw.len().min(N);
    b[..n].copy_from_slice(&raw[..n]);
    b
}

fn hex_join(raw: &[u8]) -> String {
    raw.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_u16() {
        let bytes = encode(DataType::U16, "1000").unwrap();
        assert_eq!(bytes, vec![0xE8, 0x03]);
        assert_eq!(format(DataType::U16, Radix::Dec, &bytes), "1000");
        assert_eq!(format(DataType::U16, Radix::Hex, &bytes), "0x3E8");
    }

    #[test]
    fn hex_input_u32() {
        let bytes = encode(DataType::U32, "0x04CE").unwrap();
        assert_eq!(bytes, vec![0xCE, 0x04, 0x00, 0x00]);
    }

    #[test]
    fn signed_negative() {
        let bytes = encode(DataType::I16, "-1").unwrap();
        assert_eq!(bytes, vec![0xFF, 0xFF]);
        assert_eq!(format(DataType::I16, Radix::Dec, &bytes), "-1");
    }

    #[test]
    fn overflow_rejected() {
        assert!(encode(DataType::U8, "256").is_err());
        assert!(encode(DataType::I8, "200").is_err());
    }

    #[test]
    fn visible_string() {
        let bytes = encode(DataType::VisibleString, "save").unwrap();
        assert_eq!(bytes, b"save");
        assert_eq!(format(DataType::VisibleString, Radix::Dec, &bytes), "save");
    }

    #[test]
    fn hex_bytes() {
        assert_eq!(encode(DataType::HexBytes, "DE AD BE EF").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(encode(DataType::HexBytes, "deadbeef").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn float_round_trip() {
        let bytes = encode(DataType::F32, "1.5").unwrap();
        assert_eq!(format(DataType::F32, Radix::Dec, &bytes), "1.5");
    }
}
