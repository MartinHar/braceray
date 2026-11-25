//! `encoding`: BOM, UTF-16/32, empty, whitespace-only and binary files.

use crate::manifest::{EncodingCase, write_json};
use crate::records::record_seed;
use fastrand::Rng;
use std::io;
use std::path::Path;

const SAMPLE: &str =
    "{\"name\": \"Արամ\", \"city\": \"Ереван\", \"ok\": true, \"n\": [1, 2.5, -3e2]}\n";

/// (file, bytes, expected classification, description).
pub fn cases(seed: u64) -> Vec<(&'static str, Vec<u8>, &'static str, &'static str)> {
    let utf16le: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(SAMPLE.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let utf16be: Vec<u8> = [0xFE, 0xFF]
        .into_iter()
        .chain(SAMPLE.encode_utf16().flat_map(u16::to_be_bytes))
        .collect();
    let utf16le_nobom: Vec<u8> = SAMPLE.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let utf32le: Vec<u8> = [0xFF, 0xFE, 0x00, 0x00]
        .into_iter()
        .chain(SAMPLE.chars().flat_map(|c| (c as u32).to_le_bytes()))
        .collect();
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(SAMPLE.as_bytes());
    let mut rng = Rng::with_seed(record_seed(seed, 0xB1));
    // Random bytes with NULs and invalid UTF-8; starts with a PNG-like header.
    let mut binary = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    binary.extend((0..4096).map(|_| rng.u8(..)));
    vec![
        (
            "utf8_plain.json",
            SAMPLE.as_bytes().to_vec(),
            "utf8",
            "plain UTF-8, no BOM",
        ),
        (
            "bom_utf8.json",
            bom,
            "utf8-bom",
            "UTF-8 with BOM (EF BB BF); valid JSON after the BOM",
        ),
        (
            "utf16le.json",
            utf16le,
            "utf16le",
            "UTF-16 LE with BOM (FF FE)",
        ),
        (
            "utf16be.json",
            utf16be,
            "utf16be",
            "UTF-16 BE with BOM (FE FF)",
        ),
        (
            "utf16le_nobom.json",
            utf16le_nobom,
            "utf16le",
            "UTF-16 LE without BOM (detect by NUL pattern)",
        ),
        (
            "utf32le.json",
            utf32le,
            "utf32le",
            "UTF-32 LE with BOM (FF FE 00 00)",
        ),
        ("empty.json", Vec::new(), "empty", "zero bytes"),
        (
            "whitespace.json",
            b" \n\t\r\n   \n".to_vec(),
            "whitespace-only",
            "only JSON whitespace",
        ),
        (
            "binary.bin",
            binary,
            "binary",
            "random binary data with NUL bytes",
        ),
    ]
}

/// Writes all encoding fixtures and `encoding_manifest.json` into `dir`.
pub fn generate_encoding(dir: &Path, seed: u64) -> io::Result<Vec<EncodingCase>> {
    std::fs::create_dir_all(dir)?;
    let mut manifest = Vec::new();
    for (file, bytes, expect, description) in cases(seed) {
        std::fs::write(dir.join(file), bytes)?;
        manifest.push(EncodingCase {
            file: file.to_owned(),
            expect: expect.to_owned(),
            description: description.to_owned(),
        });
    }
    write_json(&dir.join("encoding_manifest.json"), &manifest)?;
    Ok(manifest)
}
