//! Error type of `braceray-core`.
//!
//! Every variant's `Display` text is a complete, user-facing sentence that the app can show
//! as is. The underlying OS error (if any) is available through [`std::error::Error::source`].

use std::io;
use std::path::PathBuf;

/// A text encoding that Braceray can't read (only UTF-8 is supported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-16, little-endian (with a `FF FE` BOM or detected from the NUL pattern).
    Utf16Le,
    /// UTF-16, big-endian (with a `FE FF` BOM or detected from the NUL pattern).
    Utf16Be,
    /// UTF-32, little-endian.
    Utf32Le,
    /// UTF-32, big-endian.
    Utf32Be,
    /// Not text at all (many NUL or control bytes).
    Binary,
}

impl Encoding {
    /// Short name for messages, such as `"UTF-16 LE"`.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Utf32Le => "UTF-32 LE",
            Encoding::Utf32Be => "UTF-32 BE",
            Encoding::Binary => "binary",
        }
    }

    /// User-facing explanation, including how to convert the file where that's possible.
    pub fn message(self) -> String {
        let (iconv, powershell) = match self {
            Encoding::Utf16Le => ("UTF-16LE", "Unicode"),
            Encoding::Utf16Be => ("UTF-16BE", "BigEndianUnicode"),
            Encoding::Utf32Le => ("UTF-32LE", "UTF32"),
            Encoding::Utf32Be => ("UTF-32BE", "BigEndianUTF32"),
            Encoding::Binary => {
                return "This file looks like binary data, not JSON text.".to_owned();
            }
        };
        format!(
            "This file is {name}. Braceray supports UTF-8 JSON. Convert it with:\n\
             macOS:   iconv -f {iconv} -t UTF-8 in.json > out.json\n\
             Windows: Get-Content in.json -Encoding {powershell} | Set-Content out.json -Encoding utf8",
            name = self.name(),
        )
    }
}

/// Errors returned by `braceray-core`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file couldn't be opened or its metadata couldn't be read.
    #[error("Can't open {}: {source}", path.display())]
    Io {
        /// The path that was opened.
        path: PathBuf,
        /// The OS error.
        #[source]
        source: io::Error,
    },

    /// The path exists but is a directory or another non-regular file.
    #[error("{} is not a regular file.", path.display())]
    NotAFile {
        /// The path that was opened.
        path: PathBuf,
    },

    /// The file has zero bytes (after a UTF-8 BOM, if any).
    #[error("This file is empty.")]
    EmptyFile,

    /// The file contains only JSON whitespace (spaces, tabs, line breaks).
    #[error("This file contains only whitespace, no JSON value.")]
    WhitespaceOnly,

    /// The file is not UTF-8 text.
    #[error("{}", .0.message())]
    Encoding(Encoding),

    /// The first non-whitespace byte can't start a JSON value.
    #[error(
        "This doesn't look like JSON: it starts with {} at offset {offset}. \
         JSON starts with {{, [, \", a number, true, false or null.",
        describe_byte(*found)
    )]
    NotJson {
        /// Offset of the offending byte in the file content (after a BOM).
        offset: u64,
        /// The offending byte.
        found: u8,
    },

    /// The job was cancelled through its [`CancelToken`](crate::CancelToken).
    #[error("Cancelled.")]
    Cancelled,

    /// The file is larger than the index supports (offsets are stored in 40 bits).
    #[error("This file is larger than 1 TiB, which Braceray can't index.")]
    FileTooLarge,

    /// A background thread couldn't be started.
    #[error("Can't start a background thread: {source}")]
    Thread {
        /// The OS error.
        #[source]
        source: io::Error,
    },

    /// A background job stopped unexpectedly (a bug; please report it).
    #[error("The background job stopped unexpectedly. Please report this as a bug.")]
    JobPanicked,

    /// The search query can't be compiled (for example, far too long).
    #[error("Invalid search: {message}")]
    SearchQuery {
        /// What the engine reported.
        message: String,
    },

    /// The search asks for a feature that isn't available yet.
    #[error("{feature} search isn't available yet.")]
    SearchUnsupported {
        /// The feature, such as `"Regex"`.
        feature: &'static str,
    },

    /// Writing an export (or another output file) failed.
    #[error("Can't write {}: {source}", path.display())]
    Write {
        /// The file being written.
        path: PathBuf,
        /// The OS error.
        #[source]
        source: io::Error,
    },

    /// The OS refused to memory-map the file.
    #[error("Can't map this file (network drive?). Copy it locally. ({source})")]
    Mmap {
        /// The OS error.
        #[source]
        source: io::Error,
    },
}

/// Renders a byte for messages: `'x'` for printable ASCII, `0xNN` otherwise.
fn describe_byte(b: u8) -> String {
    if b.is_ascii_graphic() {
        format!("'{}'", b as char)
    } else {
        format!("byte 0x{b:02X}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_message_has_convert_commands() {
        let msg = Error::Encoding(Encoding::Utf16Le).to_string();
        assert!(msg.starts_with("This file is UTF-16 LE. Braceray supports UTF-8 JSON."));
        assert!(msg.contains("iconv -f UTF-16LE -t UTF-8"));
        assert!(msg.contains("-Encoding Unicode"));
    }

    #[test]
    fn not_json_message_shows_the_byte() {
        let e = Error::NotJson {
            offset: 3,
            found: b'<',
        };
        assert!(e.to_string().contains("starts with '<' at offset 3"));
        let e = Error::NotJson {
            offset: 0,
            found: 0x89,
        };
        assert!(e.to_string().contains("byte 0x89"));
    }
}
