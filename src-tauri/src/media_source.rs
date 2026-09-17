//! Byte-range semantics shared by original files and decoded ALAC PCM.
//!
//! HTTP-style byte ranges, clamped to the resource length, mirroring the
//! behavior the `flac://` protocol relied on: `start` is inclusive, `end` is
//! EXCLUSIVE, the complete request yields the whole resource, an open-ended
//! request extends to the end, a suffix request counts back from the end.
#[derive(Debug, PartialEq)]
pub struct Range { pub start: usize, pub end: usize, pub partial: bool }

pub fn select_range(header: Option<&str>, len: usize) -> Result<Range, ()> {
    let Some(h) = header else {
        return Ok(Range { start: 0, end: len, partial: false });
    };
    let spec = h.trim().strip_prefix("bytes=").ok_or(())?;
    // Multi-range requests are out of scope; anything with a comma is rejected
    // so the caller can answer 416 instead of guessing one part.
    if spec.contains(',') { return Err(()); }
    let (a, b) = spec.split_once('-').ok_or(())?;
    if a.is_empty() {
        // suffix form: last N bytes ("bytes=-3"). "-0" is unsatisfiable.
        let n: usize = b.trim().parse().map_err(|_| ())?;
        if n == 0 || n > len { return Err(()); }
        return Ok(Range { start: len - n, end: len, partial: true });
    }
    let start: usize = a.trim().parse().map_err(|_| ())?;
    let end = if b.is_empty() {
        len
    } else {
        // inclusive last byte in the header -> exclusive here, clamped to len
        b.trim().parse::<usize>().map_err(|_| ())?.saturating_add(1).min(len)
    };
    if start >= end.min(len).max(start) && start >= len { return Err(()); }
    if start >= len || start >= end { return Err(()); }
    Ok(Range { start, end, partial: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn complete() { assert_eq!(select_range(None, 10), Ok(Range{start:0,end:10,partial:false})); }
    #[test] fn bounded() { assert_eq!(select_range(Some("bytes=2-4"),10),Ok(Range{start:2,end:5,partial:true})); }
    #[test] fn open_ended() { assert_eq!(select_range(Some("bytes=2-"),10),Ok(Range{start:2,end:10,partial:true})); }
    #[test] fn suffix() { assert_eq!(select_range(Some("bytes=-3"),10),Ok(Range{start:7,end:10,partial:true})); }
    #[test] fn end_clamped() { assert_eq!(select_range(Some("bytes=2-99"),10),Ok(Range{start:2,end:10,partial:true})); }
    #[test] fn invalid_never_panics() {
        for h in ["bytes=8-2","bytes=10-","bytes=-0","bytes=1-2,4-5","bytes=x-y","bytes=999999999999999999999999-","bytes=-","units=1-2"] {
            assert!(select_range(Some(h),10).is_err(),"{h}");
        }
    }
    #[test] fn start_beyond_len() { assert!(select_range(Some("bytes=10-"),10).is_err()); }
}
