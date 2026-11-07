//! Mod tree expansion, scoped truncation, Unicode editing and mouse row mapping.

use super::super::rows;
use super::*;
use crate::commands::supports::mods::source_path::PathArgs;
use clap::Parser;
use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use mcpm_backend::operations::mods::loading::scan::ScannedFile;
use mcpm_lib::local::runtime::RuntimeEnvironment;
use mcpm_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry},
    modloader::BUILTIN_LOADERS,
};
use ratatui::{Terminal, backend::TestBackend};
use std::{path::Path, sync::Arc};

fn fixture(expanded: bool) -> Browser {
    #[derive(Parser, Debug)]
    struct Paths {
        #[command(flatten)]
        paths: PathArgs,
    }
    let paths = Paths::parse_from(["test"]).paths;
    let file = mcmod::open(
        ModEntry::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"))
            .unwrap(),
        BUILTIN_LOADERS.clone(),
        &RuntimeEnvironment::new().with_mods_cache(None),
        |_, _, _, _| IdentificationErrorAction::Abort,
    )
    .unwrap()
    .file;
    let files = [ListedFile {
        scanned: Arc::new(ScannedFile {
            file,
            failures: Vec::new(),
        }),
        records: None,
    }];
    Browser::new(
        tree::build(
            &files,
            &RowOptions {
                expanded,
                paths: &paths,
                index_type: rows::IndexType::Flat,
            },
        ),
        Filter::default(),
    )
}
fn summary() -> Summary<'static> {
    static EXECUTOR: std::sync::LazyLock<mcpm_exec::service::Executor> =
        std::sync::LazyLock::new(|| {
            Runtime::new()
                .unwrap()
                .block_on(mcpm_exec::service::Executor::open(
                    mcpm_exec::service::Target::Local,
                ))
                .unwrap()
        });
    Summary {
        location: Location::new(&EXECUTOR, None),
        ignored: 0,
        ignored_files: &[],
        input_errors: 0,
        failed: 0,
        cache: None,
    }
}

#[test]
fn expansion_is_progressive_and_search_finds_hidden_descendants() {
    let mut browser = fixture(false);
    assert_eq!(browser.matches.len(), 1);
    assert!(!browser.nodes[0].children.is_empty());
    assert!(browser.expand(true));
    assert_eq!(browser.matches.len(), browser.nodes[0].children.len() + 1);
    let parent = browser
        .matches
        .iter()
        .position(|&index| index != 0 && !browser.nodes[index].children.is_empty())
        .unwrap();
    browser.select(parent);
    let node = browser.selected_index().unwrap();
    assert!(browser.expand(true));
    assert!(
        browser.nodes[node]
            .children
            .iter()
            .all(|child| browser.matches.contains(child))
    );
    assert!(browser.expand(false));
    assert!(
        browser.nodes[node]
            .children
            .iter()
            .all(|child| !browser.matches.contains(child))
    );
    browser.query = "omnibus_leaf".into();
    browser.filter();
    assert!(browser.matches.iter().any(|&index| {
        filter::Filter::parse("@modid omnibus_leaf").matches(&browser.nodes[index].row)
    }));
    for &index in &browser.matches {
        if let Some(parent) = browser.nodes[index].parent {
            assert!(browser.matches.contains(&parent));
        }
    }
    browser.query.clear();
    browser.expand_all();
    assert_eq!(browser.matches.len(), browser.nodes.len());
}

#[test]
fn scoped_hits_survive_truncation_without_changing_copy_values() {
    let mut browser = fixture(false);
    let name = "AABBCC plain-name";
    let filename = format!("Create-{}-XXXXAABBCC-suffix.jar", "long-prefix-".repeat(12));
    browser.nodes[0].row.cells[Column::Name.position()] =
        vec![rows::Piece::new(name, rows::Paint::Plain)];
    browser.nodes[0].row.cells[Column::Filename.position()] =
        vec![rows::Piece::new(&filename, rows::Paint::Plain)];
    browser.query = "@file AABBCC".into();
    browser.filter();
    assert_eq!(browser.matches, vec![0]);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row_text = |y| {
        (0..100)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let name_row = browser.table_area.y + 3;
    let file_row = name_row + 1;
    let details_file = (browser.detail_area.y..browser.detail_area.bottom())
        .find(|&y| row_text(y).contains("File: "))
        .unwrap();
    for y in [file_row, details_file] {
        assert!(
            row_text(y).contains("AABBCC"),
            "the complete hit must survive truncation"
        );
    }
    assert_eq!(
        browser
            .detail_value(DetailField::Column(Column::Filename))
            .unwrap(),
        filename
    );
}

#[test]
fn filter_editing_treats_shortcuts_as_text_and_preserves_unicode_boundaries() {
    use crossterm::event::{KeyCode, KeyEvent};
    let mut browser = fixture(false);
    let press = |browser: &mut Browser, code| {
        input::handle(
            browser,
            &summary(),
            Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            10,
        )
    };
    press(&mut browser, KeyCode::Char('f'));
    for ch in "@name 中文q".chars() {
        press(&mut browser, KeyCode::Char(ch));
    }
    assert!(browser.focus == Focus::Filter);
    assert_eq!(browser.query, "@name 中文q");
    for code in [
        KeyCode::Left,
        KeyCode::Backspace,
        KeyCode::Backspace,
        KeyCode::Delete,
    ] {
        press(&mut browser, code);
        assert!(browser.query.is_char_boundary(browser.query_cursor));
    }
    assert_eq!(browser.query, "@name ");
}

#[test]
fn search_branches_can_fold_and_keep_normal_expansion_separate() {
    let mut browser = fixture(false);
    browser.query = "@modid omnibus_leaf".into();
    browser.filter();
    let all_matches = browser.matches.clone();
    assert!(all_matches.len() > 1);
    browser.select(0);
    assert!(browser.expand(false));
    assert_eq!(browser.matches.len(), 1);
    assert!(!browser.is_expanded(0));
    browser.filter();
    assert_eq!(browser.matches.len(), 1);
    browser.query.push(' ');
    browser.filter();
    assert_eq!(browser.matches.len(), 1);
    assert!(browser.toggle());
    assert_eq!(browser.matches, all_matches);
    let position = browser
        .matches
        .iter()
        .position(|&index| index != 0 && !browser.nodes[index].children.is_empty())
        .unwrap();
    browser.select(position);
    let parent = browser.selected_index().unwrap();
    assert!(browser.toggle());
    assert!(!browser.is_expanded(parent));
    browser.expand_all();
    assert_eq!(browser.matches, all_matches);
    browser.query.clear();
    browser.filter();
    assert_eq!(browser.matches.len(), 1);
    assert!(!browser.nodes[0].expanded);
}

/// Give every row an independent name so matching a parent cannot match its children.
fn name_search_rows(browser: &mut Browser) {
    use super::super::rows::{Paint, Piece};
    for (index, node) in browser.nodes.iter_mut().enumerate() {
        node.row.cells[Column::Name.position()] =
            vec![Piece::new(format!("node-{index};"), Paint::Plain)];
    }
}

#[test]
fn search_opens_only_paths_to_matches_and_restores_each_branch_immediately() {
    let mut browser = fixture(false);
    name_search_rows(&mut browser);
    let parent = browser.nodes[0]
        .children
        .iter()
        .copied()
        .find(|&index| !browser.nodes[index].children.is_empty())
        .unwrap();
    let child = browser.nodes[parent].children[0];
    // Preserve a mix of open and closed states, including hidden descendants.
    browser.nodes[child].expanded = true;
    let original: Vec<_> = browser.nodes.iter().map(|node| node.expanded).collect();
    browser.query = format!("@name node-{parent};");
    browser.filter();
    assert_eq!(browser.matches, vec![0, parent]);
    assert!(browser.is_expanded(0));
    assert!(
        !browser.is_expanded(parent),
        "a matching parent stays folded"
    );

    browser.query = format!("@name node-{child};");
    browser.filter();
    assert!(browser.matches.contains(&child));
    assert!(browser.is_expanded(parent));

    browser.query = format!("@name node-{parent};");
    browser.filter();
    assert!(
        !browser.is_expanded(parent),
        "restore before clearing search"
    );
    browser.query = "@name no-such-row".into();
    browser.filter();
    assert!(browser.matches.is_empty());
    for (index, &open) in original.iter().enumerate() {
        assert_eq!(browser.is_expanded(index), open);
        assert_eq!(browser.nodes[index].expanded, open);
    }
    browser.query.clear();
    browser.filter();
    assert_eq!(browser.matches, vec![0]);
    assert!(browser.is_expanded(child));
}

#[test]
fn search_manual_overrides_expire_only_when_the_branch_leaves_results() {
    use super::super::rows::{Paint, Piece};
    let mut browser = fixture(false);
    name_search_rows(&mut browser);
    let parents: Vec<_> = browser.nodes[0]
        .children
        .iter()
        .copied()
        .filter(|&index| !browser.nodes[index].children.is_empty())
        .take(2)
        .collect();
    assert_eq!(parents.len(), 2);
    let (first, second) = (parents[0], parents[1]);
    for (parent, name) in [(first, "target-alpha"), (second, "target-beta")] {
        let child = browser.nodes[parent].children[0];
        browser.nodes[child].row.cells[Column::Name.position()] =
            vec![Piece::new(name, Paint::Plain)];
    }
    browser.nodes[second].expanded = true;
    browser.query = "@name target-alpha".into();
    browser.filter();
    browser.select(
        browser
            .matches
            .iter()
            .position(|&index| index == first)
            .unwrap(),
    );
    assert!(browser.expand(false));
    browser.query = "@name target-".into();
    browser.filter();
    assert!(
        !browser.is_expanded(first),
        "keep override while still matching"
    );
    assert!(browser.is_expanded(second));
    browser.expand_all();
    assert!(browser.is_expanded(first));
    assert!(browser.expand(false));
    browser.query = "@name target-beta".into();
    browser.filter();
    assert!(!browser.search_overrides.contains_key(&first));
    assert!(!browser.is_expanded(first));
    browser.select(
        browser
            .matches
            .iter()
            .position(|&index| index == second)
            .unwrap(),
    );
    assert!(browser.expand(false));
    browser.query = "@name target-alpha".into();
    browser.filter();
    assert!(
        browser.is_expanded(first),
        "returning match gets a fresh auto expansion"
    );
    assert!(
        browser.is_expanded(second),
        "restore original open state off-path"
    );
    assert!(!browser.search_overrides.contains_key(&second));
    browser.query.clear();
    browser.filter();
    assert!(!browser.is_expanded(first));
    assert!(browser.is_expanded(second));
    assert!(browser.search_overrides.is_empty());
}

#[test]
fn two_line_items_map_both_rendered_rows_to_the_same_selection() {
    use crossterm::event::MouseButton;
    for columns in [
        Column::ALL.to_vec(),
        vec![Column::Filename],
        vec![Column::Name, Column::Filename],
        vec![Column::Modid, Column::Filename],
    ] {
        let mut browser = fixture(true);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        let mut page = 0;
        terminal
            .draw(|frame| {
                page = view::draw(frame, &mut browser, &columns, &summary());
            })
            .unwrap();
        let area = browser.table_area;
        let first = area.y + 3;
        assert_eq!(page, (area.height.saturating_sub(4) / 2).max(1) as usize);
        for (row, expected) in [(first, 0), (first + 1, 0), (first + 2, 1), (first + 3, 1)] {
            input::handle(
                &mut browser,
                &summary(),
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: area.x + 2,
                    row,
                    modifiers: KeyModifiers::NONE,
                }),
                page,
            );
            assert_eq!(browser.table.selected(), Some(expected));
        }
    }
}

#[test]
fn identification_counts_unique_files_and_preserves_partial_cache_facts() {
    use mcpm_backend::operations::mods::identification::{SourceResolution, SourceResult};
    use mcpm_lib::source::{
        FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef, SourceId,
    };

    let matched = |source: &str| FileMatch {
        reference: FileRef {
            project: ProjectRef {
                source: SourceId(source.into()),
                id: ProjectId("project".into()),
            },
            id: FileId("file".into()),
        },
        filename: "release.jar".into(),
        size: 1,
        hashes: vec![],
        role: MatchedFileRole::Primary,
    };
    let source = |platform: identify::Platform, resolution| SourceResult {
        source: SourceId(platform.id().into()),
        cache_id: Some(SourceId(platform.id().into())),
        resolution,
    };
    let failure = || SourceResolution::Failed(Arc::new(anyhow::anyhow!("unavailable").into()));
    let partial = Arc::new(QueryResult {
        sources: vec![
            source(
                identify::Platform::Modrinth,
                SourceResolution::Cached(matched("modrinth")),
            ),
            source(identify::Platform::CurseForge, failure()),
        ],
    });
    let mut browser = fixture(true);
    browser.identification = vec![[identify::State::Queued, identify::State::Queued]; 3];
    browser.identification_owners = vec![0, 1, 2];
    browser.identification_total = 3;
    browser.apply_identification(identify::Update::Item {
        files: vec![0, 1],
        result: Ok(partial.clone()),
    });
    let summary = browser.identification_summary();
    assert_eq!(
        (summary.total, summary.partial_failed, summary.failed),
        (1, 1, 0)
    );
    assert_eq!(browser.identification_progress(), Some((1, 2)));
    for file in [0, 1] {
        assert!(matches!(
            browser.identification[file][0],
            identify::State::Matched { cached: true, .. }
        ));
    }

    // Repeated terminal/subset events, loading, nested rows and filtering cannot recount the file.
    browser.apply_identification(identify::Update::Item {
        files: vec![1],
        result: Ok(partial),
    });
    browser.apply_identification(identify::Update::Loading { files: vec![0, 1] });
    browser.query = "no-matching-mod".into();
    browser.filter();
    assert_eq!(browser.identification_summary().total, 1);
    assert!(matches!(
        browser.identification[0][0],
        identify::State::Matched { cached: true, .. }
    ));
    let cache = browser.identification_cache_summary();
    assert_eq!(
        (cache.total, cache.fully_cached, cache.partially_cached),
        (1, 0, 1)
    );

    browser.apply_identification(identify::Update::Item {
        files: vec![2],
        result: Ok(Arc::new(QueryResult {
            sources: vec![
                source(
                    identify::Platform::Modrinth,
                    SourceResolution::Queried(Some(matched("modrinth"))),
                ),
                source(identify::Platform::CurseForge, failure()),
            ],
        })),
    });
    let summary = browser.identification_summary();
    assert_eq!(
        (
            summary.total,
            summary.success,
            summary.partial_failed,
            summary.failed
        ),
        (2, 0, 1, 1)
    );
    assert_eq!(
        summary.completed(),
        summary.success + summary.partial_failed + summary.failed
    );
    assert_eq!(browser.identification_progress(), None);
    assert!(matches!(
        browser.identification[2][0],
        identify::State::Matched { cached: false, .. }
    ));
    assert_eq!(browser.identification_cache_summary().total, 2);
}

#[test]
fn preparation_and_stopped_lifecycle_do_not_become_query_failures() {
    let failure = || {
        let (error, remote) = crate::diagnostics::capture(&anyhow::anyhow!("cannot prepare"));
        identify::Failure { error, remote }
    };
    let mut browser = fixture(false);
    browser.apply_identification(identify::Update::Item {
        files: vec![0],
        result: Err(failure()),
    });
    let summary = browser.identification_summary();
    assert_eq!(
        (
            summary.preparation_failed,
            summary.failed,
            summary.partial_failed
        ),
        (1, 0, 0)
    );
    assert_eq!(summary.completed(), 0);
    assert_eq!(browser.identification_cache_summary().total, 0);

    let mut browser = fixture(false);
    browser.apply_identification(identify::Update::Stopped {
        files: vec![0],
        failure: failure(),
    });
    let summary = browser.identification_summary();
    assert_eq!(summary.total, 0);
    assert!(!summary.has_failures());
    assert_eq!(browser.identification_progress(), None);
}
