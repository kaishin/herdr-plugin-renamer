//! Turning arbitrary text into a safe kebab-case slug, and a deterministic
//! fallback slug derived from the prompt when the Codex naming engine is
//! unavailable.

const MAX_WORDS: usize = 6;
const MAX_LEN: usize = 50;

/// Lowercase, collapse every run of non-alphanumeric characters into a single
/// hyphen, trim leading/trailing hyphens, then cap to `MAX_WORDS` words and
/// `MAX_LEN` characters. ASCII-only output suitable for a git branch name.
pub fn sanitize(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true; // start true so leading separators are dropped
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }

    let capped = out
        .split('-')
        .filter(|w| !w.is_empty())
        .take(MAX_WORDS)
        .collect::<Vec<_>>()
        .join("-");

    let mut capped = if capped.len() > MAX_LEN {
        capped[..MAX_LEN].to_string()
    } else {
        capped
    };
    while capped.ends_with('-') {
        capped.pop();
    }
    capped
}

/// Build a slug from the first non-empty line of the prompt. Never returns an
/// empty string, so a rename always has something to use.
pub fn fallback_from_prompt(prompt: &str) -> String {
    let first_line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let slug = sanitize(first_line);
    if slug.is_empty() {
        "agent-task".to_string()
    } else {
        slug
    }
}

const DISPLAY_MAX_CHARS: usize = 24;
const EN_NAME_MAX_CHARS: usize = 32;

/// Capitalise the first ASCII letter of each kebab/snake segment and join with
/// spaces. Used to derive a friendly Title Case label from a slug-only fallback
/// (`phone-otp-login` -> `Phone Otp Login`). The output mirrors what the LLM
/// engine is asked to produce directly; this helper is the deterministic
/// recovery path when the engine only emits a slug. Exposed for the Foundation
/// engine arm in `main::generate_name`.
pub fn title_case(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) if c.is_ascii_alphabetic() => {
                    let mut word = c.to_ascii_uppercase().to_string();
                    word.push_str(chars.as_str().to_ascii_lowercase().as_str());
                    word
                }
                Some(c) => c.to_string(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a Title Case English label from the first non-empty prompt line as a
/// fallback when every naming engine failed. Punctuation is collapsed to
/// spaces, each word is capitalised, and the result is capped to
/// `DISPLAY_MAX_CHARS`. Non-ASCII prompts and filler-only prompts (every
/// input word is all-lowercase and there are several of them) are rejected
/// because the Title Cased output would be misleading; the caller treats
/// such results as no-ops and uses `agent-task` instead.
fn en_display_fallback(line: &str) -> Option<String> {
    let normalized: String = line
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    let raw_words: Vec<&str> = normalized.split_whitespace().collect();
    if raw_words.is_empty() {
        return None;
    }
    // Out of scope for an English-only fork: if no word carries an ASCII
    // letter, the prompt is non-ASCII (emoji, accented Latin, etc.) and the
    // Title Cased output would be a single non-English glyph at best. Bail
    // and let the caller use `agent-task`.
    if !raw_words
        .iter()
        .any(|w| w.chars().any(|c| c.is_ascii_alphabetic()))
    {
        return None;
    }
    if raw_words.len() >= 2
        && raw_words.iter().all(|w| {
            w.chars().any(|c| c.is_ascii_alphabetic()) && !w.chars().any(|c| c.is_ascii_uppercase())
        })
    {
        return None;
    }
    let title_cased: Vec<String> = raw_words
        .iter()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) if c.is_ascii_alphabetic() => {
                    let mut word = c.to_ascii_uppercase().to_string();
                    word.push_str(chars.as_str().to_ascii_lowercase().as_str());
                    word
                }
                Some(c) => c.to_string(),
                None => String::new(),
            }
        })
        .collect();
    let title = title_cased.join(" ");
    let capped: String = title.chars().take(DISPLAY_MAX_CHARS).collect();
    let capped = capped.trim_end().to_string();
    // English-only fork: if the Title Cased output carries any non-ASCII
    // characters, the input was non-English and the result would be a
    // misleading single non-English glyph or accented Latin. Caller falls
    // back to `agent-task`.
    if !capped.is_ascii() {
        return None;
    }
    if capped.is_empty() {
        None
    } else {
        Some(capped)
    }
}

/// A display name for pane/tab/agent labels when every naming engine failed.
/// Builds a Title Case English label from the first non-empty prompt line;
/// rejects filler-only or non-ASCII input and falls back to `agent-task` so
/// the caller always gets something to write. Git branches still use
/// `fallback_from_prompt`.
pub fn display_fallback(prompt: &str) -> String {
    let first_line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if let Some(en) = en_display_fallback(first_line) {
        return en;
    }
    "agent-task".to_string()
}

const INSTRUCTION_HEAD_CHARS: usize = 500;
const INSTRUCTION_TAIL_CHARS: usize = 300;

/// A head+tail excerpt of the prompt for the engine instruction. Long prompts
/// are usually pasted context with the actual request at one end; a short
/// excerpt names just as well and keeps small relay models fast.
fn instruction_excerpt(prompt: &str) -> String {
    let char_count = prompt.chars().count();
    if char_count <= INSTRUCTION_HEAD_CHARS + INSTRUCTION_TAIL_CHARS {
        return prompt.to_string();
    }
    let head: String = prompt.chars().take(INSTRUCTION_HEAD_CHARS).collect();
    let tail_start = char_count.saturating_sub(INSTRUCTION_TAIL_CHARS);
    let tail: String = prompt.chars().skip(tail_start).collect();
    format!("{head}\n\n[... middle omitted ...]\n\n{tail}")
}

/// Build the instruction handed to a CLI naming engine. Asks for two lines
/// so the visible label can carry natural-language copy while branches stay
/// ASCII: line 1 is a Title Case English label; line 2 is the kebab-case
/// git branch slug.
pub fn engine_instruction(prompt: &str) -> String {
    let truncated = instruction_excerpt(prompt);
    format!(
        "Output only two lines, no explanation, no quotes, no numbering, no extra text.\n\
         Line 1: a short English Title Case label (1-4 words, ASCII letters and \
         spaces only, no hyphens, no underscores, no leading/trailing spaces) \
         describing what this coding task does. Write a noun phrase the user \
         can read at a glance — \"object + action/result\" — concrete and specific. \
         Strip filler words (\"help me\", \"please\", \"how to\", \"can you\", \"let's\"). \
         Keep product/proper names verbatim (herdr, NewAPI, GitHub, OTP, API).\n\
         Good examples: Storage / Phone OTP Login / Email Verification / OAuth Refresh Token / Cache Invalidation\n\
         Bad examples: storage / phone-otp-login / 1-2-3-h1-yoy / help me with auth\n\
         No kebab-case, no snake_case, no all-lowercase phrases, no metric codes.\n\
         Line 2: a kebab-case git branch slug (2-4 lowercase English words, ASCII \
         letters/digits and hyphens only, no numeric segments like 1-2-3-h1).\n\
         Task content:\n\n{truncated}"
    )
}

/// Reject digit-heavy or token-noise slugs that look like garbled metric dumps
/// (`1-2-3-6-h1-yoy`) rather than a readable task name. Pure word slugs pass.
fn is_good_slug(slug: &str) -> bool {
    if slug.is_empty() || slug == "agent-task" {
        return !slug.is_empty();
    }
    let tokens: Vec<&str> = slug.split('-').filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return false;
    }
    // A single pure-digit token (`1`) is not a useful tab label.
    if tokens.len() == 1 && tokens[0].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let digit_tokens = tokens
        .iter()
        .filter(|t| t.chars().all(|c| c.is_ascii_digit()))
        .count();
    // Two or more pure-digit segments usually means the model latched onto
    // chart indices / version bits instead of the task topic.
    if digit_tokens >= 2 {
        return false;
    }
    // Mostly non-letter content (digits + short codes) is also noise.
    let alnum: Vec<char> = slug.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if alnum.is_empty() {
        return false;
    }
    let letters = alnum.iter().filter(|c| c.is_ascii_alphabetic()).count();
    if letters * 2 < alnum.len() {
        return false;
    }
    true
}

/// Cap and strip quotes from a candidate display name line. Uses
/// `EN_NAME_MAX_CHARS` so phrases like "OAuth Refresh Token" survive.
fn clean_name_line(line: &str) -> String {
    line.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c.is_whitespace())
        .chars()
        .take(EN_NAME_MAX_CHARS)
        .collect()
}

/// Quality gate for English Title Case labels. Rejects digit-only strings,
/// kebab/snake leakage (`phone-otp-login`), and all-lowercase multi-word
/// strings — the engine prompt already asks for Title Case, so anything else
/// is a model that ignored the brief and the chain should advance.
fn is_good_en_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > EN_NAME_MAX_CHARS {
        return false;
    }
    let stripped =
        trimmed.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c.is_whitespace());
    if stripped.is_empty() {
        return false;
    }
    if stripped.contains('-') || stripped.contains('_') {
        return false;
    }
    if !stripped.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !stripped
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ')
    {
        return false;
    }
    let words: Vec<&str> = stripped.split_whitespace().collect();
    if words.len() >= 2
        && !words
            .iter()
            .any(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
    {
        return false;
    }
    true
}

/// Parse a CLI engine's raw output into `(label, branch slug)`. Prefers the
/// two-line "label then slug" layout; single-line input is treated as a slug
/// only and Title Cased for the label so older prompt templates still work.
///
/// Quality gates: en labels must look like Title Case English (no
/// kebab/snake leakage, no all-lowercase multi-word); slugs must not be
/// digit-noise. Failures return `None` so the engine chain / local fallback
/// can take over instead of writing garbled tab names like `1-2-3-6-h1-yoy`.
pub fn parse_engine_output(raw: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let last = *lines.last()?;

    // Try the two-line "label then slug" layout first. A clean label and a
    // good slug together are the strongest signal of a well-behaved engine.
    if lines.len() >= 2 {
        let label_candidate = clean_name_line(lines[lines.len() - 2]);
        let slug_candidate = sanitize(last);
        if is_good_en_name(&label_candidate)
            && !slug_candidate.is_empty()
            && is_good_slug(&slug_candidate)
        {
            return Some((label_candidate, slug_candidate));
        }
    }

    // Single-line legacy path: treat the line as the slug and Title Case it
    // for the label.
    let slug = sanitize(last);
    if slug.is_empty() || !is_good_slug(&slug) {
        return None;
    }
    let label = clean_name_line(&title_case(&slug));
    if !is_good_en_name(&label) {
        return None;
    }
    Some((label, slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_kebab() {
        assert_eq!(sanitize("OAuth Login Providers"), "oauth-login-providers");
    }

    #[test]
    fn collapses_punctuation_and_spaces() {
        assert_eq!(
            sanitize("Fix the bug!!! in   parser"),
            "fix-the-bug-in-parser"
        );
    }

    #[test]
    fn trims_edges() {
        assert_eq!(sanitize("  --Hello, World--  "), "hello-world");
    }

    #[test]
    fn caps_to_six_words() {
        assert_eq!(
            sanitize("one two three four five six seven eight"),
            "one-two-three-four-five-six"
        );
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(sanitize("   !!!   "), "");
    }

    #[test]
    fn fallback_never_empty() {
        assert_eq!(fallback_from_prompt("!!!"), "agent-task");
        assert_eq!(
            fallback_from_prompt("Add JWT auth to the API endpoints please"),
            "add-jwt-auth-to-the-api"
        );
    }

    #[test]
    fn fallback_uses_first_nonempty_line() {
        assert_eq!(
            fallback_from_prompt("\n\n  \nRefactor token validation"),
            "refactor-token-validation"
        );
    }

    #[test]
    fn display_fallback_prefers_title_case_label() {
        assert_eq!(
            display_fallback("Add JWT auth to the API endpoints please"),
            "Add Jwt Auth To The Api"
        );
    }

    #[test]
    fn display_fallback_returns_agent_task_for_non_ascii() {
        // Non-ASCII prompts are out of scope for this fork: Title Case needs
        // ASCII letters, so the deterministic fallback is `agent-task`.
        assert_eq!(display_fallback("🦀🌍🎉"), "agent-task");
        assert_eq!(display_fallback("αβγ"), "agent-task");
        assert_eq!(display_fallback("héllo"), "agent-task");
    }

    #[test]
    fn display_fallback_never_empty() {
        assert_eq!(display_fallback("!!!"), "agent-task");
        assert_eq!(display_fallback(""), "agent-task");
    }

    #[test]
    fn display_fallback_skips_filler_only_prompts() {
        // Pure filler ("help me with auth please") is all lowercase and would
        // produce a misleading "Help Me With Auth Please" label; fall back
        // to agent-task instead.
        assert_eq!(display_fallback("help me with auth please"), "agent-task");
    }

    #[test]
    fn parse_en_takes_title_case_label_and_kebab_slug() {
        let parsed = parse_engine_output("thinking...\nPhone OTP Login\nphone-otp-login\n");
        assert_eq!(
            parsed,
            Some(("Phone OTP Login".to_string(), "phone-otp-login".to_string()))
        );
    }

    #[test]
    fn parse_en_single_line_title_cases_the_slug() {
        let parsed = parse_engine_output("fix-db-index\n");
        assert_eq!(
            parsed,
            Some(("Fix Db Index".to_string(), "fix-db-index".to_string()))
        );
    }

    #[test]
    fn parse_en_rejects_punctuation_only_label() {
        assert!(parse_engine_output("！！！\n").is_none());
    }

    #[test]
    fn parse_en_rejects_digit_noise_slugs() {
        assert!(parse_engine_output("1-2-3-6-h1-yoy\n").is_none());
        assert!(parse_engine_output("1\n").is_none());
        assert_eq!(
            parse_engine_output("fix-db-index\n"),
            Some(("Fix Db Index".to_string(), "fix-db-index".to_string()))
        );
    }

    #[test]
    fn parse_en_two_line_kebab_label_falls_through_to_slug_recovery() {
        // The engine ignored the brief and emitted a kebab for both lines.
        // The two-line check rejects the kebab-shaped label, so the parser
        // falls through to the single-line legacy path: treat the last line
        // as the slug and Title Case it for the label. This is the same
        // outcome as the single-line input — we still get a usable pair.
        assert_eq!(
            parse_engine_output("phone-otp-login\nphone-otp-login\n"),
            Some(("Phone Otp Login".to_string(), "phone-otp-login".to_string()))
        );
    }

    #[test]
    fn title_case_handles_kebab_and_snake() {
        assert_eq!(title_case("phone-otp-login"), "Phone Otp Login");
        assert_eq!(title_case("fix_db_index"), "Fix Db Index");
        assert_eq!(title_case("storage"), "Storage");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn is_good_en_name_accepts_title_case() {
        assert!(is_good_en_name("Phone OTP Login"));
        assert!(is_good_en_name("Storage"));
        assert!(is_good_en_name("OAuth Refresh Token"));
        assert!(is_good_en_name("Cache Invalidation"));
    }

    #[test]
    fn is_good_en_name_rejects_kebab_and_noise() {
        assert!(!is_good_en_name("phone-otp-login"));
        assert!(!is_good_en_name("phone_otp_login"));
        assert!(!is_good_en_name("storage layer"));
        assert!(!is_good_en_name(""));
        assert!(!is_good_en_name("1-2-3"));
        assert!(!is_good_en_name("!!!"));
    }

    #[test]
    fn is_good_slug_accepts_wordy_labels() {
        assert!(is_good_slug("meituan-growth-research"));
        assert!(is_good_slug("hr-ai-efficiency"));
        assert!(!is_good_slug("1-2-3-6-h1-yoy"));
        assert!(!is_good_slug("1"));
    }
}
