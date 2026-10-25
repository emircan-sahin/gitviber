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
        // Comments on the diff's lines: not the conversation's, a list of their own.
        paths.push(format!("{base}/pulls/{number}/comments"));
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
    use super::super::client::fake;
    use super::*;
    use serde_json::json;
    use std::process::Command;

    /// A repository whose origin is github.com/o/r; nothing more is read from it.
    fn repo(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitviber-attach-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap()
                .success());
        }
        dir
    }

    /// Attachment `n`'s id, and an image of it as GitHub renders one.
    fn img(n: u32) -> (String, String) {
        let id = format!("012a2451-fa01-4fe5-8736-33f4a4d179f{n}");
        let url =
            format!("https://private-user-images.githubusercontent.com/1/{n}-{id}.png?jwt=x{n}");
        (id, format!(r#"<img src="{url}">"#))
    }

    fn list(html: &str) -> String {
        json!([{ "body_html": html }]).to_string()
    }

    #[test]
    fn a_pull_requests_line_comments_are_signed_too() {
        let dir = repo("pull");
        let (ids, html): (Vec<_>, Vec<_>) = (1..=4).map(img).unzip();
        let (session, log) = fake::serve(vec![
            (
                "/repos/o/r/issues/7".into(),
                json!({ "body_html": html[0], "pull_request": {} }).to_string(),
            ),
            ("/repos/o/r/issues/7/comments".into(), list(&html[1])),
            ("/repos/o/r/pulls/7/reviews".into(), list(&html[2])),
            ("/repos/o/r/pulls/7/comments".into(), list(&html[3])),
        ]);
        let out = attachments(&session, &dir, None, 7).unwrap();
        for id in &ids {
            assert!(out.contains_key(id), "{id} missing: {out:?}");
        }
        let log = log.lock().unwrap();
        assert!(log.iter().all(|r| r.starts_with("GET ")), "{log:?}");
        assert!(log
            .iter()
            .any(|r| r.starts_with("GET /repos/o/r/pulls/7/comments?")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_issue_asks_for_no_pull_request_lists() {
        let dir = repo("issue");
        let (id, html) = img(1);
        let (session, log) = fake::serve(vec![
            (
                "/repos/o/r/issues/3".into(),
                json!({ "body_html": html }).to_string(),
            ),
            ("/repos/o/r/issues/3/comments".into(), "[]".into()),
        ]);
        let out = attachments(&session, &dir, None, 3).unwrap();
        assert_eq!(out.keys().collect::<Vec<_>>(), [&id]);
        let log = log.lock().unwrap();
        assert!(!log.iter().any(|r| r.contains("/pulls/")), "{log:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
