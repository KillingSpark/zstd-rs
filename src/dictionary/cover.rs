//! An implementation of the dictionary generation algorithm
//! described in the paper "Effective Construction of Relative Lempel-Ziv Dictionaries",
//! by Liao, Petri, Moffat, and Wirth, published under the University of Melbourne.
//!
//! See: https://people.eng.unimelb.edu.au/ammoffat/abstracts/lpmw16www.pdf
//!
//! Facebook's implementation was also used as a reference.
//! https://github.com/facebook/zstd/tree/dev/lib/dictBuilder

use crate::dictionary::frequency::compute_frequency;
use crate::dictionary::reservoir::create_sample;
use core::convert::TryInto;
use std::collections::HashMap;
use std::io::Cursor;
use std::vec::Vec;

/// The size of each k-mer
pub(super) const K: usize = 16;
///As found under "4: Experiments - Varying k-mer Size" in the original paper,
/// "when k = 16, across all our text collections, there is a reasonable spread"
///
/// Reasonable range: [6, 16]
pub(super) type KMer = [u8; K];

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
}

struct Segment {
    /// The actual contents of the segment.
    raw: Vec<u8>,
    /// A measure of how "ideal" a given segment would be to include in the dictionary
    ///
    /// Higher is better, there's no upper limit. This number is determined by
    /// estimating the number of occurances in a given epoch
    score: usize,
}

/// A re-usable allocation containing large allocations
/// that are used multiple times during dictionary construction (once per epoch)
struct Context {
    /// Keeps track of the number of occurances of a particular k-mer within an epoch.
    ///
    /// Reset for each epoch.
    frequencies: HashMap<KMer, usize>,
    /// A collection of segments to be used in the final dictionary.
    ///
    /// Contains the best segment from every epoch.
    pool: Vec<Segment>,
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
) -> Segment {
    let mut best_segment: &[u8] = &epoch[0..params.segment_size as usize];
    let mut top_segment_score: usize = 0;
    // Iterate over segments and score each segment, keeping track of the best segment
    for segment in epoch.chunks(params.segment_size as usize) {
        let segment_score = score_segment(ctx, epoch, segment);
        if segment_score > top_segment_score {
            best_segment = segment;
            top_segment_score = segment_score;
        }
    }

    Segment {
        raw: best_segment.into(),
        score: top_segment_score,
    }
}

/// Given a segment, compute the score (or usefulness) of that segment against the entire epoch.
///
/// `score_segment` modifies ctx.frequencies.
fn score_segment(ctx: &mut Context, epoch: &[u8], segment: &[u8]) -> usize {
    // Create a reservoir sample of the entire epoch
    // so we can estimate frequencies without checking the entire epoch
    // TODO: epoch size / 10 was chosen randomly, find a better way to determine reservoir size
    let epoch_sample = create_sample(&mut Cursor::new(epoch), epoch.len() / 10);

    let mut segment_score = 0;
    // Determine the score of each overlapping k-mer
    for i in 0..segment.len() - 1 {
        let kmer: &KMer = (&segment[i..i + K])
            .try_into()
            .expect("Failed to make kmer");
        // if the kmer is already in the pool, it recieves a score of zero
        if !ctx.frequencies.contains_key(kmer) {
            continue;
        }
        let kmer_score = compute_frequency(kmer, &epoch_sample);
        ctx.frequencies.insert(*kmer, kmer_score);
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
