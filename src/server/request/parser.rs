//! Request parsing utilities.

/// Fast percent decode - only allocates if '%' is present.
#[inline]
pub fn fast_percent_decode(s: &str) -> String {
    if s.contains('%') {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    } else {
        s.to_string()
    }
}

/// Parse a query string into key-value pairs.
#[inline]
pub fn parse_query_string(query: &str) -> Vec<(String, String)> {
    let pair_count = query.matches('&').count() + 1;
    let mut params = Vec::with_capacity(pair_count.min(16));

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let (key, value) = match pair.find('=') {
            Some(pos) => (&pair[..pos], &pair[pos + 1..]),
            None => (pair, ""),
        };

        if !key.is_empty() {
            params.push((fast_percent_decode(key), fast_percent_decode(value)));
        }
    }

    params
}

/// Parse a Cookie header into name-value pairs.
#[inline]
pub fn parse_cookies(cookie_header: &str) -> Vec<(String, String)> {
    let cookie_count = cookie_header.matches(';').count() + 1;
    let mut cookies = Vec::with_capacity(cookie_count.min(16));

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if cookie.is_empty() {
            continue;
        }

        let (name, value) = match cookie.find('=') {
            Some(pos) => (cookie[..pos].trim(), cookie[pos + 1..].trim()),
            None => continue,
        };

        if !name.is_empty() {
            cookies.push((name.to_string(), fast_percent_decode(value)));
        }
    }

    cookies
}
