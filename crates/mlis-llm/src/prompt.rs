//! Qwen2.5 ChatML prompt construction — a direct port of
//! `python/inferer/prompts.py`.

const SYSTEM: &str = "You are an expert, highly accurate identity document parser. Your task is \
to extract specific fields from the provided OCR Markdown text into a \
strict, valid JSON object. If a field is not found or is illegible, use \
null. Do not invent data.";

/// Fields requested from the model (subset of the canonical schema;
/// provenance fields are added downstream, not by the model).
const FIELDS: &[&str] = &[
    "document_type",
    "issuing_country",
    "document_number",
    "surname",
    "given_names",
    "nationality",
    "date_of_birth",
    "sex",
    "date_of_expiry",
    "mrz_line",
];

/// Build the Qwen2.5 ChatML prompt for one document's OCR Markdown.
pub fn build_prompt(md_content: &str) -> String {
    let fields = FIELDS.join(", ");
    // docling renders MRZ filler chevrons as HTML entities (`<` -> `&lt;`) in
    // its Markdown output. Left escaped, the model echoes `&lt;` verbatim into
    // fields like mrz_line instead of the literal `<` printed on the
    // document, so unescape before the model ever sees the text.
    let md_content = unescape_html(md_content);
    format!(
        "<|im_start|>system\n{SYSTEM}\n<|im_end|>\n\
         <|im_start|>user\nExtract these fields: {fields}.\n\n\
         OCR Markdown Text:\n{md_content}\n\n\
         Output ONLY valid JSON. No markdown formatting, no explanations, no \
         code blocks.\n<|im_end|>\n<|im_start|>assistant"
    )
}

/// Unescape the small set of HTML entities docling actually emits. Not a
/// general-purpose HTML unescaper — just enough to undo docling's markdown
/// rendering of `<`/`>`/`&` etc. in OCR text.
fn unescape_html(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&") // must be last: undoes double-escaping of the above
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_mrz_chevrons() {
        assert_eq!(unescape_html("P&lt;UTOA1234567&lt;&lt;"), "P<UTOA1234567<<");
    }

    #[test]
    fn ampersand_unescaped_last_avoids_double_unescape() {
        // "&amp;lt;" should become "&lt;", not "<".
        assert_eq!(unescape_html("&amp;lt;"), "&lt;");
    }

    #[test]
    fn prompt_contains_chatml_markers_and_fields() {
        let p = build_prompt("some markdown");
        assert!(p.starts_with("<|im_start|>system\n"));
        assert!(p.ends_with("<|im_start|>assistant"));
        assert!(p.contains("document_number"));
        assert!(p.contains("some markdown"));
    }
}
