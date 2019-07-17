#![feature(decl_macro)]
#![feature(generators, generator_trait)]

mod generators;

use failure::Fail;

use reqwest::{self, IntoUrl, StatusCode, Url};
use reqwest::header::{self, HeaderValue};

use select::document::Document;
use select::predicate::{Attr, Class, Name, Predicate};

use std::collections::{HashSet, VecDeque};

use crate::generators::gen_iter;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct WebPageInfo {
    title: String,
    links: Vec<Url>,
}

#[derive(Debug, Fail)]
enum FetchWebPageError {
    #[fail(display = "{}", _0)]
    HttpError(#[cause] reqwest::Error),
    #[fail(display = "bad HTTP status: {}", _0)]
    BadHttpStatus(StatusCode),
    #[fail(display = "missing HTTP content type")]
    MissingContentType,
    #[fail(display = "bad HTTP content type: {:?}", _0)]
    BadContentType(HeaderValue),
    #[fail(display = "text decoding error: {:?}", _0)]
    TextDecodeError(#[cause] reqwest::Error),
}

#[derive(Debug, Fail)]
enum GetWebPageInfoError {
    #[fail(display = "document has no title")]
    NoTitle,
}

fn fetch_web_page(url: impl IntoUrl) -> Result<Document, FetchWebPageError> {
    let mut resp = reqwest::get(url).map_err(FetchWebPageError::HttpError)?;

    if !resp.status().is_success() {
        return Err(FetchWebPageError::BadHttpStatus(resp.status()));
    }

    if let Some(content_type) = resp.headers().get(header::CONTENT_TYPE) {
        if let Ok("text/html") = content_type.to_str() {
            return Err(FetchWebPageError::BadContentType(content_type.clone()));
        }
    } else {
        return Err(FetchWebPageError::MissingContentType);
    }

    let text = resp.text().map_err(FetchWebPageError::TextDecodeError)?;
    // NOTE: 'select' may not be the most robust library, since it doesn't even return potential HTML parsing errors!
    let doc = (&*text).into();
    Ok(doc)
}

fn get_web_page_info(doc: Document) -> Result<WebPageInfo, GetWebPageInfoError> {
    let title_node = doc.find(Name("title")).next().ok_or(GetWebPageInfoError::NoTitle)?;
    let title = title_node.text().trim().into();

    let anchor_nodes = doc.find(Name("a"));
    let links = anchor_nodes.filter_map(|n| {
        // Ignore anchors without `href` attribute or with invalid URLs.
        n.attr("href").and_then(|s| s.parse().ok())
    }).collect();

    Ok(WebPageInfo {
        title,
        links,
    })
}

// NOTE: ideally we'd make this a stream of futures (`FuturesUnordered`) and leverage parallelism, but this would take a lot more effort and care.
// NOTE: this could be expanded to use a library like 'robotparser' to respect websites that use a `robots.txt` to stop crawlers from indexing certain pages.
fn crawl_web_page(url: impl IntoUrl) -> impl Iterator<Item = (Url, WebPageInfo)> {
    gen_iter! {
        let mut urls_visited = HashSet::new();
        let mut urls_to_visit = VecDeque::new();
        if let Ok(url) = url.into_url() {
            urls_to_visit.push_back(url);
        }

        while let Some(url) = urls_to_visit.pop_front() {
            urls_visited.insert(url.clone());
            if let Ok(doc) = fetch_web_page(url.clone()) {
                if let Ok(page) = get_web_page_info(doc) {
                    for link_url in &page.links {
                        // Ignore already-visited pages, so we don't get cycles.
                        if !urls_visited.contains(link_url) {
                            urls_to_visit.push_back(link_url.clone());
                        }
                    }
                    yield (url.clone(), page);
                }
            }
        }
    }
}

// NOTE: ideally the test harness would spawn a temporary local HTTP server so as not to rely on the Web.
#[cfg(test)]
mod tests {
    use is_match::is_match;

    use super::*;

    #[test]
    fn test_fetch_web_page() {
        assert!(fetch_web_page("http://google.com/").is_ok());
        assert!(fetch_web_page("http://bing.com/").is_ok());
        assert!(fetch_web_page("https://en.wikipedia.org/wiki/Rust_(programming_language)").is_ok());

        assert!(is_match!(fetch_web_page("http://not.a.domain/"), Err(FetchWebPageError::HttpError(_))));

        assert!(is_match!(fetch_web_page("http://google.com/not_a_valid_url"), Err(FetchWebPageError::BadHttpStatus(StatusCode::NOT_FOUND))));

        // TODO: test other sorts of errors here.
    }

    #[test]
    fn test_web_page_info() {
        let doc = fetch_web_page("http://rust-lang.org/").unwrap();
        let doc_info = get_web_page_info(doc).unwrap();
        assert_eq!(doc_info.title, "Rust Programming Language");
        assert!(doc_info.links.contains(&"https://blog.rust-lang.org/".parse().unwrap()));
        assert!(doc_info.links.contains(&"https://doc.rust-lang.org/".parse().unwrap()));
        assert!(doc_info.links.contains(&"https://users.rust-lang.org/".parse().unwrap()));

        // TODO: test info retrieved from other websites.

        // TODO: check for web page with no title.
    }

    #[test]
    fn test_crawl_web_page() {
        let pages = crawl_web_page("http://rust-lang.org/");

        let initial_pages: Vec<_> = pages.take(10).map(|(url, page)| (url.to_string(), page.title)).collect();
        assert_eq!(&initial_pages[0],
            &("http://rust-lang.org/".to_owned(), "Rust Programming Language".to_owned())
        );
        assert!(initial_pages.contains(
            &("https://blog.rust-lang.org/".to_owned(), "The Rust Programming Language Blog".to_owned())
        ));
        assert!(initial_pages.contains(
            &("https://github.com/rust-lang/rust/blob/master/CONTRIBUTING.md".to_owned(), "rust/CONTRIBUTING.md at master · rust-lang/rust · GitHub".to_owned())
        ));

        // TODO: test other websites, cyclic cases, pages with dead/invalid links, etc.
    }
}
