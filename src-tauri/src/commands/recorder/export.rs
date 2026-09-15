//! Recorder export naming helpers.

pub(super) fn safe_file_stem(title: &str) -> String {
    let safe = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            other => other,
        })
        .collect::<String>();
    let trimmed = safe.trim();
    if trimmed.is_empty() {
        "recording".into()
    } else {
        trimmed.into()
    }
}
