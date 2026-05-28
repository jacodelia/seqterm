/// Generate an Euclidean rhythm using the Bjorklund algorithm.
///
/// # Arguments
/// * `pulses` – number of active beats (k).
/// * `total` – total number of steps (n).
///
/// Returns a `Vec<bool>` of length `total` where `true` indicates a pulse.
pub fn euclidean_rhythm(pulses: usize, total: usize) -> Vec<bool> {
    if total == 0 {
        return vec![];
    }
    let pulses = pulses.min(total);
    if pulses == 0 {
        return vec![false; total];
    }
    if pulses == total {
        return vec![true; total];
    }

    // Bjorklund algorithm via the "remainder" method.
    let mut pattern: Vec<Vec<bool>> = Vec::new();
    let mut remainder: Vec<Vec<bool>> = Vec::new();

    for _ in 0..pulses {
        pattern.push(vec![true]);
    }
    for _ in 0..(total - pulses) {
        remainder.push(vec![false]);
    }

    loop {
        let rem_len = remainder.len();
        if rem_len <= 1 {
            break;
        }
        let min_len = pattern.len().min(rem_len);
        let mut new_pattern: Vec<Vec<bool>> = Vec::with_capacity(min_len);
        for i in 0..min_len {
            let mut combined = pattern[i].clone();
            combined.extend_from_slice(&remainder[i]);
            new_pattern.push(combined);
        }
        let leftovers_p: Vec<Vec<bool>> = pattern[min_len..].to_vec();
        let leftovers_r: Vec<Vec<bool>> = remainder[min_len..].to_vec();
        pattern = new_pattern;
        remainder = if leftovers_p.is_empty() {
            leftovers_r
        } else if leftovers_r.is_empty() {
            leftovers_p
        } else {
            // The longer collection becomes the new remainder.
            if leftovers_p.len() >= leftovers_r.len() {
                let mut r = leftovers_r;
                r.extend(leftovers_p);
                r
            } else {
                let mut r = leftovers_p;
                r.extend(leftovers_r);
                r
            }
        };
    }

    let mut result: Vec<bool> = Vec::with_capacity(total);
    for seq in &pattern {
        result.extend_from_slice(seq);
    }
    for seq in &remainder {
        result.extend_from_slice(seq);
    }
    result.truncate(total);
    result
}

/// Rotate a rhythm pattern by `offset` steps to the right.
pub fn rotate(rhythm: &[bool], offset: usize) -> Vec<bool> {
    if rhythm.is_empty() {
        return vec![];
    }
    let n = rhythm.len();
    let offset = offset % n;
    let mut out = Vec::with_capacity(n);
    out.extend_from_slice(&rhythm[n - offset..]);
    out.extend_from_slice(&rhythm[..n - offset]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_e_3_8() {
        // E(3,8) = [1,0,0,1,0,0,1,0]
        let r = euclidean_rhythm(3, 8);
        assert_eq!(r.len(), 8);
        assert_eq!(r.iter().filter(|&&b| b).count(), 3);
    }

    #[test]
    fn test_e_5_16() {
        let r = euclidean_rhythm(5, 16);
        assert_eq!(r.len(), 16);
        assert_eq!(r.iter().filter(|&&b| b).count(), 5);
    }

    #[test]
    fn test_e_0_8() {
        let r = euclidean_rhythm(0, 8);
        assert!(r.iter().all(|&b| !b));
    }

    #[test]
    fn test_e_8_8() {
        let r = euclidean_rhythm(8, 8);
        assert!(r.iter().all(|&b| b));
    }
}
