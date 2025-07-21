//! An implementation of the dictionary generation algorithm
//! described in the paper "Effective Construction of Relative Lempel-Ziv Dictionaries",
//! by Liao, Petri, Moffat, and Wirth, published under the University of Melbourne.
//!
//! See: https://people.eng.unimelb.edu.au/ammoffat/abstracts/lpmw16www.pdf
//!
//! Facebook's implementation was also used as a reference.
//! https://github.com/facebook/zstd/tree/dev/lib/dictBuilder

use std::collections::HashMap;
use std::vec::Vec;

use crate::dictionary::frequency::compute_frequency;

/// A set of values that are used during dictionary construction.
///
/// Changing these values can improve the resulting dictionary size for certain datasets.
struct DictParams {
    /// Segment size.
    ///
    /// As found under "4. Experiments - Varying Segment Size" in the original paper, a
    /// segment size of 2 kiB was effective.
    ///
    /// "We explored a range of [segment_size] values and found the performance of LMC is insensitive
    /// to [segment_size]. We fix [segment_size] to 2kiB
    ///
    /// Reasonable range: [16, 2048+]
    segment_size: u32,
    /// k-mer size
    ///
    ///As found under "4: Experiments - Varying k-mer Size" in the original paper,
    /// "when k = 16, across all our text collections, there is a reasonable spread"
    ///
    /// Reasonable range: [6, 16]
    ///
    /// For now this value is ignored, and globally set to 16.
    k: u32,
}

struct Segment {
    /// Relative to the beginning of the epoch,
    /// the index of the start of the segment
    starting_offset: u32,
    /// A measure of how "ideal" a given segment would be to include in the dictionary.
    score: u32,
}

/// A re-usable allocation containing large allocations
/// that are used multiple times during dictionary construction (once per epoch)
struct Context {
    /// Keeps track of the number of occurances of a particular k-mer
    frequencies: HashMap<[u8; 2], usize>,
    /// A collection of k-mers to be used in the final dictionary
    pool: Vec<[u8; 2]>,
}

impl Context {
    fn new() -> Self {
        Self {
            frequencies: HashMap::new(),
            pool: Vec::new(),
        }
    }
}

/// Returns the highest scoring segment in an epoch
/// as a slice of that epoch.
fn pick_best_segment<'epoch>(
    params: DictParams,
    ctx: &mut Context,
    epoch: &'epoch [u8],
) -> &'epoch [u8] {
    let mut best_segment: &[u8] = &epoch[0..params.segment_size as usize];
    let mut top_segment_score = 0;
    // Iterate over segments and score each segment, keeping track of the best segment
    for segment in epoch.chunks(params.segment_size as usize) {
        let segment_score = score_segment(ctx, epoch, segment);
        if segment_score > top_segment_score {
            best_segment = segment;
            top_segment_score = segment_score;
        }
    }

    best_segment
}

/// Given a segment, compute the score (or usefulness) of that segment against the entire epoch.
///
/// `score_segment` modifies ctx.frequencies.
fn score_segment(ctx: &mut Context, epoch: &[u8], segment: &[u8]) -> usize {
    let mut segment_score = 0;
    // Determine the score of each overlapping k-mer
    for i in 0..segment.len() - 1 {
        let kmer = [segment[i], segment[i + 1]];
        // if the kmer is already in the pool, it recieves a score of zero
        if !ctx.frequencies.contains_key(&kmer) {
            continue;
        }
        let kmer_score = compute_frequency(kmer, epoch);
        ctx.frequencies.insert(kmer, kmer_score);
        segment_score += kmer_score;
    }

    segment_score
}

/// Computes the number of epochs and the size of each epoch.
///
/// Returns a (number of epochs, epoch size) tuple.
///
/// A translation of `COVER_epoch_info_t COVER_computeEpochs()` from facebook/zstd.
fn compute_epoch_info(
    params: DictParams,
    max_dict_size: usize,
    num_kmers: usize,
) -> (usize, usize) {
    let min_epoch_size = 10_000; // 10 KiB
    let mut num_epochs: usize = usize::max(1, max_dict_size / params.segment_size as usize);
    let mut epoch_size: usize = num_kmers / num_epochs;
    if epoch_size >= min_epoch_size {
        assert!(epoch_size * num_epochs <= num_kmers);
        return (num_epochs, epoch_size);
    }
    epoch_size = usize::min(min_epoch_size, num_kmers);
    num_epochs = num_kmers / epoch_size;
    (num_epochs, epoch_size)
}
