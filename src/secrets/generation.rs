//! Secret generation: random passwords for manifest-declared secrets.

use rand::Rng;

/// Generate a random secret of the given length using the given charset.
///
/// Charsets:
/// - `alnum` → [a-zA-Z0-9]
/// - `alnum+symbols` → [a-zA-Z0-9!@#$%^&*()_+-=[]{}|;:,.<>?]
/// - `hex` → [0-9a-f]
/// - `base64` → [A-Za-z0-9+/]
pub fn generate_secret(length: u32, charset: &str) -> String {
    let chars: Vec<char> = match charset {
        "alnum" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars().collect(),
        "alnum+symbols" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;:,.<>?".chars().collect(),
        "hex" => "0123456789abcdef".chars().collect(),
        "base64" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".chars().collect(),
        _ => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars().collect(),
    };

    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| chars[rng.gen_range(0..chars.len())])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_secret_length() {
        let s = generate_secret(24, "alnum");
        assert_eq!(s.len(), 24);
    }

    #[test]
    fn test_generate_secret_alnum_charset() {
        let s = generate_secret(100, "alnum");
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_secret_hex_charset() {
        let s = generate_secret(100, "hex");
        assert!(s.chars().all(|c| "0123456789abcdef".contains(c)));
    }

    #[test]
    fn test_generate_secret_uniqueness() {
        let s1 = generate_secret(32, "alnum");
        let s2 = generate_secret(32, "alnum");
        assert_ne!(s1, s2, "two generated secrets should not be equal");
    }
}
