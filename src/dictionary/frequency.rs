//! Contains `compute_frequency`, a function
//! that uses a rolling Karp-Rabin hash to
//! efficiently count the number of occurences
//! of a given k-mer within a set.

/// Computes a best effort guess as to how many times `pattern` occurs within
/// `body`. While not 100% accurate, it will be accurate the vast majority of time
pub(super) fn compute_frequency(pattern: [u8; 2], body: &[u8]) -> usize {
    assert!(body.len() >= pattern.len());
    // A prime number for modulo operations to reduce collisions (q)
    const PRIME: usize = 2654435761;
    // Number of characters in the input alphabet (d)
    const ALPHABET_SIZE: usize = 256;
    // Hash of input pattern (p)
    let mut input_hash: usize = 0;
    // Hash of the current window of text (t)
    let mut window_hash: usize = 0;
    // High-order digit multiplier (h)
    let mut h: usize = 1;

    // Precompute h (?)
    h = (h * ALPHABET_SIZE) % PRIME;

    // Compute initial hash values
    for i in 0..pattern.len() {
        input_hash = (ALPHABET_SIZE * input_hash + pattern[i] as usize) % PRIME;
        window_hash = (ALPHABET_SIZE * window_hash + body[i] as usize) % PRIME;
    }

    let mut num_occurances = 0;
    for i in 0..=body.len() - pattern.len() {
        // There's *probably* a match if these two match
        if input_hash == window_hash {
            num_occurances += 1;
        }

        // Compute hash values for next window
        if i < body.len() - pattern.len() {
            window_hash = (ALPHABET_SIZE * (window_hash - body[i] as usize * h)
                + body[i + pattern.len()] as usize)
                % PRIME;
        }
    }

    num_occurances
}

#[cfg(test)]
mod tests {
    use super::compute_frequency;
    #[test]
    fn dead_beef() {
        assert_eq!(
            compute_frequency([0xde, 0xad], &[0xde, 0xad, 0xbe, 0xef, 0xde, 0xad]),
            2
        );
    }

    #[test]
    fn smallest_body() {
        assert_eq!(compute_frequency([0x00, 0xff], &[0x00, 0xff]), 1);
    }

    #[test]
    fn no_match() {
        assert_eq!(
            compute_frequency([0xff, 0xff], &[0xde, 0xad, 0xbe, 0xef, 0xde, 0xad]),
            0
        );
    }
}
