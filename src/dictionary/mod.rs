//! Code for creating a separate content dictionary.
//!
//! Effective dictionaries are up to 1% the size of the complete training body,
//! and are trained on many examples of the original data.
//!
//! Implemented following the paper "Effective construction of
//! Relative Lempel-Ziv Dictionaries", by Kewen Liao, Matthias Petri,
//! Alistair Moffat, and Anthony Wirth

const GIBIBYTE: usize = 1 << 30;

// The algorithm is summarized here
// 1. The text is split into "epochs", or chunks from the original source
// 2. From within each epoch, we select the "segment", or 1 KiB contiguous section
//    that's predicted to be the best option to include in the dictionary. Concatenated,
//    these segments form the dictionary.
//
// This segment scoring algorithm operates as follows:
// For a given epoch:
//  - Run a reservoir sampler over the entire epoch, creating a
//    reservoir of n/t, where `t` is the desired number of occurances
//    we want the most common k-mers to have
//  - Have the ability to estimate
//    the frequency of a given k-mer: f(w: k-mer) calculates
//    the frequency of w in the reservoir using a rolling karp-rabin hash
//  - The score of a segment is the sum of `f(w)` called on every kmer within the segment
mod cover;
mod frequency;
mod reservoir;

use cover::*;
use std::io::{self, BufReader};

use crate::dictionary::reservoir::create_sample;

/// A set of values that are used during dictionary construction.
///
/// Changing these values can improve the resulting dictionary size for certain datasets.
pub struct DictParams {
    /// Segment size.
    ///
    /// As found under "4. Experiments - Varying Segment Size" in the original paper, a
    /// segment size of 2 kiB was effective.
    ///
    /// "We explored a range of [segment_size] values and found the performance of LMC is insensitive
    /// to [segment_size]. We fix [segment_size] to 2kiB
    ///
    /// Reasonable range: [16, 2048+]
    pub segment_size: u32,
}

/// Read from `source` to create a dictionary of `dict_size`. The completed dictionary is written
/// to `output`.
///
/// - `source` will be used as training data for the entire dictionary.
/// - `source_size` influences how the data is divided and sampled and is measured
///    in bytes. While this does not need to be exact, estimates should attempt to be
///    larger than the actual collection size.
/// - `output` is where the completed dictionary will be written.
/// - `dict_size` determines how large the complete dictionary should be. The completed
///   dictionary will be this size or smaller.
///
/// This function uses `BufRead` internally, the provided reader need not be buffered.
pub fn create_dict_from_source<R: io::Read, W: io::Write>(
    source: R,
    source_size: usize,
    output: &mut W,
    dict_size: usize,
) {
    let params = DictParams { segment_size: 2048 };
    let mut buffered_source = BufReader::new(source);
    let sample_size = buffered_source;
    let collection_sample = create_sample(&mut buffered_source, 2 * GIBIBYTE);
    // According to 4. Experiments - Varying Reservoir Sampler Thresholds,
    // setting reservoir size to collection size / min{collection size / 2 * number of segments,
    // 256} was effective
    let (epoch_size, num_epochs) = compute_epoch_info(params, dict_size, num_kmers);
}
