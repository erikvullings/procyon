//! Deterministic, conservative token estimation.
//!
//! Chunk sizes must be reproducible on any machine, in any process, without
//! loading a tokenizer or knowing which embedding model will eventually be
//! used. The estimator below is therefore a *documented rule*, not an
//! approximation of one specific vocabulary, and it deliberately
//! over-estimates so a chunk sized against it never overflows a real
//! byte-pair tokenizer:
//!
//! 1. A run of ASCII alphanumerics (a word, an identifier, a number) costs
//!    `ceil(len / 3)` tokens. Byte-pair vocabularies typically average close
//!    to four characters per token for prose, so dividing by three is
//!    conservative for both prose and `snake_case`/`camelCase` identifiers.
//! 2. Every other non-whitespace character - punctuation, symbols, CJK
//!    ideographs, emoji - costs one token each. Real tokenizers frequently
//!    merge punctuation into neighbouring tokens and encode CJK at roughly one
//!    token per character, so this is again an upper bound.
//! 3. Whitespace is free, except that each line break costs one token, because
//!    newlines survive as their own token in most vocabularies.
//! 4. Non-empty text always costs at least one token.
//!
//! The rule is versioned by [`TOKEN_ESTIMATOR_VERSION`]; changing it changes
//! chunk boundaries, so it participates in the chunker version.

/// Version of the estimation rule documented in this module.
pub const TOKEN_ESTIMATOR_VERSION: u32 = 1;

/// Estimates - conservatively and deterministically - how many tokens `text`
/// would occupy. See the module documentation for the exact rule.
#[must_use]
pub fn estimate_tokens(text: &str) -> u32 {
    let mut tokens: u32 = 0;
    let mut run: u32 = 0;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            run += 1;
            continue;
        }
        tokens = tokens.saturating_add(run.div_ceil(3));
        run = 0;
        // A newline is charged like any other non-space character; other
        // whitespace is free.
        if character == '\n' || !character.is_whitespace() {
            tokens = tokens.saturating_add(1);
        }
    }
    tokens = tokens.saturating_add(run.div_ceil(3));
    if tokens == 0 && !text.is_empty() {
        return 1;
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_costs_nothing_and_whitespace_only_text_costs_one() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("   "), 1);
    }

    #[test]
    fn words_are_charged_by_ceiling_of_a_third_of_their_length() {
        assert_eq!(estimate_tokens("abc"), 1);
        assert_eq!(estimate_tokens("abcd"), 2);
        assert_eq!(estimate_tokens("hello world"), 2 + 2);
    }

    #[test]
    fn punctuation_and_newlines_each_cost_one_token() {
        assert_eq!(estimate_tokens("a,b"), 1 + 1 + 1);
        assert_eq!(estimate_tokens("abc\nabc"), 1 + 1 + 1);
    }

    #[test]
    fn non_ascii_scripts_cost_one_token_per_character() {
        assert_eq!(estimate_tokens("日本語"), 3);
    }

    #[test]
    fn the_estimate_is_stable_across_calls() {
        let sample = "The quick brown fox, jumps over 13 lazy dogs.\nSecond line.";
        assert_eq!(estimate_tokens(sample), estimate_tokens(sample));
    }
}
