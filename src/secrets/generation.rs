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

    // -------------------- additional tests --------------------

    #[rstest::rstest]
    #[case("alnum")]
    #[case("alnum+symbols")]
    #[case("hex")]
    #[case("base64")]
    fn test_generate_secret_each_charset_returns_correct_length(#[case] charset: &str) {
        let s = generate_secret(32, charset);
        assert_eq!(
            s.chars().count(),
            32,
            "charset {} produced length {}",
            charset,
            s.chars().count()
        );
    }

    #[rstest::rstest]
    #[case(1)]
    #[case(8)]
    #[case(16)]
    #[case(24)]
    #[case(32)]
    #[case(64)]
    #[case(128)]
    fn test_generate_secret_returns_exact_length(#[case] length: u32) {
        let s = generate_secret(length, "alnum");
        assert_eq!(s.chars().count(), length as usize);
        assert_eq!(s.len(), length as usize);
    }

    #[test]
    fn test_generate_secret_alnum_only_contains_alphanumeric_chars() {
        for _ in 0..10 {
            let s = generate_secret(64, "alnum");
            for c in s.chars() {
                assert!(
                    c.is_ascii_alphanumeric(),
                    "alnum charset produced non-alphanumeric char: {:?}",
                    c
                );
            }
        }
    }

    #[test]
    fn test_generate_secret_hex_only_contains_hex_chars() {
        for _ in 0..10 {
            let s = generate_secret(64, "hex");
            for c in s.chars() {
                assert!(
                    "0123456789abcdef".contains(c),
                    "hex charset produced non-hex char: {:?}",
                    c
                );
            }
        }
    }

    #[test]
    fn test_generate_secret_base64_only_contains_base64_chars() {
        for _ in 0..10 {
            let s = generate_secret(64, "base64");
            for c in s.chars() {
                assert!(
                    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".contains(c),
                    "base64 charset produced non-base64 char: {:?}",
                    c
                );
            }
        }
    }

    #[test]
    fn test_generate_secret_alnum_plus_symbols_contains_only_allowed_chars() {
        let allowed = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;:,.<>?";
        for _ in 0..10 {
            let s = generate_secret(64, "alnum+symbols");
            for c in s.chars() {
                assert!(
                    allowed.contains(c),
                    "alnum+symbols charset produced unexpected char: {:?}",
                    c
                );
            }
        }
    }

    #[test]
    fn test_generate_secret_unknown_charset_falls_back_to_alnum() {
        let s = generate_secret(100, "totally-unknown-charset");
        for c in s.chars() {
            assert!(
                c.is_ascii_alphanumeric(),
                "unknown charset should fall back to alnum, got: {:?}",
                c
            );
        }
    }

    #[test]
    fn test_generate_secret_unknown_charset_empty_string_falls_back_to_alnum() {
        let s = generate_secret(50, "");
        for c in s.chars() {
            assert!(c.is_ascii_alphanumeric());
        }
    }

    #[test]
    fn test_generate_secret_length_zero_returns_empty_string() {
        let s = generate_secret(0, "alnum");
        assert_eq!(s, "");
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn test_generate_secret_length_zero_with_other_charsets_returns_empty() {
        let s1 = generate_secret(0, "hex");
        let s2 = generate_secret(0, "base64");
        let s3 = generate_secret(0, "alnum+symbols");
        let s4 = generate_secret(0, "unknown");
        assert_eq!(s1, "");
        assert_eq!(s2, "");
        assert_eq!(s3, "");
        assert_eq!(s4, "");
    }

    #[test]
    fn test_generate_secret_two_consecutive_calls_differ() {
        // 32 chars from a charset of 62 symbols gives ~190 bits of entropy;
        // collision probability is astronomically low.
        let s1 = generate_secret(32, "alnum");
        let s2 = generate_secret(32, "alnum");
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_generate_secret_length_1000_does_not_panic() {
        let s = generate_secret(1000, "alnum");
        assert_eq!(s.len(), 1000);
    }

    #[test]
    fn test_generate_secret_length_1000_hex_does_not_panic() {
        let s = generate_secret(1000, "hex");
        assert_eq!(s.len(), 1000);
        for c in s.chars() {
            assert!("0123456789abcdef".contains(c));
        }
    }

    #[test]
    fn test_generate_secret_hex_distribution_all_chars_appear() {
        // Generate 10000 single-char secrets with hex charset. With 16 hex
        // chars and uniform random sampling, the probability of any one char
        // NOT appearing is (15/16)^10000 ≈ 0 — essentially impossible.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10000 {
            let s = generate_secret(1, "hex");
            seen.insert(s);
        }
        // All 16 hex digits should appear at least once.
        for c in "0123456789abcdef".chars() {
            assert!(
                seen.contains(&c.to_string()),
                "hex char {:?} did not appear in 10000 samples (statistical anomaly)",
                c
            );
        }
        assert_eq!(seen.len(), 16, "expected all 16 hex chars to appear");
    }

    #[test]
    fn test_generate_secret_alnum_distribution_uppercase_appears() {
        // Generate 10000 single-char secrets with alnum charset; all 26
        // uppercase letters should appear at least once.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10000 {
            let s = generate_secret(1, "alnum");
            seen.insert(s);
        }
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars() {
            assert!(
                seen.contains(&c.to_string()),
                "uppercase letter {:?} did not appear in 10000 samples",
                c
            );
        }
    }

    #[test]
    fn test_generate_secret_alnum_distribution_lowercase_appears() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10000 {
            let s = generate_secret(1, "alnum");
            seen.insert(s);
        }
        for c in "abcdefghijklmnopqrstuvwxyz".chars() {
            assert!(
                seen.contains(&c.to_string()),
                "lowercase letter {:?} did not appear in 10000 samples",
                c
            );
        }
    }

    #[test]
    fn test_generate_secret_alnum_distribution_digits_appear() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10000 {
            let s = generate_secret(1, "alnum");
            seen.insert(s);
        }
        for c in "0123456789".chars() {
            assert!(
                seen.contains(&c.to_string()),
                "digit {:?} did not appear in 10000 samples",
                c
            );
        }
    }

    #[test]
    fn test_generate_secret_alnum_charset_has_62_symbols() {
        // Sanity check on the documented charset size.
        let alnum: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            .chars()
            .collect();
        assert_eq!(alnum.len(), 62);
    }

    #[test]
    fn test_generate_secret_hex_charset_has_16_symbols() {
        let hex: Vec<char> = "0123456789abcdef".chars().collect();
        assert_eq!(hex.len(), 16);
    }

    #[test]
    fn test_generate_secret_base64_charset_has_64_symbols() {
        let b64: Vec<char> =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
                .chars()
                .collect();
        assert_eq!(b64.len(), 64);
    }

    #[test]
    fn test_generate_secret_alnum_plus_symbols_charset_has_more_than_alnum() {
        let alnum: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            .chars()
            .collect();
        let alnum_sym: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;:,.<>?"
            .chars()
            .collect();
        assert!(alnum_sym.len() > alnum.len());
    }

    #[test]
    fn test_generate_secret_long_string_unicode_safe() {
        // The charsets only contain ASCII, so length-in-chars == length-in-bytes.
        let s = generate_secret(500, "alnum");
        assert_eq!(s.chars().count(), 500);
        assert_eq!(s.len(), 500);
    }
}
