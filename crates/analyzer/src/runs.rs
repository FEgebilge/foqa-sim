use std::ops::Range;

/// Maximal stretches of consecutive indices in `0..len` where `pred` holds:
/// airborne periods, exceedance episodes, low-altitude stretches.
pub fn runs(len: usize, mut pred: impl FnMut(usize) -> bool) -> impl Iterator<Item = Range<usize>> {
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < len && !pred(i) {
            i += 1;
        }
        let start = i;
        while i < len && pred(i) {
            i += 1;
        }
        (start < i).then_some(start..i)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_stretch_including_the_edges() {
        let held = [true, true, false, true, false, false, true];
        assert_eq!(
            runs(held.len(), |i| held[i]).collect::<Vec<_>>(),
            [0..2, 3..4, 6..7]
        );
        assert_eq!(runs(0, |_| true).count(), 0);
        assert_eq!(runs(3, |_| false).count(), 0);
    }
}
