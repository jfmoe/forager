//! `youtube_web` fixtures for the command contract tests, served from the stored samples in
//! `tests/samples/youtube_web/`.

use super::Response;
use super::samples::sample;

/// The origin of the stored player's caption tracks.
const STORED_ORIGIN: &str = "https://www.youtube.com";

/// Enables `youtube_web` alone, pointed at the fixture server.
pub(crate) fn config(url: &str) -> String {
    format!(
        "[providers.youtube_web]\nurl = \"{url}\"\n\n[platforms.youtube]\norder = [\"youtube_web\"]\n"
    )
}

/// The stored watch page.
pub(crate) fn watch_page() -> Response {
    Response::new(
        200,
        "text/html; charset=utf-8",
        &sample("youtube_web/watch.html"),
    )
}

/// The stored ANDROID player response, its caption tracks served from `origin`.
pub(crate) fn player(origin: &str) -> Response {
    Response::json(
        200,
        &sample("youtube_web/player.json").replace(STORED_ORIGIN, origin),
    )
}

/// The stored srv1 caption track.
pub(crate) fn srv1() -> Response {
    Response::new(
        200,
        "text/xml; charset=UTF-8",
        &sample("youtube_web/srv1.xml"),
    )
}
