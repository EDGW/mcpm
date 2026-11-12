//! Live Minecraft EULA preview; no agreement text is embedded in the application.

use anyhow::{Context, Result, ensure};
use regex::Regex;
use url::Url;

use super::Downloader;

pub const EULA_URL: &str = "https://www.minecraft.net/en-us/eula";

/// First twenty nonempty visible body blocks and the omitted block count.
pub struct EulaPreview {
    pub url: &'static str,
    pub lines: Vec<EulaLine>,
    pub remaining_lines: usize,
}

/// Structural heading information, independent of frontend colors and styles.
#[derive(Debug)]
pub struct EulaLine {
    pub text: String,
    pub heading: bool,
}

/// Fetches the agreement through the caller's exact source order and timeouts.
pub async fn fetch(downloader: &Downloader) -> Result<EulaPreview> {
    downloader
        .metadata(&Url::parse(EULA_URL)?, |bytes| {
            preview(std::str::from_utf8(bytes).context("EULA page is not UTF-8")?)
        })
        .await
        .with_context(|| format!("failed to fetch Minecraft EULA from {EULA_URL}"))
}

fn preview(html: &str) -> Result<EulaPreview> {
    let comments = Regex::new(r"(?s)<!--.*?-->")?;
    let mut clean = comments.replace_all(html, "").into_owned();
    for name in ["script", "style"] {
        let pattern = Regex::new(&format!(r"(?is)<{name}\b[^>]*>.*?</{name}\s*>"))?;
        clean = pattern.replace_all(&clean, "").into_owned();
    }
    let main = Regex::new(r#"(?is)<div\b[^>]*\bid\s*=\s*["']main-content["'][^>]*>"#)?;
    let start = main
        .find(&clean)
        .context("EULA page has no main-content body")?
        .end();
    let content = &clean[start..];
    let tags = Regex::new(r"(?s)<[^>]*>")?;
    let mut text = String::new();
    let mut lines = Vec::new();
    let mut total = 0;
    let mut heading = false;
    let mut offset = 0;
    let mut depth = 1usize;
    for tag in tags.find_iter(content) {
        append_text(&mut text, &content[offset..tag.start()]);
        let raw = tag
            .as_str()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        let closing = raw.starts_with('/');
        let name = raw
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if name == "div" {
            if closing {
                depth -= 1;
            } else if !raw.ends_with('/') {
                depth += 1;
            }
            if depth == 0 {
                break;
            }
        }
        let heading_tag = matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        if heading_tag
            || matches!(
                name.as_str(),
                "p" | "li"
                    | "ul"
                    | "ol"
                    | "br"
                    | "div"
                    | "section"
                    | "table"
                    | "tr"
                    | "article"
                    | "hr"
            )
        {
            flush_line(&mut text, heading, &mut lines, &mut total);
        }
        if heading_tag {
            heading = !closing;
        }
        offset = tag.end();
    }
    flush_line(&mut text, heading, &mut lines, &mut total);
    ensure!(
        !lines.is_empty(),
        "EULA main-content body has no visible text"
    );
    let remaining_lines = total - lines.len();
    Ok(EulaPreview {
        url: EULA_URL,
        lines,
        remaining_lines,
    })
}

fn flush_line(text: &mut String, heading: bool, lines: &mut Vec<EulaLine>, total: &mut usize) {
    let normalized = entities(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !normalized.is_empty() {
        *total += 1;
        if lines.len() < 20 {
            lines.push(EulaLine {
                text: normalized,
                heading,
            });
        }
    }
    text.clear();
}

fn append_text(output: &mut String, text: &str) {
    for ch in text.chars() {
        output.push(if ch.is_whitespace() { ' ' } else { ch });
    }
}

fn entities(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';').filter(|end| *end <= 16) else {
            output.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let value = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            name if name.starts_with("#x") || name.starts_with("#X") => {
                u32::from_str_radix(&name[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            name if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        if let Some(value) = value {
            output.push(value);
        } else {
            output.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uses_only_visible_main_body_and_preserves_inline_text() {
        let html = "<nav>navigation</nav><div id='main-content'><script>hidden</script><style>hidden</style><!-- hidden --><h1>Title &#x2014; text</h1><p>Some\n <b>bold</b> &amp; <a>linked</a> text&#39;s.</p><div><p>Nested&nbsp;paragraph</p></div></div><p>footer</p>";
        let parsed = preview(html).unwrap();
        assert_eq!(
            parsed
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            [
                "Title — text",
                "Some bold & linked text's.",
                "Nested paragraph"
            ]
        );
        assert!(parsed.lines[0].heading);
        assert!(!parsed.lines[1].heading);
        assert_eq!(parsed.remaining_lines, 0);
        assert!(
            preview("<div id='main-content'><script>hidden</script></div><p>footer</p>").is_err()
        );
        assert!(preview("<p>No main container</p>").is_err());
    }

    #[test]
    fn preview_limits_nonempty_blocks_not_inline_fragments() {
        let mut html = String::from("<div id=\"main-content\">");
        for i in 0..23 {
            html.push_str(&format!("<p><b>{i}</b> value<br> </p>"));
        }
        html.push_str("</div>");
        let expected: Vec<_> = (0..20).map(|i| format!("{i} value")).collect();
        let parsed = preview(&html).unwrap();
        assert_eq!(
            parsed
                .lines
                .iter()
                .map(|line| &line.text)
                .collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert_eq!(parsed.remaining_lines, 3);
    }
}
