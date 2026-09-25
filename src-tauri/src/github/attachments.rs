//! Signed links for images attached to PRs and issues in private repositories.

use super::{all_pages, request, target, Method, Session};
use std::collections::HashMap;
use std::path::Path;

/// Signed image links for a PR's or issue's attachments, by attachment id. In a private
/// repo, `github.com/user-attachments/assets/<id>` needs a github.com login the webview
/// doesn't have; the API's rendered HTML carries short-lived signed links instead, so the
/// token itself never leaves api.github.com.
pub fn attachments(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<HashMap<String, String>, String> {
    const HTML: &str = "application/vnd.github.html+json";
    let r = target(session, repo, to)?;
    let base = r.api("");
    let mut out = HashMap::new();
    // The issues endpoint serves PRs too, and says which one this is.
    let thread = request(
        session,
        repo,
        Method::Get,
        &format!("{base}/issues/{number}"),
        HTML,
    )?;
    signed_images(thread["body_html"].as_str().unwrap_or_default(), &mut out);
    let mut paths = vec![format!("{base}/issues/{number}/comments")];
    if thread.get("pull_request").is_some() {
        paths.push(format!("{base}/pulls/{number}/reviews"));
    }
    for path in paths {
        if let Ok(list) = all_pages(session, repo, &path, HTML) {
            for c in list {
                signed_images(c["body_html"].as_str().unwrap_or_default(), &mut out);
            }
        }
    }
    Ok(out)
}

/// `…githubusercontent.com/<user>/<n>-<id>.<ext>?jwt=…`: the id is the attachment's UUID.
fn signed_images(html: &str, out: &mut HashMap<String, String>) {
    // Only image sources: the same URL as link text or an href is anyone's to write.
    const SRC: &str = "src=\"https://private-user-images.githubusercontent.com/";
    for (i, _) in html.match_indices(SRC) {
        let url = html[i + 5..]
            .split(['"', '\'', ' ', '<', '>'])
            .next()
            .unwrap_or_default()
            .replace("&amp;", "&");
        let name = url.split('?').next().unwrap_or_default();
        let stem = name.rsplit('/').next().unwrap_or_default();
        let stem = stem.split('.').next().unwrap_or_default();
        let Some(id) = stem.len().checked_sub(36).and_then(|n| stem.get(n..)) else {
            continue;
        };
        let uuid = id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
        if uuid {
            // The description comes first, then comments in order: a later comment reusing
            // an id can't replace the link the description's image resolves to.
            out.entry(id.to_string()).or_insert(url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_image_links() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        let url = format!("https://private-user-images.githubusercontent.com/23744935/650874155-{id}.png?jwt=eyJ.x&amp;y=1");
        let html = format!(
            r#"<a href="{url}"><img src="{url}" alt="x"></a> <img src="https://private-user-images.githubusercontent.com/1/2-nope.png?jwt=z">"#
        );
        let mut out = HashMap::new();
        signed_images(&html, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[id], url.replace("&amp;", "&"));
    }

    #[test]
    fn signed_links_first_wins_and_only_from_images() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        let url = |n: u32| {
            format!("https://private-user-images.githubusercontent.com/1/{n}-{id}.png?jwt=x{n}")
        };
        let mut out = HashMap::new();
        signed_images(&format!(r#"<img src="{}">"#, url(1)), &mut out);
        signed_images(
            &format!(
                r#"<img src="{}"> <a href="{}">{}</a>"#,
                url(2),
                url(3),
                url(4)
            ),
            &mut out,
        );
        assert_eq!(out[id], url(1));
        let mut out = HashMap::new();
        signed_images(&format!(r#"<a href="{}">{}</a>"#, url(3), url(4)), &mut out);
        assert!(out.is_empty());
    }
}
