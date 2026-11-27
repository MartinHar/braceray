//! Chunked scanning and stitching of the match chain.
//!
//! Matches are **non-overlapping, leftmost-first**: the chain from a position `p` is the
//! first acceptable match starting at or after `p`, then the chain from that match's end.
//! A chunk `[s, e)` is scanned on its own from `s` (its window reaches `max_len - 1`
//! bytes past `e`, so every match *starting* in the chunk is seen). When the previous
//! chunks' last match ends after `s` (it straddles the border), the true chain enters
//! the chunk at that end instead; [`stitch`] rescans from there until it meets a match of
//! the chunk's own chain, after which both chains are identical.

use crate::error::Error;
use crate::job::CancelToken;

use super::Match;
use super::engine::Engine;

/// Matches the worker keeps per chunk without asking the memory budget.
pub(crate) const FREE_HEAD: usize = 64;

/// What a worker found in one chunk, scanning from the chunk start.
#[derive(Debug, Default)]
pub(crate) struct ChunkScan {
    /// Number of matches starting in the chunk.
    pub count: u64,
    /// End of the last of them.
    pub last_end: Option<u64>,
    /// The first matches, in order (all of them if `complete`).
    pub head: Vec<Match>,
    /// `head.len() == count`.
    pub complete: bool,
    /// Budget entries reserved for `head` beyond [`FREE_HEAD`] (released after stitching).
    pub reserved: usize,
}

/// A chunk after stitching it onto the chain.
#[derive(Debug, Default)]
pub(crate) struct Stitched {
    /// Matches of the true chain starting in the chunk.
    pub count: u64,
    /// The carry for the next chunk: end of the last match so far (in any chunk).
    pub carry: u64,
    /// The chain's first matches in this chunk (at most the `room` asked for).
    pub stored: Vec<Match>,
    /// A position on the true chain at or before every match of the chunk:
    /// `max(carry_in, chunk start)`.
    pub chain_start: u64,
}

/// Read-only inputs of a scan.
#[derive(Clone, Copy)]
pub(crate) struct Scanner<'a> {
    pub engine: &'a Engine,
    pub bytes: &'a [u8],
    pub cancel: &'a CancelToken,
}

/// Cancel check interval, in matches.
const CANCEL_EVERY: u64 = 1024;

impl Scanner<'_> {
    /// Calls `f` for each match of the chain from `from` that starts before `end`, until
    /// `f` returns `false`.
    pub(crate) fn walk(
        &self,
        from: u64,
        end: u64,
        mut f: impl FnMut(Match) -> bool,
    ) -> Result<(), Error> {
        let len = self.bytes.len();
        let end = (end as usize).min(len);
        let limit = end.saturating_add(self.engine.max_len() - 1).min(len);
        let mut pos = from as usize;
        let mut n = 0u64;
        while pos < end {
            let Some((s, e)) = self.engine.find(self.bytes, pos, limit) else {
                break;
            };
            if s >= end {
                break;
            }
            let m = Match {
                offset: s as u64,
                len: (e - s) as u32,
            };
            if !f(m) {
                break;
            }
            pos = e;
            n += 1;
            if n.is_multiple_of(CANCEL_EVERY) {
                self.cancel.check()?;
            }
        }
        Ok(())
    }

    /// Scans chunk `[s, e)` from `s`. `grow()` is asked before the head grows past
    /// [`FREE_HEAD`] and every `block` entries after that; when it says no, only the
    /// count and the last end are kept.
    pub(crate) fn scan_chunk(
        &self,
        s: u64,
        e: u64,
        block: usize,
        mut grow: impl FnMut() -> bool,
    ) -> Result<ChunkScan, Error> {
        let mut scan = ChunkScan {
            complete: true,
            ..ChunkScan::default()
        };
        let mut allowed = FREE_HEAD;
        self.walk(s, e, |m| {
            scan.count += 1;
            scan.last_end = Some(m.end());
            if scan.complete {
                if scan.head.len() == allowed {
                    if grow() {
                        allowed += block;
                        scan.reserved += block;
                    } else {
                        scan.complete = false;
                    }
                }
                if scan.complete {
                    scan.head.push(m);
                }
            }
            true
        })?;
        Ok(scan)
    }

    /// Collects up to `limit` chain matches from `from` (a chain position) that start
    /// before `end`.
    fn collect(
        &self,
        from: u64,
        end: u64,
        limit: usize,
        out: &mut Vec<Match>,
    ) -> Result<(), Error> {
        let goal = out.len() + limit;
        if limit == 0 {
            return Ok(());
        }
        self.walk(from, end, |m| {
            out.push(m);
            out.len() < goal
        })
    }

    /// Joins chunk `[s, e)`, scanned from `s` as `scan`, onto the chain that so far ends
    /// at `carry`, keeping up to `room` of its matches.
    pub(crate) fn stitch(
        &self,
        s: u64,
        e: u64,
        carry: u64,
        scan: &ChunkScan,
        room: usize,
    ) -> Result<Stitched, Error> {
        let from = carry.max(s);
        let mut out = Stitched {
            carry,
            chain_start: from,
            ..Stitched::default()
        };
        if scan.count == 0 {
            // No acceptable match starts in [s, e), so none starts in [from, e) either.
            return Ok(out);
        }
        // Walk the true chain from `from` until it meets the chunk's own chain.
        let mut sync = if from == s { Some(0) } else { None };
        if sync.is_none() {
            let mut prefix = 0u64;
            self.walk(from, e, |m| {
                if let Ok(j) = scan.head.binary_search_by_key(&m.offset, |h| h.offset) {
                    sync = Some(j);
                    return false;
                }
                prefix += 1;
                out.carry = m.end();
                if out.stored.len() < room {
                    out.stored.push(m);
                }
                true
            })?;
            out.count = prefix;
        }
        let Some(j) = sync else {
            // Never met (only possible when the head is incomplete): the walk above
            // covered the whole chunk.
            return Ok(out);
        };
        // From head[j] on, the chains are the same.
        out.count += scan.count - j as u64;
        out.carry = scan.last_end.unwrap_or(out.carry);
        let need = room - out.stored.len();
        if need > 0 {
            let tail = &scan.head[j..];
            let take = tail.len().min(need);
            out.stored.extend_from_slice(&tail[..take]);
            let known = j + take;
            let still = room - out.stored.len();
            if still > 0 && (known as u64) < scan.count {
                // The head ran out: continue the chain after the last known match.
                let resume = scan.head.get(known - 1).map_or(from, Match::end);
                self.collect(resume, e, still, &mut out.stored)?;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chunks the haystack, scans each chunk on its own (with a tiny head) and stitches
    /// in order; returns (count, stored offsets).
    fn chunked(hay: &[u8], text: &str, ww: bool, chunk: u64, room: usize) -> (u64, Vec<u64>) {
        let engine = Engine::new(text, true, ww).unwrap().unwrap();
        let cancel = CancelToken::new();
        let sc = Scanner {
            engine: &engine,
            bytes: hay,
            cancel: &cancel,
        };
        let len = hay.len() as u64;
        let (mut carry, mut count, mut stored) = (0, 0, Vec::new());
        let mut s = 0;
        while s < len {
            let e = (s + chunk).min(len);
            let mut grants = 1; // the head may grow once past FREE_HEAD, then stops
            let scan = sc
                .scan_chunk(s, e, 2, || {
                    grants -= 1;
                    grants >= 0
                })
                .unwrap();
            let st = sc.stitch(s, e, carry, &scan, room - stored.len()).unwrap();
            count += st.count;
            carry = st.carry;
            stored.extend(st.stored.iter().map(|m| m.offset));
            s = e;
        }
        (count, stored)
    }

    fn naive(hay: &[u8], needle: &[u8]) -> Vec<u64> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + needle.len() <= hay.len() {
            if &hay[i..i + needle.len()] == needle {
                out.push(i as u64);
                i += needle.len();
            } else {
                i += 1;
            }
        }
        out
    }

    #[test]
    fn straddling_self_overlapping_needle() {
        let hay = vec![b'a'; 1001];
        for chunk in [1, 2, 3, 7, 64, 65, 1000] {
            let (count, stored) = chunked(&hay, "aaa", false, chunk, usize::MAX);
            let want = naive(&hay, b"aaa");
            assert_eq!(count, want.len() as u64, "chunk {chunk}");
            assert_eq!(stored, want, "chunk {chunk}");
        }
    }

    #[test]
    fn needle_longer_than_a_chunk() {
        let hay = b"xxabcabcabcabcabcabcyy abcabcabc".repeat(5);
        for chunk in [1, 2, 4, 5] {
            let (count, stored) = chunked(&hay, "abcabc", false, chunk, usize::MAX);
            assert_eq!(stored, naive(&hay, b"abcabc"), "chunk {chunk}");
            assert_eq!(count, stored.len() as u64);
        }
    }

    #[test]
    fn room_limits_stored_but_not_count() {
        let hay = vec![b'a'; 500];
        let (count, stored) = chunked(&hay, "aa", false, 33, 10);
        assert_eq!(count, 250);
        assert_eq!(stored, (0..10).map(|i| i * 2).collect::<Vec<_>>());
    }
}
