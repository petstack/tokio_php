//! Request parsing utilities.

use std::borrow::Cow;

use crate::types::ParamList;

/// Fast percent decode - returns Cow to avoid allocation when no decoding needed.
#[inline]
pub fn fast_percent_decode(s: &str) -> Cow<'static, str> {
    if s.contains('%') {
        Cow::Owned(
            percent_encoding::percent_decode_str(s)
                .decode_utf8_lossy()
                .into_owned(),
        )
    } else {
        Cow::Owned(s.to_string())
    }
}

/// Parse a query string into key-value pairs.
///
/// Returns `ParamList` (Vec of Cow pairs) - all values are dynamic (Owned).
#[inline]
pub fn parse_query_string(query: &str) -> ParamList {
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
///
/// Returns `ParamList` (Vec of Cow pairs) - all values are dynamic (Owned).
#[inline]
pub fn parse_cookies(cookie_header: &str) -> ParamList {
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
            cookies.push((Cow::Owned(name.to_string()), fast_percent_decode(value)));
        }
    }

    cookies
}
