use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleFileHtmlOptions {
    pub inline_scripts: bool,
    pub inline_styles: bool,
    pub inline_assets_below_bytes: usize,
    pub max_total_bytes: usize,
    pub offline: bool,
}

impl SingleFileHtmlOptions {
    pub fn test_default() -> Self {
        Self {
            inline_scripts: true,
            inline_styles: true,
            inline_assets_below_bytes: 64 * 1024,
            max_total_bytes: 1024 * 1024,
            offline: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SingleHtmlError {
    #[error("single HTML output too large: {size} exceeds {limit}")]
    TooLarge { size: usize, limit: usize },
}

pub fn emit_single_html_from_parts(
    script: &str,
    style: &str,
    options: SingleFileHtmlOptions,
) -> Result<String, SingleHtmlError> {
    let mut scripts = String::new();
    let mut styles = String::new();

    if options.inline_styles && !style.is_empty() {
        styles.push_str("<style>\n");
        styles.push_str(style);
        styles.push_str("\n</style>\n");
    }

    if options.inline_scripts && !script.is_empty() {
        scripts.push_str("<script type=\"module\">\n");
        scripts.push_str(&escape_inline_script(script));
        scripts.push_str("\n</script>\n");
    }

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
{styles}
<title>Forge App</title>
</head>
<body>
<div id="__forge_root"></div>
<script type="application/json" id="__FORGE_DATA__">{{"schemaVersion":1,"singleFile":true}}</script>
{scripts}
</body>
</html>"#
    );

    if html.len() > options.max_total_bytes {
        return Err(SingleHtmlError::TooLarge {
            size: html.len(),
            limit: options.max_total_bytes,
        });
    }

    Ok(html)
}

pub fn escape_inline_script(code: &str) -> String {
    code.replace("</script", "<\\/script")
}
