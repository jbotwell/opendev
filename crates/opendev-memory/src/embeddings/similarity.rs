//! Similarity calculation functions and hashing utilities.

use sha2::{Digest, Sha256};

/// Calculate cosine similarity between two vectors.
///
/// Returns a value between -1.0 and 1.0:
/// - 1.0 = identical direction
/// - 0.0 = orthogonal
/// - -1.0 = opposite direction
pub fn cosine_similarity(vec1: &[f64], vec2: &[f64]) -> f64 {
    if vec1.len() != vec2.len() || vec1.is_empty() {
        return 0.0;
    }

    let dot: f64 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f64 = vec1.iter().map(|a| a * a).sum::<f64>().sqrt();
    let norm2: f64 = vec2.iter().map(|a| a * a).sum::<f64>().sqrt();

    if norm1 == 0.0 || norm2 == 0.0 {
        return 0.0;
    }

    let similarity = dot / (norm1 * norm2);
    similarity.clamp(-1.0, 1.0)
}

/// Calculate cosine similarity between a query vector and multiple vectors.
pub fn batch_cosine_similarity(query: &[f64], vectors: &[Vec<f64>]) -> Vec<f64> {
    vectors
        .iter()
        .map(|v| cosine_similarity(query, v))
        .collect()
}

/// Create a SHA-256 based cache key (first 16 hex chars).
pub(super) fn make_key(text: &str, model: &str) -> String {
    make_hash(&format!("{model}:{text}"))
}

/// SHA-256 hash truncated to 16 hex chars.
pub fn make_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8])
}

/// Inline hex encoding (avoids pulling in the `hex` crate just for this).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![0.0, 1.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![-1.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let v1 = vec![1.0, 2.0];
        let v2 = vec![0.0, 0.0];
        assert_eq!(cosine_similarity(&v1, &v2), 0.0);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let v1 = vec![1.0, 2.0];
        let v2 = vec![1.0];
        assert_eq!(cosine_similarity(&v1, &v2), 0.0);
    }

    #[test]
    fn test_batch_cosine_similarity() {
        let query = vec![1.0, 0.0];
        let vectors = vec![
            vec![1.0, 0.0],  // identical
            vec![0.0, 1.0],  // orthogonal
            vec![-1.0, 0.0], // opposite
        ];
        let results = batch_cosine_similarity(&query, &vectors);
        assert_eq!(results.len(), 3);
        assert!((results[0] - 1.0).abs() < 1e-10);
        assert!(results[1].abs() < 1e-10);
        assert!((results[2] - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn test_make_hash_deterministic() {
        let h1 = make_hash("test-model:hello");
        let h2 = make_hash("test-model:hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 8 bytes = 16 hex chars
    }

    #[test]
    fn test_make_hash_different_inputs() {
        let h1 = make_hash("a");
        let h2 = make_hash("b");
        assert_ne!(h1, h2);
    }
}
