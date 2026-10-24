//! What a PR or issue list is narrowed to, on top of open/closed: whose work (the chips), drafts,
//! labels and an author. The chips' choice is kept per repository; labels and an author aren't.

use gitviber_core::github::ListFilter;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListKind {
    Pulls,
    Issues,
}

impl ListKind {
    fn key(self) -> &'static str {
        match self {
            Self::Pulls => "pulls",
            Self::Issues => "issues",
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::Pulls => "pull requests",
            Self::Issues => "issues",
        }
    }
}

/// Whose pull requests or issues a list shows, as the account says "me".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Created,
    Assigned,
    Mentioned,
    ReviewRequested,
}

pub struct ScopeInfo {
    pub id: Scope,
    pub key: &'static str,
    pub label: &'static str,
    pulls_only: bool,
    phrase: &'static str,
}

/// The chips, in the order they show.
pub const SCOPES: [ScopeInfo; 4] = [
    ScopeInfo {
        id: Scope::Created,
        key: "created",
        label: "Created by me",
        pulls_only: false,
        phrase: "created by you",
    },
    ScopeInfo {
        id: Scope::Assigned,
        key: "assigned",
        label: "Assigned to me",
        pulls_only: false,
        phrase: "assigned to you",
    },
    ScopeInfo {
        id: Scope::Mentioned,
        key: "mentioned",
        label: "Mentions me",
        pulls_only: false,
        phrase: "that mention you",
    },
    ScopeInfo {
        id: Scope::ReviewRequested,
        key: "reviewRequested",
        label: "Review requested",
        pulls_only: true,
        phrase: "waiting for your review",
    },
];

pub fn scopes_for(kind: ListKind) -> impl Iterator<Item = &'static ScopeInfo> {
    SCOPES
        .iter()
        .filter(move |s| kind == ListKind::Pulls || !s.pulls_only)
}

fn info(scope: Scope) -> &'static ScopeInfo {
    SCOPES
        .iter()
        .find(|s| s.id == scope)
        .expect("every scope has its chip")
}

/// What the chips choose, kept for each repository.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Choice {
    pub scope: Option<Scope>,
    /// Pull requests only: Some(true) drafts, Some(false) those ready for review.
    pub draft: Option<bool>,
}

/// One repository's saved choices, for pulls and for issues: anything unreadable is "All".
pub fn parse_choice(saved: Option<&Value>, kind: ListKind) -> Choice {
    let Some(v) = saved
        .and_then(|s| s.get(kind.key()))
        .and_then(Value::as_object)
    else {
        return Choice::default();
    };
    let scope = v
        .get("scope")
        .and_then(Value::as_str)
        .and_then(|s| scopes_for(kind).find(|i| i.key == s))
        .map(|i| i.id);
    let draft = if kind == ListKind::Pulls {
        v.get("draft").and_then(Value::as_bool)
    } else {
        None
    };
    Choice { scope, draft }
}

/// `saved` with `choice` as `kind`'s; the other kind's stays.
pub fn with_choice(saved: Option<&Value>, kind: ListKind, choice: Choice) -> Value {
    let mut map = saved
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(Map::new);
    map.insert(
        kind.key().into(),
        json!({ "scope": choice.scope.map(|s| info(s).key), "draft": choice.draft }),
    );
    Value::Object(map)
}

/// What a list is narrowed to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Narrow {
    pub scope: Option<Scope>,
    pub draft: Option<bool>,
    /// Carrying all of them.
    pub labels: Vec<String>,
    /// Opened by this login, as the lists spell it.
    pub author: Option<String>,
}

impl Narrow {
    pub fn is_narrowed(&self) -> bool {
        self.scope.is_some()
            || self.draft.is_some()
            || !self.labels.is_empty()
            || self.author.is_some()
    }

    /// Tells one narrowing from another, for cache keys: "" for none, and the order labels were
    /// picked in doesn't matter. JSON: a label name may hold any separator.
    pub fn key(&self) -> String {
        if !self.is_narrowed() {
            return String::new();
        }
        let mut labels = self.labels.clone();
        labels.sort();
        json!([
            self.scope.map(|s| info(s).key),
            self.draft,
            labels,
            self.author
        ])
        .to_string()
    }

    /// core's filter. Its scope type isn't exported, so it's built the way the page sent it.
    pub fn to_core(&self) -> ListFilter {
        let v = json!({
            "scope": self.scope.map(|s| info(s).key),
            "draft": self.draft,
            "labels": self.labels,
            "author": self.author,
        });
        serde_json::from_value(v).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    #[default]
    Open,
    Closed,
    All,
}

impl Filter {
    pub const ALL: [Filter; 3] = [Filter::Open, Filter::Closed, Filter::All];

    /// core's name for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Closed => "Closed",
            Self::All => "All",
        }
    }
}

/// What an empty list says: "No open pull requests by mona assigned to you."
pub fn empty_text(kind: ListKind, filter: Filter, n: &Narrow) -> String {
    let mut words: Vec<&str> = Vec::new();
    if filter != Filter::All {
        words.push(filter.as_str());
    }
    match n.draft {
        Some(true) => words.push("draft"),
        Some(false) => words.push("non-draft"),
        None => {}
    }
    words.push(kind.noun());
    let mut tail: Vec<String> = Vec::new();
    if let Some(author) = &n.author {
        tail.push(format!("by {author}"));
    }
    if let Some(scope) = n.scope {
        tail.push(info(scope).phrase.into());
    }
    match n.labels.len() {
        0 => {}
        1 => tail.push("with this label".into()),
        _ => tail.push("with all these labels".into()),
    }
    let tail: String = tail.iter().map(|t| format!(" {t}")).collect();
    format!("No {}{tail}.", words.join(" "))
}

/// GitHub's own short counts: 1.2K, 12K.
pub fn compact_count(n: u64) -> String {
    let short = |v: f64, unit: &str| {
        let s = if v < 10.0 {
            format!("{:.1}", (v * 10.0).floor() / 10.0)
        } else {
            format!("{}", v.floor())
        };
        format!("{}{unit}", s.trim_end_matches(".0"))
    };
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => short(n as f64 / 1e3, "K"),
        _ => short(n as f64 / 1e6, "M"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn narrow(f: impl FnOnce(&mut Narrow)) -> Narrow {
        let mut n = Narrow::default();
        f(&mut n);
        n
    }

    #[test]
    fn pull_requests_offer_a_review_chip_issues_dont() {
        let keys = |k| scopes_for(k).map(|s| s.key).collect::<Vec<_>>();
        assert_eq!(
            keys(ListKind::Pulls),
            ["created", "assigned", "mentioned", "reviewRequested"]
        );
        assert_eq!(keys(ListKind::Issues), ["created", "assigned", "mentioned"]);
    }

    #[test]
    fn a_saved_choice_is_read_back_for_its_own_kind() {
        let saved = json!({ "pulls": { "scope": "reviewRequested", "draft": true }, "issues": { "scope": "assigned" } });
        assert_eq!(
            parse_choice(Some(&saved), ListKind::Pulls),
            Choice {
                scope: Some(Scope::ReviewRequested),
                draft: Some(true)
            }
        );
        assert_eq!(
            parse_choice(Some(&saved), ListKind::Issues),
            Choice {
                scope: Some(Scope::Assigned),
                draft: None
            }
        );
    }

    #[test]
    fn anything_unreadable_or_not_offered_for_the_kind_is_all() {
        for bad in [
            json!(null),
            json!("x"),
            json!(3),
            json!([]),
            json!({ "pulls": "created" }),
            json!({ "pulls": { "scope": "everyone", "draft": "yes" } }),
        ] {
            assert_eq!(parse_choice(Some(&bad), ListKind::Pulls), Choice::default());
        }
        assert_eq!(parse_choice(None, ListKind::Pulls), Choice::default());
        let hand_edited = json!({ "issues": { "scope": "reviewRequested", "draft": true } });
        assert_eq!(
            parse_choice(Some(&hand_edited), ListKind::Issues),
            Choice::default()
        );
    }

    #[test]
    fn saving_one_kinds_choice_keeps_the_others() {
        let first = with_choice(
            None,
            ListKind::Pulls,
            Choice {
                scope: Some(Scope::Created),
                draft: None,
            },
        );
        let both = with_choice(
            Some(&first),
            ListKind::Issues,
            Choice {
                scope: Some(Scope::Mentioned),
                draft: None,
            },
        );
        assert_eq!(
            parse_choice(Some(&both), ListKind::Pulls),
            Choice {
                scope: Some(Scope::Created),
                draft: None
            }
        );
        assert_eq!(
            parse_choice(Some(&both), ListKind::Issues),
            Choice {
                scope: Some(Scope::Mentioned),
                draft: None
            }
        );
        assert_eq!(
            with_choice(Some(&json!("junk")), ListKind::Pulls, Choice::default()),
            json!({ "pulls": { "scope": null, "draft": null } })
        );
    }

    #[test]
    fn a_draft_flag_that_isnt_a_boolean_is_dropped_the_scope_kept() {
        for draft in [json!("true"), json!(1)] {
            let saved = json!({ "pulls": { "scope": "created", "draft": draft } });
            assert_eq!(
                parse_choice(Some(&saved), ListKind::Pulls),
                Choice {
                    scope: Some(Scope::Created),
                    draft: None
                }
            );
        }
    }

    #[test]
    fn the_cache_key_tells_narrowings_apart_and_is_empty_for_none() {
        assert_eq!(Narrow::default().key(), "");
        assert_ne!(
            narrow(|n| n.scope = Some(Scope::Created)).key(),
            narrow(|n| n.scope = Some(Scope::Assigned)).key()
        );
        assert_ne!(
            narrow(|n| n.draft = Some(true)).key(),
            narrow(|n| n.draft = Some(false)).key()
        );
        assert_ne!(narrow(|n| n.draft = Some(false)).key(), "");
        assert_eq!(
            narrow(|n| n.labels = vec!["bug".into(), "ui".into()]).key(),
            narrow(|n| n.labels = vec!["ui".into(), "bug".into()]).key()
        );
        assert_ne!(
            narrow(|n| n.labels = vec!["a,b".into()]).key(),
            narrow(|n| n.labels = vec!["a".into(), "b".into()]).key()
        );
        assert_ne!(
            narrow(|n| n.author = Some("bug".into())).key(),
            narrow(|n| n.labels = vec!["bug".into()]).key()
        );
        assert_ne!(
            narrow(|n| n.author = Some("mona".into())).key(),
            narrow(|n| n.author = Some("Mona".into())).key()
        );
        assert_ne!(
            narrow(|n| n.author = Some("\"],null".into())).key(),
            narrow(|n| n.labels = vec!["\"],null".into()]).key()
        );
    }

    #[test]
    fn a_narrowing_is_anything_that_cuts_the_list() {
        assert!(!Narrow::default().is_narrowed());
        assert!(narrow(|n| n.draft = Some(false)).is_narrowed());
        assert!(narrow(|n| n.labels = vec!["bug".into()]).is_narrowed());
        assert!(narrow(|n| n.author = Some("mona".into())).is_narrowed());
    }

    #[test]
    fn an_empty_list_names_the_filter() {
        use ListKind::*;
        assert_eq!(
            empty_text(Pulls, Filter::Open, &Narrow::default()),
            "No open pull requests."
        );
        assert_eq!(
            empty_text(Pulls, Filter::All, &Narrow::default()),
            "No pull requests."
        );
        assert_eq!(
            empty_text(
                Pulls,
                Filter::Open,
                &narrow(|n| n.scope = Some(Scope::Assigned))
            ),
            "No open pull requests assigned to you."
        );
        let review = narrow(|n| {
            n.scope = Some(Scope::ReviewRequested);
            n.draft = Some(false);
        });
        assert_eq!(
            empty_text(Pulls, Filter::Closed, &review),
            "No closed non-draft pull requests waiting for your review."
        );
        let mentioned = narrow(|n| {
            n.scope = Some(Scope::Mentioned);
            n.labels = vec!["bug".into()];
        });
        assert_eq!(
            empty_text(Issues, Filter::Open, &mentioned),
            "No open issues that mention you with this label."
        );
        assert_eq!(
            empty_text(
                Issues,
                Filter::All,
                &narrow(|n| n.labels = vec!["bug".into(), "ui".into()])
            ),
            "No issues with all these labels."
        );
        assert_eq!(
            empty_text(Pulls, Filter::Open, &narrow(|n| n.draft = Some(true))),
            "No open draft pull requests."
        );
        assert_eq!(
            empty_text(
                Pulls,
                Filter::Open,
                &narrow(|n| n.author = Some("buildbot[bot]".into()))
            ),
            "No open pull requests by buildbot[bot]."
        );
        let all = narrow(|n| {
            n.author = Some("mona".into());
            n.scope = Some(Scope::Assigned);
            n.labels = vec!["bug".into()];
        });
        assert_eq!(
            empty_text(Issues, Filter::All, &all),
            "No issues by mona assigned to you with this label."
        );
    }

    #[test]
    fn core_reads_the_narrowing_as_the_page_sent_it() {
        let f = narrow(|n| {
            n.scope = Some(Scope::ReviewRequested);
            n.labels = vec!["bug".into()];
        })
        .to_core();
        assert!(f.scope.is_some() && f.labels == ["bug"] && f.author.is_none());
        assert!(Narrow::default().to_core().scope.is_none());
    }

    #[test]
    fn counts_shorten_as_githubs_do() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1_250), "1.2K");
        assert_eq!(compact_count(12_900), "12K");
        assert_eq!(compact_count(2_000_000), "2M");
    }
}
