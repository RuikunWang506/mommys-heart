//! Formatting helpers shared by the server and the UI.

use crate::server_fns::capabilities::CaseCapability;

/// The class list for a small rounded status pill, so every badge in the app is
/// shaped the same and only its colours differ.
pub fn badge_pill(colors: &str) -> String {
    format!("inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {colors}")
}

/// Up to two initials for an avatar, or "?" when there is no name to take them from.
pub fn initials(name: &str) -> String {
    let found: String = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .flat_map(|c| c.to_uppercase())
        .collect();
    if found.is_empty() {
        "?".to_string()
    } else {
        found
    }
}

/// Percent-encode a document path for a query string. Paths carry user-supplied
/// folder and file names, so everything outside the unreserved set is escaped
/// rather than trusted to be URL-safe — `/` included, since it is a separator
/// within the path and not a path boundary in the URL.
///
/// Lives here because both the Documents panel and the note page's filed-record
/// panel build the same download link.
pub fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Format a byte count as a short human-readable size (e.g. "2.4 MB").
pub fn human_size(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// A short label summarizing a user's access to a case from their capabilities.
pub fn access_label(caps: &[CaseCapability]) -> Option<(&'static str, &'static str)> {
    if caps.is_empty() {
        return None;
    }
    if caps.len() == CaseCapability::ALL.len() {
        Some((
            "Full access",
            "bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30",
        ))
    } else if caps.contains(&CaseCapability::EditCase) {
        Some((
            "Manager",
            "bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30",
        ))
    } else if caps.contains(&CaseCapability::UploadEvidence)
        || caps.contains(&CaseCapability::AddNotes)
        || caps.contains(&CaseCapability::SendMessages)
    {
        Some((
            "Contributor",
            "bg-sky-500/15 text-sky-300 ring-1 ring-sky-500/30",
        ))
    } else {
        Some((
            "Viewer",
            "bg-slate-500/15 text-slate-300 ring-1 ring-slate-500/30",
        ))
    }
}
