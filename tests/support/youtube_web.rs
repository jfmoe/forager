//! YouTube web fixtures for `youtube_web`: a watch page, ANDROID player responses, and srv1
//! caption tracks, trimmed from the anonymous responses of 2026-10-17 to the fields the route
//! reads.

use serde_json::{Value, json};

use super::Response;

/// The public `InnerTube` key the fixture watch page carries.
pub(crate) const INNERTUBE_KEY: &str = "AIzaSyFixtureInnertubeKey_0123456789";
pub(crate) const ZOO: &str = "jNQXAC9IVRw";
pub(crate) const ZOO_DESCRIPTION: &str =
    "The first video on YouTube.\n\n00:00 Intro\n00:05 The cool thing\n00:17 End";
/// The English srv1 track of "Me at the zoo", as YouTube served it without `fmt`.
pub(crate) const ZOO_SRV1_EN: &str = r#"<?xml version="1.0" encoding="utf-8" ?><transcript><text start="1.2" dur="2.16">All right, so here we are, in front of the
elephants</text><text start="5.318" dur="2.656">the cool thing about these guys is that they
have really...</text><text start="7.974" dur="4.642">really really long trunks</text><text start="12.616" dur="1.751">and that&amp;#39;s cool</text><text start="14.421" dur="1.312">(baaaaaaaaaaahhh!!)</text><text start="16.881" dur="2">and that&amp;#39;s pretty much all there is to say</text></transcript>"#;

/// Enables `youtube_web` alone, pointed at the fixture server.
pub(crate) fn config(url: &str) -> String {
    format!(
        "[providers.youtube_web]\nurl = \"{url}\"\n\n[platforms.youtube]\norder = [\"youtube_web\"]\n"
    )
}

/// The `videoDetails` of "Me at the zoo".
pub(crate) fn zoo_details(description: &str) -> Value {
    json!({
        "videoId": ZOO,
        "title": "Me at the zoo",
        "lengthSeconds": "19",
        "keywords": ["me at the zoo"],
        "channelId": "UC4QobU6STFB0P71PMvOGN5A",
        "isOwnerViewing": false,
        "shortDescription": description,
        "isCrawlable": true,
        "allowRatings": true,
        "viewCount": "444721399",
        "author": "jawed",
        "isPrivate": false,
        "isUnpluggedCorpus": false,
        "isLiveContent": false
    })
}

/// The player response the watch page embeds: its caption URLs need a proof-of-origin token,
/// which is why the route asks the ANDROID player for tracks.
pub(crate) fn zoo_embedded(description: &str) -> Value {
    json!({
        "playabilityStatus": {"status": "OK", "playableInEmbed": true},
        "captions": {"playerCaptionsTracklistRenderer": {
            "captionTracks": [{
                "baseUrl": "https://www.youtube.com/api/timedtext?v=jNQXAC9IVRw&exp=xpe&lang=en",
                "name": {"simpleText": "English"},
                "vssId": ".en",
                "languageCode": "en",
                "isTranslatable": true
            }],
            "translationLanguages": [{"languageCode": "fr", "languageName": {"simpleText": "French"}}]
        }},
        "videoDetails": zoo_details(description),
        "microformat": {"playerMicroformatRenderer": {
            "title": {"simpleText": "Me at the zoo"},
            "lengthSeconds": "19",
            "ownerChannelName": "jawed",
            "viewCount": "444721399",
            "publishDate": "2005-04-23T20:31:52-07:00",
            "uploadDate": "2005-04-23T20:31:52-07:00",
            "likeCount": "20083234"
        }}
    })
}

/// A watch page that carries the `InnerTube` key and embeds `player`.
pub(crate) fn watch_page(player: &Value) -> Response {
    Response::new(
        200,
        "text/html; charset=utf-8",
        &format!(
            "<!DOCTYPE html><html><head><script nonce=\"n\">var ytInitialPlayerResponse = {player};var meta = document.createElement('meta');</script>\
             <script nonce=\"n\">ytcfg.set({{\"INNERTUBE_API_KEY\": \"{INNERTUBE_KEY}\",\"INNERTUBE_CLIENT_NAME\":\"WEB\"}});</script></head><body></body></html>"
        ),
    )
}

/// The page YouTube serves an egress IP it wants to verify: a bot check and no key.
pub(crate) fn bot_check_page() -> Response {
    Response::new(
        200,
        "text/html; charset=utf-8",
        "<html><body><form action=\"/das_captcha\"><div class=\"g-recaptcha\" data-sitekey=\"k\"></div></form></body></html>",
    )
}

/// One caption track as the ANDROID player lists it, served from `origin`.
pub(crate) fn track(origin: &str, language: &str, asr: bool) -> Value {
    let caps = if asr { "&caps=asr" } else { "" };
    let mut track = json!({
        "baseUrl": format!("{origin}/api/timedtext?v={ZOO}&ei=fixture{caps}&xoaf=5&hl=en&lang={language}&fmt=srv3"),
        "name": {"runs": [{"text": language}]},
        "vssId": format!(".{language}"),
        "languageCode": language,
        "isTranslatable": true,
        "trackName": ""
    });
    if asr {
        track["kind"] = json!("asr");
        track["vssId"] = json!(format!("a.{language}"));
    }
    track
}

/// An OK ANDROID player response for "Me at the zoo" with `tracks`; the player shows the track
/// at `default_track` by default.
pub(crate) fn zoo_player(tracks: &[Value], default_track: usize) -> Value {
    json!({
        "responseContext": {},
        "playabilityStatus": {"status": "OK", "playableInEmbed": true},
        "captions": {"playerCaptionsTracklistRenderer": {
            "captionTracks": tracks,
            "audioTracks": [{
                "captionTrackIndices": (0..tracks.len()).collect::<Vec<_>>(),
                "defaultCaptionTrackIndex": default_track,
                "hasDefaultTrack": true
            }],
            "defaultAudioTrackIndex": 0
        }},
        "videoDetails": zoo_details(ZOO_DESCRIPTION)
    })
}

/// The ANDROID player's answer to a request it refuses.
pub(crate) fn refusal(status: &str, reason: &str) -> Value {
    json!({
        "responseContext": {},
        "playabilityStatus": {"status": status, "reason": reason}
    })
}

pub(crate) fn json(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

pub(crate) fn srv1(xml: &str) -> Response {
    Response::new(200, "text/xml; charset=UTF-8", xml)
}
