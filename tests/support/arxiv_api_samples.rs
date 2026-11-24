//! Query API feeds derived from the stored `arxiv_api/feed.xml` sample, shared by the command
//! contract tests and, through the crate root, the in-crate tests. A variant changes only what
//! its test is about, so every other element stays as the sample has it.

use super::samples::sample;

/// The paper the stored feed holds, out of 42 results.
pub(crate) const STORED_ID: &str = "2401.01234v2";

/// The stored feed with its paper renamed `id`.
pub(crate) fn feed_of(id: &str) -> String {
    sample("arxiv_api/feed.xml").replace(STORED_ID, id)
}

/// The stored feed as a page of `total` results holding its entry once for each of `ids`.
pub(crate) fn page(total: u64, ids: &[&str]) -> String {
    let feed = sample("arxiv_api/feed.xml");
    let start = feed.find("<entry>").expect("the stored feed has an entry");
    let end = feed.find("</entry>").expect("the stored entry ends") + "</entry>".len();
    let entries = ids
        .iter()
        .map(|id| feed[start..end].replace(STORED_ID, id))
        .collect::<String>();
    format!("{}{entries}{}", &feed[..start], &feed[end..]).replace(
        "<opensearch:totalResults>42</opensearch:totalResults>",
        &format!("<opensearch:totalResults>{total}</opensearch:totalResults>"),
    )
}
