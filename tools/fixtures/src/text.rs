//! Word pools for realistic records. Strings marked "escaped" are JSON string
//! bodies that already contain escapes; they cover every escape type
//! (`\" \\ \/ \b \f \n \r \t \uXXXX`, including a surrogate pair), several
//! scripts (Armenian, Cyrillic, CJK), emoji, and the characters `{ } [ ] : ,`.
//!
//! No pool may contain the substring `needle` in any case: needle counts
//! in manifests are computed by construction.

use crate::emit::write_u64;
use fastrand::Rng;

pub const FIRST_NAMES: &[&str] = &[
    "Anna",
    "Martin",
    "Lilit",
    "Aram",
    "Olga",
    "Dmitri",
    "Wei",
    "Yuki",
    "Sofia",
    "Liam",
    "José",
    "Zoë",
    "Արամ",
    "Անի",
    "Анна",
    "Сергей",
    "王伟",
    "さくら",
    "Chloé",
    "Nour",
];

pub const LAST_NAMES: &[&str] = &[
    "Petrosyan",
    "Harutyunyan",
    "Ivanova",
    "Smirnov",
    "Zhang",
    "Tanaka",
    "García",
    "O'Brien",
    "Müller",
    "Петров",
    "Սարգսյան",
    "李",
    "van der Berg",
    "Nakamura",
];

pub const EMAIL_NAMES: &[&str] = &[
    "anna", "martin", "lilit", "aram", "olga", "dmitri", "wei", "yuki", "sofia", "liam", "jose",
    "zoe", "chloe", "nour",
];

pub const DOMAINS: &[&str] = &["example.com", "example.org", "mail.example.net", "test.am"];

pub const COUNTRIES: &[&str] = &[
    "AM", "US", "DE", "FR", "JP", "CN", "RU", "BR", "IN", "GB", "NL", "CA",
];

/// Escaped tag bodies (some contain structural characters).
pub const TAGS: &[&str] = &[
    "vip",
    "b2b",
    "new",
    "returning",
    "wholesale",
    "{beta}",
    "a:b,c",
    "[promo]",
];

/// Escaped item titles.
pub const TITLES: &[&str] = &[
    r#"Café crème \"special\""#,
    "Widget [large]",
    "Gadget {pro}",
    "Cable: USB-C, 2m",
    "Ճաշատեսակ «Արարատ»",
    "Чайник электрический",
    "咖啡杯 (陶瓷)",
    r"Tab\tseparated\tvalues",
    r"Path C:\\temp\\file.txt",
    r"Slash \/ escaped",
    "Emoji 😀🎉 pack",
    r"Surrogate \ud83d\ude00 pair",
    r"Control \b\f\r chars",
    r"Unicode \u00e9\u0531\u4e2d\u0416",
    "Plain notebook",
    "Desk lamp, LED",
];

/// Escaped notes fragments.
pub const NOTES: &[&str] = &[
    r"line1\nline2 \\ backslash é 😀",
    r#"Deliver to: \"Main St, 5\" [rear door]"#,
    "{not json} [just text]",
    "Армения, Ереван",
    "東京都渋谷区",
    "Leave at the door",
    "",
    r"Call first\r\nthen knock",
    "Հաճախորդը խնդրեց արագ առաքում",
    r"key: value, list: [1, 2]",
];

pub const SERVICES: &[&str] = &["api", "auth", "billing", "search", "worker", "gateway"];
pub const LEVELS: &[&str] = &["DEBUG", "INFO", "INFO", "INFO", "WARN", "ERROR"];

/// Escaped log messages.
pub const MESSAGES: &[&str] = &[
    "request completed",
    r#"user \"admin\" logged in"#,
    "cache miss for key {user:42}",
    r"slow query:\n  SELECT * FROM orders WHERE id = 7",
    "Թարմացում ավարտված է",
    "Ошибка подключения к базе",
    "支付成功",
    r"path=C:\\logs\\app.log",
    "retrying in 5s [attempt 2/3]",
    "😀 all good",
];

/// Words for long string bodies (all escaped / safe).
pub const WORDS: &[&str] = &[
    "lorem", "ipsum", "dolor", "sit", "amet", "Բարև", "мир", "世界", "😀", r"\n", r"\t", r#"\""#,
    r"\\", r"\/", r"\u00e9", "{x}", "[y]", "a:b", "c,d", "data", "value", "stream",
];

pub fn pick<'a>(rng: &mut Rng, pool: &[&'a str]) -> &'a str {
    pool[rng.usize(..pool.len())]
}

/// Appends an RFC 4122-looking UUID (v4 layout) from the RNG.
pub fn write_uuid(rng: &mut Rng, out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let hi = rng.u64(..);
    let lo = rng.u64(..);
    let mut nibbles = [0u8; 32];
    for (k, nib) in nibbles.iter_mut().enumerate() {
        let src = if k < 16 { hi } else { lo };
        *nib = ((src >> ((k % 16) * 4)) & 0xf) as u8;
    }
    nibbles[12] = 4; // version 4
    nibbles[16] = 8 | (nibbles[16] & 3); // RFC 4122 variant
    for (k, nib) in nibbles.iter().enumerate() {
        if matches!(k, 8 | 12 | 16 | 20) {
            out.push(b'-');
        }
        out.push(HEX[*nib as usize]);
    }
}

fn two(out: &mut Vec<u8>, v: u32) {
    out.push(b'0' + (v / 10) as u8);
    out.push(b'0' + (v % 10) as u8);
}

/// Appends an ISO-8601 UTC timestamp in 2025–2026.
pub fn write_timestamp(rng: &mut Rng, out: &mut Vec<u8>) {
    write_u64(out, 2025 + rng.u64(..2));
    out.push(b'-');
    two(out, rng.u32(1..=12));
    out.push(b'-');
    two(out, rng.u32(1..=28));
    out.push(b'T');
    two(out, rng.u32(..24));
    out.push(b':');
    two(out, rng.u32(..60));
    out.push(b':');
    two(out, rng.u32(..60));
    out.push(b'Z');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_never_contain_needles() {
        let pools: &[&[&str]] = &[
            FIRST_NAMES,
            LAST_NAMES,
            EMAIL_NAMES,
            DOMAINS,
            COUNTRIES,
            TAGS,
            TITLES,
            NOTES,
            SERVICES,
            LEVELS,
            MESSAGES,
            WORDS,
        ];
        for pool in pools {
            for s in *pool {
                assert!(!s.to_lowercase().contains("needle"), "{s}");
            }
        }
    }

    #[test]
    fn escaped_pools_are_valid_json_strings() {
        for pool in [TAGS, TITLES, NOTES, MESSAGES, WORDS] {
            for s in pool {
                let quoted = format!("\"{s}\"");
                serde_json::from_str::<String>(&quoted).unwrap_or_else(|e| panic!("{s}: {e}"));
            }
        }
    }

    #[test]
    fn uuid_shape() {
        let mut rng = Rng::with_seed(1);
        let mut out = Vec::new();
        write_uuid(&mut rng, &mut out);
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.len(), 36);
        assert_eq!(&s[14..15], "4");
    }
}
