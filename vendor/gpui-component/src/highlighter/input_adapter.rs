use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gpui::{HighlightStyle, SharedString, Task};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit as BaseInputEdit, InputHighlighter,
    InputHighlighterFactory, RopeExt as _,
};
use ropey::Rope;
use tree_sitter::{InputEdit, ParseOptions, Point};

use super::{LanguageRegistry, SyntaxHighlighter, highlighter::parse_input_bytes};

pub(crate) fn input_highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        LanguageRegistry::singleton().has_parser(language).then(|| {
            Box::new(TreeSitterInputHighlighter::new(language)) as Box<dyn InputHighlighter>
        })
    })
}

const SYNC_PARSE_TIMEOUT: Duration = Duration::from_millis(2);
const SYNC_PARSE_MAX_BYTES: usize = 256 * 1024;
const PARSE_DEBOUNCE: Duration = Duration::from_millis(150);

/// Styles are cached per this many lines: the editor asks for the visible lines every frame,
/// and a query over them each time made scrolling stutter.
const STYLE_CHUNK_LINES: usize = 32;
/// A chunk this large (a minified line) is not cached: the editor skips highlighting lines over
/// 10k bytes, and caching would query the whole chunk instead.
const STYLE_CHUNK_MAX_BYTES: usize = 32 * 1024;
/// Bounds the cache when scrolling through a long file without editing it.
const STYLE_CHUNKS_KEPT: usize = 128;

type Styles = Vec<(Range<usize>, HighlightStyle)>;

/// Styles by chunk of lines for the current tree and resolver. Any change to the tree clears it:
/// an edit can recolor text before it as well as after (a reparse reshapes enclosing nodes).
#[derive(Default)]
struct StyleCache {
    /// Address of the resolver the styles were resolved with; a theme change brings a new one.
    resolver: usize,
    chunks: HashMap<usize, Rc<Styles>>,
}

struct TreeSitterInputHighlighter {
    inner: Rc<RefCell<SyntaxHighlighter>>,
    style_cache: Rc<RefCell<StyleCache>>,
    parse_task: Rc<RefCell<Option<Task<()>>>>,
}

impl TreeSitterInputHighlighter {
    fn new(language: &str) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SyntaxHighlighter::new(language))),
            style_cache: Rc::default(),
            parse_task: Rc::new(RefCell::new(None)),
        }
    }

    /// Bring the tree up to `text` with `edits` (none: the change is unknown), parsing
    /// synchronously within a small budget. Returns whether the parse completed.
    fn apply_edits(&self, edits: &[InputEdit], text: &Rope) -> bool {
        self.style_cache.borrow_mut().chunks.clear();
        let mut highlighter = self.inner.borrow_mut();
        if text.len() > SYNC_PARSE_MAX_BYTES {
            if edits.is_empty() {
                highlighter.edit_tree(None, text);
            }
            for edit in edits {
                highlighter.edit_tree(Some(*edit), text);
            }
            false
        } else {
            highlighter.update_edits(edits, text, Some(SYNC_PARSE_TIMEOUT))
        }
    }

    fn cached_styles(&self, range: &Range<usize>, resolver: &dyn HighlightStyleResolver) -> Styles {
        let inner = self.inner.borrow();
        let text = inner.text();
        if range.is_empty() || range.end > text.len() {
            return inner.styles(range, resolver);
        }

        let mut cache = self.style_cache.borrow_mut();
        let resolver_key = resolver as *const dyn HighlightStyleResolver as *const () as usize;
        if cache.resolver != resolver_key {
            cache.resolver = resolver_key;
            cache.chunks.clear();
        }

        let first = text.offset_to_point(range.start).row / STYLE_CHUNK_LINES;
        let last = text.offset_to_point(range.end - 1).row / STYLE_CHUNK_LINES;
        let mut out = Styles::new();
        for chunk in first..=last {
            let chunk_range = text.line_start_offset(chunk * STYLE_CHUNK_LINES)
                ..text.line_start_offset((chunk + 1) * STYLE_CHUNK_LINES);
            let wanted = chunk_range.start.max(range.start)..chunk_range.end.min(range.end);
            if chunk_range.len() > STYLE_CHUNK_MAX_BYTES {
                out.extend(inner.styles(&wanted, resolver));
                continue;
            }
            let styles = match cache.chunks.get(&chunk) {
                Some(styles) => styles.clone(),
                None => {
                    if cache.chunks.len() >= STYLE_CHUNKS_KEPT {
                        cache.chunks.clear();
                    }
                    let styles = Rc::new(inner.styles(&chunk_range, resolver));
                    cache.chunks.insert(chunk, styles.clone());
                    styles
                }
            };
            // Runs are sorted and cover the chunk: keep the ones inside `wanted`, clipped.
            let from = styles.partition_point(|(run, _)| run.end <= wanted.start);
            for (run, style) in &styles[from..] {
                if run.start >= wanted.end {
                    break;
                }
                out.push((run.start.max(wanted.start)..run.end.min(wanted.end), *style));
            }
        }
        out
    }

    /// Settle after the edits reached the tree: drop a pending background
    /// parse when the synchronous one `completed`, or schedule one for `text`.
    fn finish_update(
        &mut self,
        completed: bool,
        text: &Rope,
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        if completed {
            self.parse_task.borrow_mut().take();
            return;
        }

        let highlighter = self.inner.clone();
        let style_cache = self.style_cache.clone();
        let parse_task = self.parse_task.clone();
        let language = highlighter.borrow().language().clone();
        let old_tree = highlighter.borrow().tree().cloned();
        let injection_data = highlighter.borrow().injection_parse_data();
        let text = text.clone();
        let text_for_apply = text.clone();
        let cancel = Arc::new(AtomicBool::new(false));

        let task = cx.spawn_in(window, async move |entity, cx| {
            struct CancelOnDrop(Arc<AtomicBool>);
            impl Drop for CancelOnDrop {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _cancel_guard = CancelOnDrop(cancel.clone());
            cx.background_executor().timer(PARSE_DEBOUNCE).await;

            let parse_cancel = cancel.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (mut parser, grammar) =
                        LanguageRegistry::singleton().parser(&language).ok()?;
                    parser.set_language(&grammar).ok()?;
                    let mut progress = |_: &tree_sitter::ParseState| {
                        if parse_cancel.load(Ordering::Relaxed) {
                            std::ops::ControlFlow::Break(())
                        } else {
                            std::ops::ControlFlow::Continue(())
                        }
                    };
                    let options = ParseOptions::new().progress_callback(&mut progress);
                    let tree = parser.parse_with_options(
                        &mut |offset, _| parse_input_bytes(&text, offset),
                        old_tree.as_ref(),
                        Some(options),
                    )?;
                    if parse_cancel.load(Ordering::Relaxed) {
                        return None;
                    }
                    let injections = injection_data.map_or_else(Default::default, |data| {
                        SyntaxHighlighter::compute_injection_layers(data, &tree, &text)
                    });
                    let folds = if folding {
                        extract_fold_ranges(&tree)
                    } else {
                        Vec::new()
                    };
                    Some((tree, injections, folds))
                })
                .await;

            if let Some((tree, injections, folds)) = result {
                highlighter
                    .borrow_mut()
                    .apply_background_tree(tree, &text_for_apply, injections);
                style_cache.borrow_mut().chunks.clear();
                let _ = entity.update(cx, |state, cx| {
                    state.apply_highlighter_fold_candidates(folds, cx);
                });
            }
        });
        parse_task.borrow_mut().replace(task);
    }
}

impl SyntaxHighlighter {
    pub(crate) fn update_input(
        &mut self,
        edit: Option<BaseInputEdit>,
        text: &Rope,
        timeout: Option<Duration>,
    ) -> bool {
        self.update(edit.map(to_tree_sitter_edit), text, timeout)
    }
}

impl InputHighlighter for TreeSitterInputHighlighter {
    fn language(&self) -> SharedString {
        self.inner.borrow().language().clone()
    }

    fn update(
        &mut self,
        edit: Option<BaseInputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        let edit = edit.map(to_tree_sitter_edit);
        let completed = self.apply_edits(edit.as_slice(), text);
        self.finish_update(completed, text, folding, window, cx);
    }

    fn update_batch(
        &mut self,
        edits: &[(BaseInputEdit, Rope)],
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        let Some((_, text)) = edits.last() else {
            return;
        };
        let edits: Vec<InputEdit> = edits
            .iter()
            .map(|(edit, _)| to_tree_sitter_edit(*edit))
            .collect();
        let completed = self.apply_edits(&edits, text);
        self.finish_update(completed, text, folding, window, cx);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.cached_styles(range, resolver)
    }
    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(extract_fold_ranges)
            .unwrap_or_default()
    }

    fn fold_ranges_for_edit(&self, range: Range<usize>, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(|tree| extract_fold_ranges_in_range(tree, range))
            .unwrap_or_default()
    }
}

fn to_tree_sitter_edit(edit: BaseInputEdit) -> InputEdit {
    InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: Point::new(edit.start_position.row, edit.start_position.column),
        old_end_position: Point::new(edit.old_end_position.row, edit.old_end_position.column),
        new_end_position: Point::new(edit.new_end_position.row, edit.new_end_position.column),
    }
}

fn extract_fold_ranges(tree: &tree_sitter::Tree) -> Vec<FoldRange> {
    extract_fold_ranges_in_range(tree, 0..usize::MAX)
}

fn extract_fold_ranges_in_range(
    tree: &tree_sitter::Tree,
    byte_range: Range<usize>,
) -> Vec<FoldRange> {
    fn collect(node: tree_sitter::Node, bytes: &Range<usize>, ranges: &mut Vec<FoldRange>) {
        if node.end_byte() <= bytes.start || node.start_byte() >= bytes.end {
            return;
        }
        let start = node.start_position().row;
        let end = node.end_position().row;
        if end.saturating_sub(start) < 2 {
            return;
        }
        ranges.push(FoldRange::new(start, end));
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect(child, bytes, ranges);
        }
    }

    let root = tree.root_node();
    let mut ranges = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        collect(child, &byte_range, &mut ranges);
    }
    ranges.sort_by_key(|range| range.start_line);
    ranges.dedup_by_key(|range| range.start_line);
    ranges
}

#[cfg(all(test, feature = "tree-sitter-languages"))]
mod tests {
    use super::*;
    use crate::highlighter::HighlightTheme;

    /// The style of every byte: runs split at chunk boundaries where a fresh query has one run.
    fn per_byte(styles: &Styles) -> Vec<HighlightStyle> {
        styles
            .iter()
            .flat_map(|(run, style)| run.clone().map(|_| *style))
            .collect()
    }

    fn color_at(styles: &Styles, offset: usize) -> Option<gpui::Hsla> {
        styles
            .iter()
            .find(|(run, _)| run.contains(&offset))
            .and_then(|(_, style)| style.color)
    }

    #[test]
    fn test_cached_styles_match_uncached_and_follow_edits() {
        let theme = HighlightTheme::default_dark();
        // The closer sits in a line comment until a block comment is opened above it.
        let mut source: String = (0..80)
            .map(|i| format!("fn f{i}() {{ let x = {i}; }}\n"))
            .collect();
        source.push_str("// */\n");
        let text = Rope::from(source.as_str());
        let highlighter = TreeSitterInputHighlighter::new("rust");
        assert!(highlighter.apply_edits(&[], &text));

        // Across a chunk boundary and twice (the second from the cache), as a fresh query.
        let range = text.line_start_offset(20)..text.line_start_offset(70);
        let fresh = per_byte(&highlighter.inner.borrow().styles(&range, theme.as_ref()));
        assert_eq!(fresh.len(), range.len());
        assert_eq!(
            per_byte(&highlighter.cached_styles(&range, theme.as_ref())),
            fresh
        );
        assert_eq!(
            per_byte(&highlighter.cached_styles(&range, theme.as_ref())),
            fresh
        );

        // A block comment opened above the cached lines colors them as a comment.
        let keyword = text.line_start_offset(60);
        let before = color_at(&highlighter.cached_styles(&range, theme.as_ref()), keyword);
        let comment = theme.style("comment").and_then(|style| style.color);
        assert!(comment.is_some());
        assert_ne!(before, comment);

        let new_text = Rope::from(format!("/*\n{source}").as_str());
        let edit = InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 3,
            start_position: Point::new(0, 0),
            old_end_position: Point::new(0, 0),
            new_end_position: Point::new(1, 0),
        };
        assert!(highlighter.apply_edits(&[edit], &new_text));
        let shifted = new_text.line_start_offset(20)..new_text.line_start_offset(70);
        let after = highlighter.cached_styles(&shifted, theme.as_ref());
        assert_eq!(color_at(&after, keyword + 3), comment);
        assert_eq!(
            per_byte(&after),
            per_byte(&highlighter.inner.borrow().styles(&shifted, theme.as_ref()))
        );
    }
}
