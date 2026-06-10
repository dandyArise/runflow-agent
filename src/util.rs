pub fn slugify(input: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

pub fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

pub fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let idx = text.find(&marker)?;
    let rest = &text[idx + marker.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    parse_json_string_content(rest)
}

pub fn extract_json_number(text: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let idx = text.find(&marker)?;
    let rest = &text[idx + marker.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let number = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect::<String>();
    if number.is_empty() {
        None
    } else {
        Some(number)
    }
}

fn parse_json_string_content(rest: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000C}'),
                'u' => {
                    let code = chars.by_ref().take(4).collect::<String>();
                    if let Ok(value) = u16::from_str_radix(&code, 16) {
                        if let Some(decoded) = char::from_u32(value as u32) {
                            out.push(decoded);
                        }
                    }
                }
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}
