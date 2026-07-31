use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use pulp::{Arch, Simd, WithSimd};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RankedCandidate {
    pub score: f32,
    pub document: u32,
}

impl PartialEq for RankedCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.document == other.document && self.score.total_cmp(&other.score) == Ordering::Equal
    }
}

impl Eq for RankedCandidate {}

impl PartialOrd for RankedCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            // A lower stable document ID wins an exact score tie.
            .then_with(|| other.document.cmp(&self.document))
    }
}

#[inline]
fn consider(
    heap: &mut BinaryHeap<Reverse<RankedCandidate>>,
    limit: usize,
    candidate: RankedCandidate,
) {
    if candidate.score <= 0.0 || limit == 0 {
        return;
    }
    if heap.len() < limit {
        heap.push(Reverse(candidate));
        return;
    }
    if heap.peek().is_some_and(|threshold| candidate > threshold.0) {
        let _ = heap.pop();
        heap.push(Reverse(candidate));
    }
}

fn finish(heap: &mut BinaryHeap<Reverse<RankedCandidate>>, output: &mut Vec<u32>) {
    output.clear();
    output.extend(heap.drain().map(|candidate| candidate.0.document));
}

pub(crate) fn retain_sparse(
    scores: &[f32],
    touched: &[u32],
    limit: usize,
    heap: &mut BinaryHeap<Reverse<RankedCandidate>>,
    output: &mut Vec<u32>,
) {
    heap.clear();
    for &document in touched {
        consider(
            heap,
            limit,
            RankedCandidate {
                score: scores[document as usize],
                document,
            },
        );
    }
    finish(heap, output);
}

pub(crate) fn retain_dense_simd(
    scores: &[f32],
    limit: usize,
    candidates: &mut Vec<RankedCandidate>,
    output: &mut Vec<u32>,
) {
    candidates.clear();
    Arch::new().dispatch(DenseCollect { scores, candidates });
    if candidates.len() > limit {
        candidates.select_nth_unstable_by(limit, |left, right| right.cmp(left));
        candidates.truncate(limit);
    }
    output.clear();
    output.extend(candidates.iter().map(|candidate| candidate.document));
}

struct DenseCollect<'a> {
    scores: &'a [f32],
    candidates: &'a mut Vec<RankedCandidate>,
}

impl WithSimd for DenseCollect<'_> {
    type Output = ();

    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) {
        let lanes = S::F32_LANES;
        let (packed, tail) = S::as_simd_f32s(self.scores);
        for (block, values) in packed.iter().copied().enumerate() {
            if simd.reduce_max_f32s(values) <= 0.0 {
                continue;
            }
            let start = block * lanes;
            for (lane, &score) in self.scores[start..start + lanes].iter().enumerate() {
                if score > 0.0 {
                    self.candidates.push(RankedCandidate {
                        score,
                        document: (start + lane) as u32,
                    });
                }
            }
        }
        let tail_start = self.scores.len() - tail.len();
        for (lane, &score) in tail.iter().enumerate() {
            if score > 0.0 {
                self.candidates.push(RankedCandidate {
                    score,
                    document: (tail_start + lane) as u32,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{retain_dense_simd, retain_sparse, RankedCandidate};
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    #[test]
    fn dense_and_sparse_select_the_same_stable_top_k() {
        let scores = [0.0, 5.0, 2.0, 5.0, 9.0, 0.5, 8.0, 8.0, 1.0];
        let touched = (0..scores.len() as u32).collect::<Vec<_>>();
        let mut heap = BinaryHeap::<Reverse<RankedCandidate>>::new();
        let mut sparse = Vec::new();
        retain_sparse(&scores, &touched, 4, &mut heap, &mut sparse);
        let mut dense = Vec::new();
        let mut candidates = Vec::new();
        retain_dense_simd(&scores, 4, &mut candidates, &mut dense);
        sparse.sort_unstable();
        dense.sort_unstable();
        assert_eq!(sparse, vec![1, 4, 6, 7]);
        assert_eq!(dense, sparse);
    }
}
