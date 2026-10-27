// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! HTML5 and CSS token-based extraction of static resource references.
use cssparser::ParseError;
use cssparser::Parser;
use cssparser::Token;
use scraper::Html;
use scraper::Selector;

use crate::pages_error::PagesError;
fn css_tokens(parser: &mut Parser<'_>, image_set: bool) -> Vec<String> {
    let mut urls = Vec::new();
    let mut import = false;
    while let Ok(token) = parser.next_including_whitespace_and_comments().cloned() {
        match token {
            Token::UnquotedUrl(url) => {
                urls.push(url.to_string());
                import = false;
            }
            Token::QuotedString(url) if import || image_set => {
                urls.push(url.to_string());
                import = false;
            }
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => import = true,
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                if let Ok(url) =
                    parser.parse_nested_block(|inner| Ok::<_, ParseError<()>>(inner.expect_string_cloned()?))
                {
                    urls.push(url.to_string());
                }
                import = false;
            }
            Token::Function(name)
                if name.eq_ignore_ascii_case("image-set") || name.eq_ignore_ascii_case("-webkit-image-set") =>
            {
                if let Ok(nested) = parser.parse_nested_block(|inner| Ok::<_, ParseError<()>>(css_tokens(inner, true)))
                {
                    urls.extend(nested);
                }
                import = false;
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::CurlyBracketBlock | Token::SquareBracketBlock => {
                if let Ok(nested) = parser.parse_nested_block(|inner| Ok::<_, ParseError<()>>(css_tokens(inner, false)))
                {
                    urls.extend(nested);
                }
                import = false;
            }
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            _ => import = false,
        }
    }
    urls
}
fn css(text: &str) -> Vec<String> {
    css_tokens(&mut Parser::new(text), false)
}
fn srcset(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let url = &rest[..end];
        urls.push(url.trim_end_matches(',').to_owned());
        rest = &rest[end..];
        if !url.ends_with(',') {
            if let Some(comma) = rest.find(',') {
                rest = &rest[comma + 1..];
            } else {
                break;
            }
        }
    }
    urls
}
fn resolve(values: Vec<String>, base: &url::Url, document_url: &url::Url) -> Result<Vec<String>, PagesError> {
    values
        .into_iter()
        .map(|value| {
            let target = base
                .join(value.trim())
                .map_err(|error| PagesError::Gate(format!("invalid static URL: {error}")))?;
            if target.origin() == document_url.origin() {
                let mut value = target.path().to_owned();
                if let Some(query) = target.query() {
                    value.push('?');
                    value.push_str(query);
                }
                if let Some(fragment) = target.fragment() {
                    value.push('#');
                    value.push_str(fragment);
                }
                Ok(value)
            } else {
                Ok(target.to_string())
            }
        })
        .collect()
}
pub(crate) fn urls(text: &str, extension: &str, document_url: &url::Url) -> Result<Vec<String>, PagesError> {
    if extension.eq_ignore_ascii_case("css") {
        return resolve(css(text), document_url, document_url);
    }
    let document = Html::parse_document(text);
    let selector = Selector::parse("*").map_err(PagesError::configuration)?;
    let base = document
        .select(&selector)
        .filter(|element| element.value().name() == "base")
        .find_map(|element| element.value().attr("href"));
    let base = base
        .map(|base| document_url.join(base))
        .transpose()
        .map_err(|error| PagesError::Gate(format!("invalid HTML base URL: {error}")))?
        .unwrap_or_else(|| document_url.clone());
    let mut result = Vec::new();
    for element in document.select(&selector) {
        if element.value().name() == "base" {
            continue;
        }
        if element.value().name() == "meta"
            && element
                .value()
                .attr("http-equiv")
                .is_some_and(|value| value.eq_ignore_ascii_case("refresh"))
            && let Some((_, navigation)) = element
                .value()
                .attr("content")
                .and_then(|content| content.split_once([';', ',']))
        {
            let navigation = navigation.trim();
            let target = if let Some((name, value)) = navigation.split_once('=')
                && name.trim().eq_ignore_ascii_case("url")
            {
                value.trim()
            } else {
                navigation
            };
            let target = if let Some(quote) = target.chars().next().filter(|quote| matches!(quote, '\'' | '"')) {
                target[1..].split(quote).next().unwrap_or("")
            } else {
                target
            };
            if !target.is_empty() {
                result.push(target.to_owned());
            }
        }
        if element.value().name() == "style" {
            result.extend(css(&element.text().collect::<String>()));
        }
        for (name, value) in element.value().attrs() {
            match name {
                "href" | "src" | "action" | "poster" | "cite" | "background" | "longdesc" | "manifest"
                | "formaction" | "xlink:href" => result.push(value.to_owned()),
                "data" if element.value().name() == "object" => result.push(value.to_owned()),
                "srcset" | "imagesrcset" => result.extend(srcset(value)),
                "style" => result.extend(css(value)),
                _ => {}
            }
        }
    }
    resolve(result, &base, document_url)
}
