//! Query API feeds derived from the stored `arxiv_api/feed.xml` sample, shared by the command
//! contract tests and, through the route's in-crate tests, the crate. A variant changes only what
//! its test is about, so every other element stays as the sample has it.

/// The paper the stored feed holds, out of 42 results.
pub(crate) const STORED_ID: &str = "2401.01234v2";

/// `feed` with its paper renamed `id`.
pub(crate) fn feed_of(feed: &str, id: &str) -> String {
    feed.replace(STORED_ID, id)
}

/// A page of `total` results holding the entry of `feed` once for each of `ids`.
pub(crate) fn page(feed: &str, total: u64, ids: &[&str]) -> String {
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

/// `xml` without every element named in `names`, such as `arxiv:doi`.
pub(crate) fn without(xml: &str, names: &[&str]) -> String {
    let mut xml = xml.to_owned();
    for name in names {
        while let Some(start) = xml
            .find(&format!("<{name}>"))
            .or_else(|| xml.find(&format!("<{name} ")))
        {
            let rest = &xml[start..];
            let close = format!("</{name}>");
            let end = match (rest.find("/>"), rest.find('>'), rest.find(&close)) {
                (Some(empty), Some(open), _) if empty + 1 == open => start + empty + 2,
                (_, _, Some(closing)) => start + closing + close.len(),
                _ => panic!("element `{name}` does not end"),
            };
            let line_start = xml[..start].rfind('\n').map_or(start, |newline| newline);
            let cut_from = if xml[line_start..start].trim().is_empty() {
                line_start
            } else {
                start
            };
            xml.replace_range(cut_from..end, "");
        }
    }
    xml
}
