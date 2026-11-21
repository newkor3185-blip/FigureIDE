//! Search and replace helpers for the active R source document.

use std::ops::Range;

#[derive(Clone, Debug, Default)]
pub(crate) struct EditorSearch {
    pub(crate) open: bool,
    pub(crate) replace_open: bool,
    pub(crate) query: String,
    pub(crate) replacement: String,
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
    pub(crate) regular_expression: bool,
    pub(crate) current_match: usize,
    pub(crate) document_id: Option<u64>,
    pub(crate) focus_query: bool,
}

fn compile(query: &str, case_sensitive: bool, whole_word: bool, regular_expression: bool) -> Result<regex::Regex, regex::Error> {
    let body = if regular_expression { query.to_owned() } else { regex::escape(query) };
    let pattern = if whole_word { format!(r"\b(?:{body})\b") } else { body };
    regex::RegexBuilder::new(&pattern).case_insensitive(!case_sensitive).build()
}

pub(crate) fn matches(text: &str, search: &EditorSearch) -> Result<Vec<Range<usize>>, regex::Error> {
    if search.query.is_empty() {
        return Ok(Vec::new());
    }
    Ok(compile(&search.query, search.case_sensitive, search.whole_word, search.regular_expression)?
        .find_iter(text)
        .take(100_000)
        .map(|found| found.start()..found.end())
        .collect())
}

pub(crate) fn replace_current(text: &mut String, range: Range<usize>, search: &EditorSearch) -> Result<(), String> {
    let expression = compile(&search.query, search.case_sensitive, search.whole_word, search.regular_expression)
        .map_err(|error| format!("Invalid search expression: {error}"))?;
    let found = text.get(range.clone()).ok_or_else(|| "Search result is no longer valid".to_owned())?;
    let replacement = expression.replace(found, search.replacement.as_str()).into_owned();
    text.replace_range(range, &replacement);
    Ok(())
}

pub(crate) fn replace_all(text: &mut String, search: &EditorSearch) -> Result<usize, String> {
    if search.query.is_empty() {
        return Ok(0);
    }
    let expression = compile(&search.query, search.case_sensitive, search.whole_word, search.regular_expression)
        .map_err(|error| format!("Invalid search expression: {error}"))?;
    let count = expression.find_iter(text).take(100_000).count();
    if count == 0 {
        return Ok(0);
    }
    *text = expression.replace_all(text, search.replacement.as_str()).into_owned();
    Ok(count)
}

/// Apply search backgrounds to syntax-highlighted sections without changing their text formats.
pub(crate) fn highlight_layout_job(
    job: &mut egui::text::LayoutJob,
    ranges: &[Range<usize>],
    current: Option<usize>,
    background: egui::Color32,
    current_background: egui::Color32,
) {
    if ranges.is_empty() {
        return;
    }
    let sections = std::mem::take(&mut job.sections);
    for section in sections {
        let start = usize::from(section.byte_range.start);
        let end = usize::from(section.byte_range.end);
        let mut boundaries = vec![start, end];
        for range in ranges.iter().filter(|range| range.start < end && range.end > start) {
            boundaries.push(range.start.max(start));
            boundaries.push(range.end.min(end));
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for pair in boundaries.windows(2) {
            let (Some(&piece_start), Some(&piece_end)) = (pair.first(), pair.get(1)) else { continue };
            if piece_start >= piece_end {
                continue;
            }
            let matching = ranges.iter().position(|range| range.start < piece_end && range.end > piece_start);
            let mut format = section.format.clone();
            if let Some(index) = matching {
                format.background = if Some(index) == current { current_background } else { background };
            }
            job.sections.push(egui::text::LayoutSection {
                leading_space: if piece_start == start { section.leading_space } else { 0.0 },
                byte_range: egui::text::ByteIndex(piece_start)..egui::text::ByteIndex(piece_end),
                format,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str, query: &str, case_sensitive: bool, whole_word: bool, regular_expression: bool) -> Vec<Range<usize>> {
        matches(text, &EditorSearch { query: query.into(), case_sensitive, whole_word, regular_expression, ..Default::default() })
            .expect("valid search")
    }

    #[test]
    fn literal_search_supports_case_and_unicode_boundaries() {
        assert_eq!(query("Plot plot Plotting", "plot", false, false, false), vec![0..4, 5..9, 10..14]);
        assert_eq!(query("Plot plot", "plot", true, false, false), vec![5..9]);
        assert_eq!(query("xplot x plot!", "plot", false, true, false), vec![8..12]);
        assert_eq!(query("\u{d55c}\u{ae00}plot \u{d55c}\u{ae00} plot!", "plot", false, true, false), vec![18..22]);
    }

    #[test]
    fn regular_expression_replacement_uses_capture_groups() {
        let mut text = "foo=1\nfoo=22".to_owned();
        let search = EditorSearch { query: r"foo=(\d+)".into(), regular_expression: true, replacement: "bar=$1".into(), ..Default::default() };
        assert_eq!(replace_all(&mut text, &search), Ok(2));
        assert_eq!(text, "bar=1\nbar=22");
    }

    #[test]
    fn invalid_regular_expression_is_reported_without_mutating_text() {
        let mut text = "unchanged".to_owned();
        let search = EditorSearch { query: "(".into(), regular_expression: true, replacement: "x".into(), ..Default::default() };
        assert!(replace_all(&mut text, &search).is_err());
        assert_eq!(text, "unchanged");
    }
}
