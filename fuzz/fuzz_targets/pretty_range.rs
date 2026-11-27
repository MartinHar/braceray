//! Arbitrary bytes → pretty rows from the start, from anchors at a few offsets, backwards,
//! and original-mode rows. Must not panic; rows must tile; for valid JSON, seeking from an
//! anchor and stepping back must give the rows of a full pass.
#![no_main]

use braceray_core::index::{self, IndexOptions, LineTable};
use braceray_core::nav::{Navigator, Span};
use braceray_core::pretty::lex::{LexState, tokenize};
use braceray_core::pretty::original::{next_row, prev_row, row_at};
use braceray_core::pretty::{
    Generator, PrettyOptions, PrettyPos, Seek, anchor_before, prev_line, seek,
};
use braceray_core::{CancelToken, Progress};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = PrettyOptions {
        wrap: 16,
        indent: 2,
    };
    let rows: Vec<_> = Generator::new(data, PrettyPos::START, &opts)
        .take(100_000)
        .collect();
    for w in rows.windows(2) {
        assert_eq!(w[0].next, w[1].pos);
        assert!(w[0].next.offset > w[0].pos.offset);
    }
    let Ok(index) = index::build_bytes(
        data,
        &IndexOptions::default(),
        &CancelToken::new(),
        &Progress::default(),
    ) else {
        return;
    };
    let nav = Navigator::new(data, &index);
    let valid = index.is_valid();
    for x in [0, data.len() / 4, data.len() / 2, data.len() * 3 / 4, data.len()] {
        let a = anchor_before(&nav, x as u64);
        assert!(a.offset as usize <= x);
        let got = seek(data, a, x as u64, usize::MAX, &opts);
        if valid && !rows.is_empty() {
            let want = rows
                .iter()
                .rposition(|r| r.pos.offset as usize <= x)
                .map_or(rows[0].pos, |i| rows[i].pos);
            assert_eq!(got, Seek::Found(want), "seek to {x}");
        }
    }
    if valid {
        for w in rows.windows(2).take(2000) {
            assert_eq!(
                prev_line(&nav, w[1].pos, usize::MAX, &opts),
                Some(Seek::Found(w[0].pos))
            );
        }
    } else if let Some(r) = rows.last() {
        let _ = prev_line(&nav, r.pos, usize::MAX, &opts);
    }
    if !data.is_empty() {
        let lines = LineTable::build(data);
        let mut r = row_at(data, &lines, 0, 16);
        let mut n = 0;
        while let Some(next) = next_row(data, &r, 16) {
            assert_eq!(prev_row(data, &lines, &next, 16), Some(r));
            assert_eq!(row_at(data, &lines, next.start, 16), next);
            r = next;
            n += 1;
            assert!(n <= data.len());
        }
        let mut sink = Vec::new();
        let all = Span {
            start: 0,
            end: data.len() as u64,
        };
        tokenize(data, all, LexState::default(), &mut sink);
    }
});
