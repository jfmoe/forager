//! The login page logic of `xiaohongshu_browser`: `forager browser login xiaohongshu` opens the
//! site's home page, where a logged-out visitor gets the QR code login, and counts the login as
//! done once the page's own account request answers for a non-guest user.

use reqwest::Url;
use serde_json::Value;

use super::{ACCOUNT_PATH, HOME_URL, on_site};
use crate::providers::browser::{CaptureLevel, Exchange, LoginPage, RequestMethod, ResponseBody};

pub(crate) const LOGIN_PAGE: LoginPage = LoginPage {
    entry_url: HOME_URL,
    capture,
    logged_in,
};

fn capture(url: &Url) -> CaptureLevel {
    if on_site(url) && url.path() == ACCOUNT_PATH {
        CaptureLevel::Body
    } else {
        CaptureLevel::Ignore
    }
}

/// A logged-out page answers the account request with `code` -101, or with a guest user; a
/// logged-in one answers `code` 0 with `guest: false` and the user's id.
fn logged_in(exchanges: &[Exchange]) -> bool {
    exchanges
        .iter()
        .filter(|exchange| {
            exchange.method == RequestMethod::Get && exchange.url.path() == ACCOUNT_PATH
        })
        .filter_map(|exchange| match &exchange.body {
            ResponseBody::Received(body) => serde_json::from_str::<Value>(body).ok(),
            _ => None,
        })
        .any(|account| {
            account["code"] == 0
                && account["data"]["guest"] == false
                && account["data"]["user_id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use reqwest::Url;
    use serde_json::json;

    use super::{ACCOUNT_PATH, logged_in};
    use crate::providers::browser::{Exchange, RequestMethod, ResponseBody};

    fn account(body: &serde_json::Value) -> Exchange {
        Exchange {
            method: RequestMethod::Get,
            url: Url::parse(&format!("https://edith.xiaohongshu.com{ACCOUNT_PATH}"))
                .expect("account URL"),
            status: Some(200),
            request_body: None,
            body: ResponseBody::Received(body.to_string()),
        }
    }

    #[test]
    fn a_non_guest_account_answer_is_a_login() {
        let answer =
            json!({"code": 0, "success": true, "data": {"guest": false, "user_id": "5f0a"}});

        assert!(logged_in(&[account(&answer)]));
    }

    #[test]
    fn a_guest_or_missing_session_is_no_login() {
        let answers = [
            json!({"code": 0, "success": true, "data": {"guest": true, "user_id": "6a1b"}}),
            json!({"code": -101, "success": false, "msg": "无登录信息，或登录信息为空", "data": {}}),
        ];

        assert!(!logged_in(&answers.map(|answer| account(&answer))));
    }
}
