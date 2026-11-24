//! The login page logic of `reddit_browser`: `forager browser login reddit` opens the site's
//! login page and counts the login as done once a home page document marks its session logged
//! in, as the site's `shreddit-app` element does with `user-logged-in="true"`.

use reqwest::Url;
use scraper::{Html, Selector};

use super::SITE_HOST;
use crate::providers::browser::{CaptureLevel, Exchange, LoginPage, RequestMethod, ResponseBody};

const LOGIN_URL: &str = "https://www.reddit.com/login/";

pub(crate) const LOGIN_PAGE: LoginPage = LoginPage {
    entry_url: LOGIN_URL,
    capture,
    logged_in,
};

/// Keeps the home page documents, where the site lands once a login succeeds.
fn capture(url: &Url) -> CaptureLevel {
    if is_home(url) {
        CaptureLevel::Body
    } else {
        CaptureLevel::Ignore
    }
}

fn is_home(url: &Url) -> bool {
    url.host_str() == Some(SITE_HOST) && url.path() == "/"
}

fn logged_in(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|exchange| {
        exchange.method == RequestMethod::Get
            && is_home(&exchange.url)
            && matches!(&exchange.body, ResponseBody::Received(body) if marks_logged_in(body))
    })
}

/// Reads only the `shreddit-app` start tag: a home page document is about half a megabyte, and
/// the login wait checks every document again on each poll.
fn marks_logged_in(html: &str) -> bool {
    let Some(start) = html.find("<shreddit-app") else {
        return false;
    };
    let Some(length) = html[start..].find('>') else {
        return false;
    };
    let tag = Html::parse_fragment(&format!("{}</shreddit-app>", &html[start..=start + length]));
    let app = Selector::parse("shreddit-app").expect("a valid selector");
    tag.select(&app)
        .next()
        .is_some_and(|app| app.attr("user-logged-in") == Some("true"))
}

#[cfg(test)]
mod tests {
    use reqwest::Url;

    use super::logged_in;
    use crate::providers::browser::{Exchange, RequestMethod, ResponseBody};
    use crate::providers::reddit_browser::tests::stored;

    /// The stored home page document of a logged-in session.
    fn home() -> String {
        stored("home.html")
    }

    fn document(url: &str, body: &str) -> Exchange {
        Exchange {
            sequence: 0,
            method: RequestMethod::Get,
            url: Url::parse(url).expect("site URL"),
            status: Some(200),
            request_body: None,
            body: ResponseBody::Received(body.to_owned()),
        }
    }

    #[test]
    fn a_home_page_whose_app_is_marked_logged_in_is_a_login() {
        assert!(logged_in(&[document("https://www.reddit.com/", &home())]));
    }

    #[test]
    fn a_home_page_without_the_affirmative_mark_or_another_page_is_no_login() {
        let pages = [
            document(
                "https://www.reddit.com/",
                &home().replace(r#"user-logged-in="true""#, r#"user-logged-in="false""#),
            ),
            // The mark moved from the app to an element inside it.
            document(
                "https://www.reddit.com/",
                &home().replace(
                    r#" user-logged-in="true" theme="light">"#,
                    r#" theme="light"><p user-logged-in="true"></p>"#,
                ),
            ),
            document("https://www.reddit.com/login/", &home()),
        ];

        assert!(!logged_in(&pages));
    }
}
