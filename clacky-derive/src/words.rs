//! Names in words: `HttpServer` and `http_server` both read "Http server".

/// Acronyms stay whole: `HTTPServer` reads "HTTP server".
pub(crate) fn words(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut found: Vec<String> = Vec::new();
    let mut fresh = true;
    for (at, &c) in chars.iter().enumerate() {
        if c == '_' {
            fresh = true;
        } else {
            let before = at.checked_sub(1).and_then(|back| chars.get(back)).copied();
            let after = chars.get(at + 1).copied();
            let boundary = c.is_uppercase()
                && (before.is_some_and(|b| b.is_lowercase() || b.is_ascii_digit())
                    || (before.is_some_and(char::is_uppercase)
                        && after.is_some_and(char::is_lowercase)));
            match found.last_mut() {
                Some(word) if !fresh && !boundary => word.push(c),
                _ => found.push(c.to_string()),
            }
            fresh = false;
        }
    }
    found
        .iter()
        .enumerate()
        .map(|(at, word)| {
            let acronym =
                word.chars().count() > 1 && word.chars().all(|c| !c.is_lowercase());
            if acronym {
                word.clone()
            } else if at == 0 {
                capitalised(word)
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn names_split_into_words() {
        assert_eq!(words("Lib"), "Lib");
        assert_eq!(words("HttpServer"), "Http server");
        assert_eq!(words("http_server"), "Http server");
        assert_eq!(words("IOError"), "IO error");
        assert_eq!(words("other_kind_2"), "Other kind 2");
        assert_eq!(words("Ipv4Addr"), "Ipv4 addr");
        assert_eq!(words("_x"), "X");
        assert_eq!(words(""), "");
    }
}
