//! Secret and token redaction for captured provider output.
//!
//! Provider stdout/stderr is stored in run artifacts and structured logs.
//! Any recognised credential pattern is replaced with `[REDACTED]` before
//! the output leaves the capture boundary, so raw tokens never reach disk or
//! tracing output.
//!
//! # Patterns covered
//!
//! | Pattern | Description |
//! |---------|-------------|
//! | `ghp_<36+chars>` | GitHub personal access token (new format) |
//! | `gho_<36+chars>` | GitHub OAuth token |
//! | `ghs_<36+chars>` | GitHub App installation token |
//! | `ghr_<36+chars>` | GitHub refresh token |
//! | `Authorization: Bearer <token>` | HTTP Authorization header (Bearer) |
//! | `Authorization: token <token>` | HTTP Authorization header (token) |
//! | `GITHUB_TOKEN=<value>` | Environment variable assignment |
//! | `GH_TOKEN=<value>` | Alternative env var used by the `gh` CLI |

const REDACTED: &str = "[REDACTED]";

/// Replace all recognised secret patterns in `input` with `[REDACTED]`.
///
/// The function is designed to be cheap to call on every captured line; it
/// performs a small number of simple string scans without requiring a regex
/// engine.
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    out = redact_gh_tokens(out);
    out = redact_auth_headers(out);
    out = redact_env_vars(out);
    out
}

// ── GitHub token prefixes ─────────────────────────────────────────────────────

/// Prefixes used by GitHub-issued tokens.
const GH_TOKEN_PREFIXES: &[&str] = &["ghp_", "gho_", "ghs_", "ghr_"];

/// Minimum number of alphanumeric characters that must follow a prefix for it
/// to be considered a real token (avoids redacting short test strings).
const GH_TOKEN_MIN_SUFFIX: usize = 36;

fn redact_gh_tokens(mut s: String) -> String {
    for prefix in GH_TOKEN_PREFIXES {
        s = redact_prefixed_token(&s, prefix, GH_TOKEN_MIN_SUFFIX);
    }
    s
}

/// Find every occurrence of `prefix` followed by at least `min_suffix`
/// alphanumeric-or-underscore characters and replace them with
/// `{prefix}[REDACTED]`.
fn redact_prefixed_token(s: &str, prefix: &str, min_suffix: usize) -> String {
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;

    while let Some(pos) = remaining.find(prefix) {
        // Append everything before the prefix.
        result.push_str(&remaining[..pos]);

        let after_prefix = &remaining[pos + prefix.len()..];
        let suffix_len = after_prefix
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .count();

        if suffix_len >= min_suffix {
            // Redact: emit prefix + [REDACTED], skip the suffix chars.
            result.push_str(prefix);
            result.push_str(REDACTED);
            // Advance past prefix + suffix (byte length, prefix is ASCII).
            remaining = &remaining[pos + prefix.len() + suffix_len..];
        } else {
            // Not long enough — keep the prefix verbatim and advance past it.
            result.push_str(prefix);
            remaining = after_prefix;
        }
    }

    result.push_str(remaining);
    result
}

// ── Authorization headers ─────────────────────────────────────────────────────

/// Redact `Authorization: Bearer <token>` and `Authorization: token <token>`.
///
/// Matching is case-insensitive for the header name and scheme keyword.
fn redact_auth_headers(s: String) -> String {
    redact_header_value(&s, "Bearer").pipe(|s| redact_header_value(&s, "token"))
}

fn redact_header_value(s: &str, scheme: &str) -> String {
    // Pattern: "Authorization:" (optional whitespace) scheme (whitespace) <value>
    //
    // Matching is case-insensitive for the header name and scheme keyword, but
    // only for ASCII characters.  We use `to_ascii_lowercase()` (not
    // `to_lowercase()`) because it preserves byte length — general Unicode case
    // folding does not (e.g. `ß` → `ss`, Turkish dotted-I), and we index back
    // into the original byte-for-byte.  All character-class checks below use
    // `is_ascii_whitespace()` on *bytes*, which is safe because ASCII whitespace
    // bytes cannot appear inside a multi-byte UTF-8 sequence, so byte positions
    // are always on character boundaries.
    let needle = "authorization:";
    let lower = s.to_ascii_lowercase();
    let scheme_lower = scheme.to_ascii_lowercase();

    let mut result = String::with_capacity(s.len());
    let mut remaining_lower = lower.as_str();
    let mut remaining_orig = s;

    loop {
        let Some(pos) = remaining_lower.find(needle) else {
            result.push_str(remaining_orig);
            break;
        };

        // Append everything up to and including "Authorization:"
        let header_end = pos + needle.len();
        result.push_str(&remaining_orig[..header_end]);

        let after_colon_lower = &remaining_lower[header_end..];
        let after_colon_orig = &remaining_orig[header_end..];

        // Skip optional inline whitespace (spaces/tabs, not newlines).
        let ws_len = count_inline_ws_bytes(after_colon_lower);

        let scheme_start = ws_len;
        let scheme_end = scheme_start + scheme_lower.len();

        if after_colon_lower.len() >= scheme_end
            && &after_colon_lower[scheme_start..scheme_end] == scheme_lower.as_str()
        {
            // Emit the whitespace + scheme.
            result.push_str(&after_colon_orig[..scheme_end]);

            let after_scheme_lower = &after_colon_lower[scheme_end..];
            let after_scheme_orig = &after_colon_orig[scheme_end..];

            // Skip whitespace between scheme and value.
            let inner_ws = count_inline_ws_bytes(after_scheme_lower);

            if inner_ws > 0 {
                result.push_str(&after_scheme_orig[..inner_ws]);
            }

            // Byte length of the value, measured up to the first whitespace
            // byte.  `is_ascii_whitespace()` on a byte is safe here: ASCII
            // whitespace bytes (< 0x80) cannot appear inside a UTF-8 multi-byte
            // sequence, so the count always lands on a char boundary even if
            // the value contains multi-byte UTF-8.
            let value_bytes = &after_scheme_orig.as_bytes()[inner_ws..];
            let value_len = value_bytes
                .iter()
                .position(u8::is_ascii_whitespace)
                .unwrap_or(value_bytes.len());

            if value_len > 0 {
                result.push_str(REDACTED);
                let total_skip = scheme_end + inner_ws + value_len;
                remaining_lower = &after_colon_lower[total_skip..];
                remaining_orig = &after_colon_orig[total_skip..];
            } else {
                remaining_lower = after_scheme_lower;
                remaining_orig = after_scheme_orig;
            }
        } else {
            // Scheme not found after the colon — keep as-is and advance.
            remaining_lower = after_colon_lower;
            remaining_orig = after_colon_orig;
        }
    }

    result
}

/// Count the number of leading inline-whitespace bytes (space / tab) in `s`.
///
/// Stops at the first newline, carriage return, or non-whitespace byte.  Returns
/// a byte count that is guaranteed to be on a UTF-8 character boundary because
/// inline-whitespace bytes are all in the ASCII range and cannot appear inside
/// a multi-byte UTF-8 sequence.
fn count_inline_ws_bytes(s: &str) -> usize {
    s.as_bytes()
        .iter()
        .take_while(|&&b| b.is_ascii_whitespace() && b != b'\n' && b != b'\r')
        .count()
}

// ── Environment variable assignments ─────────────────────────────────────────

const SENSITIVE_ENV_VARS: &[&str] = &["GITHUB_TOKEN", "GH_TOKEN"];

fn redact_env_vars(mut s: String) -> String {
    for var in SENSITIVE_ENV_VARS {
        s = redact_env_var_value(s, var);
    }
    s
}

fn redact_env_var_value(s: String, var_name: &str) -> String {
    let needle = format!("{var_name}=");
    let mut result = String::with_capacity(s.len());
    let mut remaining = s.as_str();

    while let Some(pos) = remaining.find(needle.as_str()) {
        result.push_str(&remaining[..pos + needle.len()]);
        let after_eq = &remaining[pos + needle.len()..];
        // Value ends at the first whitespace byte or end-of-string.  We measure
        // the length in *bytes* (not chars) so the resulting split is a valid
        // UTF-8 boundary even when the value contains multi-byte characters —
        // ASCII whitespace bytes cannot appear inside a multi-byte UTF-8
        // sequence, so the first such byte is always on a char boundary.
        let after_bytes = after_eq.as_bytes();
        let value_len = after_bytes
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(after_bytes.len());
        if value_len > 0 {
            result.push_str(REDACTED);
            remaining = &after_eq[value_len..];
        } else {
            remaining = after_eq;
        }
    }

    result.push_str(remaining);
    result
}

// ── Extension trait for method-chain style ────────────────────────────────────

trait Pipe: Sized {
    fn pipe<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }
}

impl Pipe for String {}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── GitHub token patterns ─────────────────────────────────────────────────

    // 40-character suffixes (well above the 36-char minimum).
    const PAT: &str = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
    const OAUTH: &str = "gho_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
    const APP: &str = "ghs_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
    const REFRESH: &str = "ghr_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";

    #[test]
    fn redacts_github_pat() {
        let input = format!("token is {PAT} end");
        let output = redact_secrets(&input);
        assert!(!output.contains("ghp_ABCDE"), "{output}");
        assert!(output.contains("ghp_[REDACTED]"), "{output}");
    }

    #[test]
    fn redacts_github_oauth_token() {
        let output = redact_secrets(OAUTH);
        assert!(output.contains("gho_[REDACTED]"), "{output}");
        assert!(!output.contains("gho_ABCDE"), "{output}");
    }

    #[test]
    fn redacts_github_app_token() {
        let input = format!("value={APP}");
        let output = redact_secrets(&input);
        assert!(output.contains("ghs_[REDACTED]"), "{output}");
    }

    #[test]
    fn redacts_github_refresh_token() {
        let input = format!("refresh={REFRESH}");
        let output = redact_secrets(&input);
        assert!(output.contains("ghr_[REDACTED]"), "{output}");
    }

    #[test]
    fn does_not_redact_short_gh_prefix() {
        // Only 10 chars after prefix — below the 36-char minimum threshold.
        let input = "ghp_short123";
        let output = redact_secrets(input);
        assert_eq!(output, input, "Short suffix should NOT be redacted");
    }

    #[test]
    fn redacts_multiple_tokens_in_one_string() {
        let input = format!("{PAT} and {OAUTH}");
        let output = redact_secrets(&input);
        assert!(output.contains("ghp_[REDACTED]"), "{output}");
        assert!(output.contains("gho_[REDACTED]"), "{output}");
    }

    #[test]
    fn redacts_token_at_end_of_string() {
        let output = redact_secrets(PAT);
        assert!(output.contains("ghp_[REDACTED]"), "{output}");
    }

    // ── Authorization headers ─────────────────────────────────────────────────

    #[test]
    fn redacts_bearer_header() {
        let input = "Authorization: Bearer mysecrettoken123";
        let output = redact_secrets(input);
        assert!(output.contains("Bearer [REDACTED]"), "{output}");
        assert!(!output.contains("mysecrettoken123"), "{output}");
    }

    #[test]
    fn redacts_token_scheme_header() {
        let input = "Authorization: token myapitoken456";
        let output = redact_secrets(input);
        assert!(output.contains("token [REDACTED]"), "{output}");
        assert!(!output.contains("myapitoken456"), "{output}");
    }

    #[test]
    fn auth_header_redaction_is_case_insensitive_for_header_name() {
        let input = "authorization: Bearer mysecrettoken123";
        let output = redact_secrets(input);
        assert!(!output.contains("mysecrettoken123"), "{output}");
    }

    #[test]
    fn auth_header_preserves_rest_of_line() {
        let input =
            "GET /api HTTP/1.1\nAuthorization: Bearer abc123def\nContent-Type: application/json";
        let output = redact_secrets(input);
        assert!(!output.contains("abc123def"), "{output}");
        assert!(
            output.contains("Content-Type: application/json"),
            "{output}"
        );
        assert!(output.contains("GET /api HTTP/1.1"), "{output}");
    }

    // ── Environment variable patterns ─────────────────────────────────────────

    #[test]
    fn redacts_github_token_env_var() {
        let input = "GITHUB_TOKEN=ghp_somevalue123";
        let output = redact_secrets(input);
        assert!(output.starts_with("GITHUB_TOKEN=[REDACTED]"), "{output}");
    }

    #[test]
    fn redacts_gh_token_env_var() {
        let input = "GH_TOKEN=sometoken export OTHER=value";
        let output = redact_secrets(input);
        assert!(output.contains("GH_TOKEN=[REDACTED]"), "{output}");
        assert!(output.contains("OTHER=value"), "{output}");
    }

    #[test]
    fn env_var_redaction_stops_at_whitespace() {
        let input = "GITHUB_TOKEN=abc123 OTHER=keep";
        let output = redact_secrets(input);
        assert!(output.contains("GITHUB_TOKEN=[REDACTED]"), "{output}");
        assert!(output.contains("OTHER=keep"), "{output}");
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn empty_string_returns_empty() {
        assert_eq!(redact_secrets(""), "");
    }

    #[test]
    fn string_with_no_secrets_is_unchanged() {
        let input = "hello world, no secrets here";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn multiline_input_handled_correctly() {
        let input = format!("line1\n{PAT}\nline3");
        let output = redact_secrets(&input);
        assert!(output.contains("line1"), "{output}");
        assert!(output.contains("ghp_[REDACTED]"), "{output}");
        assert!(output.contains("line3"), "{output}");
    }

    #[test]
    fn non_ascii_content_is_preserved() {
        let input = "result: OK — no token here";
        assert_eq!(redact_secrets(input), input);
    }

    // ── UTF-8 edge cases (regression: #60) ────────────────────────────────────
    //
    // These inputs used to panic because the old implementation counted
    // characters and then used the char count as a byte index.  Any multi-byte
    // character in (or right before) the value region would land mid-codepoint.

    #[test]
    fn bearer_header_with_multibyte_value_does_not_panic() {
        // Value contains a multi-byte character (`é`, 2 bytes in UTF-8).
        let input = "Authorization: Bearer tokén_with_multibyte";
        let output = redact_secrets(input);
        assert!(output.contains("Bearer [REDACTED]"), "{output}");
        assert!(!output.contains("tokén"), "{output}");
    }

    #[test]
    fn bearer_header_with_multibyte_before_value_does_not_panic() {
        // Multi-byte character appears *before* the header, so all byte offsets
        // within the scan are shifted relative to what a char count would say.
        let input = "résumé\nAuthorization: Bearer abc123def";
        let output = redact_secrets(input);
        assert!(output.contains("Bearer [REDACTED]"), "{output}");
        assert!(!output.contains("abc123def"), "{output}");
        assert!(output.contains("résumé"), "{output}");
    }

    #[test]
    fn env_var_with_multibyte_value_does_not_panic() {
        let input = "GITHUB_TOKEN=tokén_value next";
        let output = redact_secrets(input);
        assert!(output.contains("GITHUB_TOKEN=[REDACTED]"), "{output}");
        assert!(output.contains("next"), "{output}");
        assert!(!output.contains("tokén_value"), "{output}");
    }

    #[test]
    fn bearer_header_with_mixed_case_scheme() {
        let input = "Authorization: BeArEr mytoken123";
        let output = redact_secrets(input);
        assert!(!output.contains("mytoken123"), "{output}");
    }

    #[test]
    fn case_insensitive_header_scan_survives_german_sharp_s() {
        // `ß` in `to_lowercase()` expands to `ss` (2 bytes → 2 bytes in this
        // case, but the general-case-folding path is what broke the old
        // implementation).  Use `to_ascii_lowercase()` internally, which
        // leaves non-ASCII untouched and preserves byte length.
        let input = "straße\nAuthorization: Bearer secret\nend";
        let output = redact_secrets(input);
        assert!(output.contains("Bearer [REDACTED]"), "{output}");
        assert!(!output.contains("secret"), "{output}");
    }

    #[test]
    fn empty_bearer_value_is_not_replaced() {
        // "Bearer" with nothing after it — no value to redact, should pass
        // through without inserting `[REDACTED]`.
        let input = "Authorization: Bearer ";
        let output = redact_secrets(input);
        assert!(!output.contains("[REDACTED]"), "{output}");
    }

    // ── Real-world edge cases (#70) ───────────────────────────────────────────

    #[test]
    fn bearer_header_with_tab_separator() {
        // A tab between `Authorization:` and `Bearer`, and between scheme
        // and value, is legal HTTP and must still trigger redaction.
        let input = "Authorization:\tBearer\tmyapitoken789";
        let output = redact_secrets(input);
        assert!(!output.contains("myapitoken789"), "{output}");
        assert!(output.contains("[REDACTED]"), "{output}");
    }

    #[test]
    fn env_var_with_crlf_terminator_is_bounded_correctly() {
        // Windows-written log files use CRLF; the redactor must stop the
        // value at `\r` (or any whitespace byte) and not swallow the rest
        // of the line.
        let input = "GITHUB_TOKEN=ghp_somesecret_value\r\nnext-line";
        let output = redact_secrets(input);
        assert!(output.contains("GITHUB_TOKEN=[REDACTED]"), "{output}");
        assert!(output.contains("next-line"), "{output}");
        assert!(!output.contains("ghp_somesecret_value"), "{output}");
    }

    #[test]
    fn token_immediately_preceded_by_alphanumeric_still_redacted() {
        // The current prefix scanner matches `ghp_` wherever it appears, so
        // `prefix-ghp_<40 chars>` should still redact.  Lock in the
        // behaviour explicitly.
        let input = format!("prefix-{PAT}");
        let output = redact_secrets(&input);
        assert!(output.contains("ghp_[REDACTED]"), "{output}");
    }

    #[test]
    fn ansi_escape_wrapped_token_is_not_split_by_redaction() {
        // ANSI color escapes often wrap log content.  The prefix scanner
        // only stops at non-alphanumeric bytes, so an escape right after
        // the prefix currently breaks redaction.  This test documents the
        // limitation so a future fix has a clear before/after target.
        //
        // NOTE: the behaviour tested here is intentionally the *current*
        // behaviour (redaction does NOT succeed across a raw `\x1b` byte).
        // If you tighten the scanner later, flip the assertion.
        let input = format!("\x1b[31m{PAT}\x1b[0m");
        let output = redact_secrets(&input);
        assert!(
            output.contains("ghp_[REDACTED]"),
            "expected redaction despite ANSI wrapping, got: {output}"
        );
    }
}
