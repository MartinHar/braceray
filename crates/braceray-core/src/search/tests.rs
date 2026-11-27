use super::*;
use crate::job::no_repaint;

fn run(hay: &[u8], q: &SearchQuery, chunk: usize, cap: usize) -> SearchHandle {
    let opts = SearchOptions {
        chunk_size: chunk,
        max_stored_results: cap,
        threads: Some(4),
    };
    let h = start_search(Arc::new(hay.to_vec()), q, opts, no_repaint()).unwrap();
    h.wait();
    h
}

fn offsets(h: &SearchHandle) -> Vec<u64> {
    (0..h.stored()).map(|i| h.get(i).unwrap().offset).collect()
}

#[test]
fn empty_query_is_done_with_nothing() {
    let h = run(b"abc", &SearchQuery::new(""), 64, 10);
    assert!(h.is_done());
    assert_eq!(h.count(), 0);
    assert_eq!(h.next_after(0), None);
    assert_eq!(h.duration(), Some(Duration::ZERO));
}

#[test]
fn query_longer_than_the_file() {
    let h = run(b"ab", &SearchQuery::new("abc"), 1, 10);
    assert!(h.is_done());
    assert_eq!(h.count(), 0);
}

#[test]
fn regex_and_scope_are_not_supported_yet() {
    let bytes = Arc::new(b"x".to_vec());
    let q = SearchQuery {
        regex: true,
        ..SearchQuery::new("x")
    };
    let e = start_search(bytes.clone(), &q, SearchOptions::default(), no_repaint());
    assert!(matches!(e, Err(Error::SearchUnsupported { .. })));
    let q = SearchQuery {
        scope: Scope::Keys,
        ..SearchQuery::new("x")
    };
    let e = start_search(bytes, &q, SearchOptions::default(), no_repaint());
    assert!(matches!(e, Err(Error::SearchUnsupported { .. })));
}

#[test]
fn ordered_results_with_many_chunks() {
    let hay = b"needle hay ".repeat(1000);
    let h = run(&hay, &SearchQuery::new("NEEDLE"), 7, usize::MAX);
    assert!(h.is_done());
    assert_eq!(h.count(), 1000);
    let want: Vec<u64> = (0..1000).map(|i| i * 11).collect();
    assert_eq!(offsets(&h), want);
    assert_eq!(h.index_of_offset(22), Ok(2));
    assert_eq!(h.index_of_offset(23), Err(3));
    assert_eq!(h.progress(), 1.0);
    assert!(h.duration().is_some());
}

#[test]
fn next_and_prev_across_the_cap() {
    let hay = b"ab ".repeat(100); // matches at 0, 3, 6, …
    let h = run(&hay, &SearchQuery::new("ab"), 16, 10);
    assert_eq!(h.count(), 100);
    assert_eq!(h.stored(), 10);
    assert!(h.is_capped());
    let mut off = 0;
    let mut seen = vec![0];
    while let Some(m) = h.next_after(off) {
        seen.push(m.offset);
        off = m.offset;
    }
    assert_eq!(seen, (0..100).map(|i| i * 3).collect::<Vec<_>>());
    let mut back = vec![];
    let mut off = hay.len() as u64;
    while let Some(m) = h.prev_before(off) {
        back.push(m.offset);
        off = m.offset;
    }
    back.reverse();
    assert_eq!(back, seen);
    assert_eq!(h.next_after(1).map(|m| m.offset), Some(3));
    assert_eq!(h.prev_before(100).map(|m| m.offset), Some(99));
}

#[test]
fn whole_word_through_the_handle() {
    let q = SearchQuery {
        whole_word: true,
        ..SearchQuery::new("id")
    };
    let h = run(br#"{"id": 1, "idx": 2, "uuid": 3, "ID": 4}"#, &q, 3, 100);
    assert_eq!(offsets(&h), [2, 32]);
}
