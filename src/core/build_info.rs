/// Cargo package version (always available at compile time).
pub const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Optional build metadata injected by CI (for release builds).
pub const GIT_TAG: Option<&str> = option_env!("PULSE_GATE_GIT_TAG");
pub const GIT_SHA: Option<&str> = option_env!("PULSE_GATE_GIT_SHA");

fn short_sha(sha: &str) -> &str {
    let max = 8;
    if sha.len() > max { &sha[..max] } else { sha }
}

/// Returns an app version label suitable for UI display.
///
/// Examples:
/// - "pulse-gate 0.1.0"
/// - "pulse-gate 0.1.0 (v0.1.0)"
/// - "pulse-gate 0.1.0 (v0.1.0, a1b2c3d4)"
pub fn display_app_version() -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(tag) = GIT_TAG
        && !tag.is_empty()
    {
        parts.push(tag.to_string());
    }

    if let Some(sha) = GIT_SHA
        && !sha.is_empty()
    {
        parts.push(short_sha(sha).to_string());
    }

    if parts.is_empty() {
        format!("pulse-gate {CARGO_VERSION}")
    } else {
        format!("pulse-gate {CARGO_VERSION} ({})", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_sha_truncates_long_values() {
        assert_eq!(short_sha("1234567890abcdef"), "12345678");
    }

    #[test]
    fn short_sha_keeps_short_values() {
        assert_eq!(short_sha("1234"), "1234");
    }
}
