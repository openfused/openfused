// Context validity windows — time-bound sections in CONTEXT.md that expire.
// Agents annotate sections with <!-- validity: 6h --> and the system tracks
// when they were added via <!-- openfuse:added: ISO --> timestamps.
// Confidence decays exponentially: 1.0 at write time, 0.5 at TTL, asymptotes to 0.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Parsed validity section from CONTEXT.md
#[derive(Debug, Serialize, Clone)]
pub struct ValiditySection {
    pub header: String,
    pub content: String,
    pub ttl_str: String,
    pub ttl_ms: u64,
    pub added: Option<String>,
    pub confidence: f64,
    pub expired: bool,
}

/// Report of all validity-annotated sections
#[derive(Debug, Serialize)]
pub struct ValidityReport {
    pub entries: Vec<ValiditySection>,
    pub stale: usize,
    pub fresh: usize,
}

/// Parse a TTL string like "6h", "1d", "3d", "30m" to milliseconds.
pub fn parse_ttl_ms(ttl: &str) -> Option<u64> {
    let ttl = ttl.trim();
    if ttl.len() < 2 {
        return None;
    }
    let (num_str, unit) = ttl.split_at(ttl.len() - 1);
    let num: f64 = num_str.parse().ok()?;
    match unit {
        "m" => Some((num * 60.0 * 1000.0) as u64),
        "h" => Some((num * 3600.0 * 1000.0) as u64),
        "d" => Some((num * 86400.0 * 1000.0) as u64),
        _ => None,
    }
}

/// Exponential decay confidence: 1.0 at write, 0.5 at TTL, asymptotes to 0.
fn compute_confidence(age_ms: u64, ttl_ms: u64) -> f64 {
    if ttl_ms == 0 {
        return 0.0;
    }
    let ratio = age_ms as f64 / ttl_ms as f64;
    // ln(2) ≈ 0.693 — half-life at exactly TTL
    (-0.693 * ratio).exp()
}

/// Parse CONTEXT.md for validity-annotated sections.
pub fn parse_validity_sections(content: &str) -> Vec<ValiditySection> {
    let now = Utc::now();
    let mut sections = Vec::new();
    let mut current_header = String::new();
    let mut current_content = String::new();
    let mut current_ttl: Option<String> = None;
    let mut current_added: Option<String> = None;
    let mut in_section = false;

    let flush = |header: &str,
                 content: &str,
                 ttl: &Option<String>,
                 added: &Option<String>,
                 now: DateTime<Utc>,
                 sections: &mut Vec<ValiditySection>| {
        if let Some(ref ttl_str) = ttl {
            if let Some(ttl_ms) = parse_ttl_ms(ttl_str) {
                let age_ms = if let Some(ref ts) = added {
                    if let Ok(dt) = ts.parse::<DateTime<Utc>>() {
                        (now - dt).num_milliseconds().max(0) as u64
                    } else {
                        ttl_ms * 2 // Can't parse → assume stale
                    }
                } else {
                    ttl_ms * 2 // No timestamp → assume stale
                };
                let confidence = compute_confidence(age_ms, ttl_ms);
                sections.push(ValiditySection {
                    header: header.to_string(),
                    content: content.to_string(),
                    ttl_str: ttl_str.clone(),
                    ttl_ms,
                    added: added.clone(),
                    confidence,
                    expired: confidence < 0.5,
                });
            }
        }
    };

    for line in content.lines() {
        if line.starts_with("## ") || line.starts_with("### ") {
            if in_section {
                flush(
                    &current_header,
                    &current_content,
                    &current_ttl,
                    &current_added,
                    now,
                    &mut sections,
                );
            }
            current_header = line.to_string();
            current_content = String::new();
            current_ttl = None;
            current_added = None;
            in_section = true;
        } else if in_section {
            current_content.push_str(line);
            current_content.push('\n');

            // Parse <!-- validity: 6h -->
            if let Some(start) = line.find("<!-- validity:") {
                if let Some(end) = line[start..].find("-->") {
                    let val = line[start + 14..start + end].trim();
                    current_ttl = Some(val.to_string());
                }
            }

            // Parse <!-- openfuse:added: ISO -->
            if let Some(start) = line.find("<!-- openfuse:added:") {
                if let Some(end) = line[start..].find("-->") {
                    let val = line[start + 20..start + end].trim();
                    current_added = Some(val.to_string());
                }
            }
        }
    }

    // Flush last section
    if in_section {
        flush(
            &current_header,
            &current_content,
            &current_ttl,
            &current_added,
            now,
            &mut sections,
        );
    }

    sections
}

/// Build a validity report from CONTEXT.md content.
pub fn build_validity_report(content: &str) -> ValidityReport {
    let entries = parse_validity_sections(content);
    let stale = entries.iter().filter(|s| s.expired).count();
    let fresh = entries.iter().filter(|s| !s.expired).count();
    ValidityReport {
        entries,
        stale,
        fresh,
    }
}

/// Replace expired validity sections in content with archive markers.
/// Returns the modified content with stale sections replaced.
pub fn prune_stale_sections(content: &str) -> (String, usize) {
    let sections = parse_validity_sections(content);
    let stale_headers: Vec<&str> = sections
        .iter()
        .filter(|s| s.confidence < 0.1) // Very stale
        .map(|s| s.header.as_str())
        .collect();

    if stale_headers.is_empty() {
        return (content.to_string(), 0);
    }

    let mut result = String::new();
    let mut skip_until_next_header = false;
    let mut pruned = 0;

    for line in content.lines() {
        if line.starts_with("## ") || line.starts_with("### ") {
            if stale_headers.contains(&line) {
                result.push_str(line);
                result.push('\n');
                result.push_str("[STALE — archived by openfuse compact --prune-stale]\n\n");
                skip_until_next_header = true;
                pruned += 1;
                continue;
            } else {
                skip_until_next_header = false;
            }
        }
        if !skip_until_next_header {
            result.push_str(line);
            result.push('\n');
        }
    }

    (result, pruned)
}
