//! Regression coverage for real mod trees, scroll reversals and compact card geometry.

use super::super::rows;
use super::*;
use crate::commands::mod_support::{scan::ScannedFile, source_path::PathArgs};
use clap::Parser;
use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry},
    modloader::BUILTIN_LOADERS,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
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
        |_, _, _, _| IdentificationErrorAction::Abort,
    )
    .unwrap();
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
    Summary {
        location: Location::Local,
        ignored: 0,
        ignored_files: &[],
        input_errors: 0,
        failed: 0,
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
fn wheel_reversal_responds_immediately_after_repeated_boundary_events() {
    let mut browser = fixture(true);
    browser.table_area = Rect::new(0, 0, 80, 20);
    let wheel = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })
    };
    for _ in 0..1000 {
        input::handle(
            &mut browser,
            &summary(),
            wheel(MouseEventKind::ScrollDown),
            10,
        );
    }
    assert_eq!(browser.table.selected(), Some(browser.matches.len() - 1));
    assert!(matches!(
        input::handle(
            &mut browser,
            &summary(),
            wheel(MouseEventKind::ScrollDown),
            10
        ),
        input::Update::Ignore
    ));
    input::handle(
        &mut browser,
        &summary(),
        wheel(MouseEventKind::ScrollUp),
        10,
    );
    assert_eq!(browser.table.selected(), Some(browser.matches.len() - 2));
    for _ in 0..1000 {
        input::handle(
            &mut browser,
            &summary(),
            wheel(MouseEventKind::ScrollUp),
            10,
        );
    }
    input::handle(
        &mut browser,
        &summary(),
        wheel(MouseEventKind::ScrollDown),
        10,
    );
    assert_eq!(browser.table.selected(), Some(1));
    browser.ignored = true;
    browser.scroll_limit = 9;
    for _ in 0..1000 {
        browser.scroll_by(1);
    }
    assert_eq!(browser.scroll, 9);
    browser.scroll_by(-1);
    assert_eq!(browser.scroll, 8);
    for _ in 0..1000 {
        browser.scroll_by(-1);
    }
    browser.scroll_by(1);
    assert_eq!(browser.scroll, 1);
}

#[test]
fn compact_details_stay_below_table_and_selection_uses_background() {
    let mut browser = fixture(false);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    assert_eq!(browser.detail_area.height, 10);
    assert!(browser.detail_area.y >= browser.table_area.bottom());
    let buffer = terminal.backend().buffer();
    let row_y = browser.table_area.y + 3;
    let mut saw_background = false;
    for x in browser.table_area.x + 1..browser.table_area.right() - 1 {
        saw_background |= buffer[(x, row_y)].bg != ratatui::style::Color::Reset;
    }
    assert!(saw_background || std::env::var_os("NO_COLOR").is_some());
    browser.focus = Focus::Details;
    browser.move_by(1);
    assert_eq!(browser.field, 1);
    let selected = browser.table.selected();
    browser.move_by(1000);
    assert_eq!(browser.field, 5);
    browser.move_by(-1);
    assert_eq!(browser.field, 4);
    assert_eq!(browser.table.selected(), selected);
    for (width, height) in [(1, 1), (2, 8), (10, 10), (30, 24), (60, 40), (120, 24)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                view::draw(frame, &mut browser, &Column::ALL, &summary());
            })
            .unwrap();
    }
}

#[test]
fn scoped_filters_exclude_indices_and_scope_only_the_next_matcher() {
    use super::super::rows::{Paint, Piece};
    let mut browser = fixture(false);
    let row = &mut browser.nodes[0].row;
    for (column, value) in [
        (Column::Index, "only-in-index"),
        (Column::Name, "Fabric API 中文"),
        (Column::Modid, "fabric_api"),
        (Column::Filename, "mods/fabric-api.jar"),
        (Column::Loaders, "[Fabric]"),
    ] {
        row.cells[column.position()] = vec![Piece::new(value, Paint::Plain)];
    }
    for query in [
        "Fabric API",
        "fabric_api",
        "mods/fabric-api.jar",
        "[Fabric]",
        "@name Fabric API 中文 @modid fabric_api @file api.jar @loaders Fabric",
        "API @name Fabric API @file fabric",
    ] {
        assert!(filter::Filter::parse(query).matches(row), "{query}");
    }
    for query in [
        "only-in-index",
        "@index only-in-index",
        "@ name Fabric",
        "@name fabric_api",
        "@file \"Fabric API\"",
        "@loaders Forge",
        "@name Fabric API @file unrelated",
    ] {
        assert!(!filter::Filter::parse(query).matches(row), "{query}");
    }
    assert!(!filter::Filter::parse("@name Fabric API 中文 @file fabric").invalid());
    assert!(filter::Filter::parse("@ name Fabric").invalid());
    assert!(filter::Filter::parse("@index").invalid());
}

#[test]
fn scoped_hits_remain_visible_and_styled_in_file_rows_and_details_without_changing_copy_values() {
    use crate::theme::{Role, widgets};
    use ratatui::style::Modifier;
    use unicode_width::UnicodeWidthStr;
    let mut browser = fixture(false);
    let name = "AABBCC plain-name";
    let filename = format!("Create-{}-XXXXAABBCC-suffix.jar", "long-prefix-".repeat(12));
    browser.nodes[0].row.cells[Column::Name.position()] =
        vec![rows::Piece::new(name, rows::Paint::Plain)];
    browser.nodes[0].row.cells[Column::Filename.position()] =
        vec![rows::Piece::new(&filename, rows::Paint::Role(Role::Muted))];
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
    for y in [name_row, file_row, details_file] {
        let text = row_text(y);
        let start = text
            .find("AABBCC")
            .expect("the complete hit must survive truncation");
        let x = text[..start].width() as u16;
        let cell = &buffer[(x, y)];
        if y == name_row {
            assert!(
                !cell.modifier.contains(Modifier::BOLD),
                "@file must not highlight Name"
            );
        } else {
            assert_eq!(
                cell.fg,
                widgets::style(Role::Secondary).fg.unwrap_or_default()
            );
            assert!(cell.modifier.contains(Modifier::BOLD));
            assert!(!cell.modifier.contains(Modifier::DIM));
            assert!(text.contains("..."));
        }
    }
    assert_eq!(
        browser
            .detail_value(DetailField::Column(Column::Filename))
            .unwrap(),
        filename
    );
}

#[test]
fn filter_card_keys_keep_text_and_highlight_scopes_and_values_separately() {
    use crate::theme::{Role, widgets};
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
    press(&mut browser, KeyCode::Char('F'));
    assert!(browser.filter_open && browser.focus == Focus::Filter);
    for ch in "@name Fabric API @file complex".chars() {
        press(&mut browser, KeyCode::Char(ch));
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    assert!(browser.filter_area.y >= browser.table_area.bottom());
    assert!(browser.filter_area.bottom() <= browser.detail_area.y);
    let buffer = terminal.backend().buffer();
    let (x, y) = (browser.filter_area.x + 2, browser.filter_area.y + 1);
    assert_eq!(buffer[(x, y)].symbol(), "@");
    assert_eq!(
        buffer[(x, y)].fg,
        widgets::style(Role::Primary).fg.unwrap_or_default()
    );
    assert_eq!(
        buffer[(x + 6, y)].fg,
        widgets::style(Role::Secondary).fg.unwrap_or_default()
    );
    press(&mut browser, KeyCode::Tab);
    assert!(browser.focus == Focus::Details);
    press(&mut browser, KeyCode::BackTab);
    assert!(browser.focus == Focus::Filter);
    press(&mut browser, KeyCode::Esc);
    assert!(!browser.filter_open && browser.focus == Focus::Mods);
    assert_eq!(browser.query, "@name Fabric API @file complex");
    press(&mut browser, KeyCode::Char('f'));
    press(&mut browser, KeyCode::Char('q'));
    assert!(browser.query.ends_with("complexq"));
    press(&mut browser, KeyCode::Char('Q'));
    assert!(!browser.filter_open && browser.focus == Focus::Mods);
    press(&mut browser, KeyCode::Char('r'));
    assert!(browser.query.is_empty());
    press(&mut browser, KeyCode::Char('f'));
    for ch in "@name 中文".chars() {
        press(&mut browser, KeyCode::Char(ch));
    }
    press(&mut browser, KeyCode::Left);
    press(&mut browser, KeyCode::Delete);
    press(&mut browser, KeyCode::Backspace);
    assert_eq!(browser.query, "@name ");
    for (width, height) in [(1, 1), (10, 10), (30, 24), (60, 30)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                view::draw(frame, &mut browser, &Column::ALL, &summary());
            })
            .unwrap();
    }
}

#[test]
fn initial_filter_supports_scoped_groups_and_editor_highlights_tokens_and_errors() {
    use crate::theme::{Role, widgets};
    use ratatui::style::Modifier;
    let original = fixture(false);
    let source = "@loaders (=Forge | =Fabric) & @file ~'complex.*jar$'";
    let mut browser = Browser::new(original.nodes, source.parse().unwrap());
    assert!(browser.filter_open);
    assert_eq!(browser.query, source);
    assert_eq!(browser.query_cursor, source.len());
    assert!(browser.focus == Focus::Mods);
    assert!(!browser.parsed.invalid());
    assert!(
        browser
            .matches
            .iter()
            .any(|&index| { browser.nodes[index].row.searchable(Column::Loaders).0 == "[Fabric]" })
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    let (x, y) = (browser.filter_area.x + 2, browser.filter_area.y + 1);
    let buffer = terminal.backend().buffer();
    for (token, role) in [
        ("@loaders", Role::Primary),
        ("(", Role::Muted),
        ("=", Role::Important),
        ("Forge", Role::Secondary),
        ("|", Role::Important),
        ("&", Role::Important),
        ("~", Role::Important),
        ("'", Role::Muted),
        ("complex", Role::Secondary),
    ] {
        let offset = source.find(token).unwrap() as u16;
        assert_eq!(
            buffer[(x + offset, y)].fg,
            widgets::style(role).fg.unwrap_or_default(),
            "{token}"
        );
    }
    browser.query = "@loaders (Forge | Fabric) & @unknown value".into();
    browser.filter();
    assert!(browser.matches.is_empty());
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let offset = browser.query.find("@unknown").unwrap() as u16;
    assert_eq!(
        buffer[(x + offset, y)].fg,
        widgets::style(Role::Danger).fg.unwrap_or_default()
    );
    assert!(
        buffer[(x + offset, y)]
            .modifier
            .contains(Modifier::UNDERLINED)
    );
    let title: String = (0..120)
        .map(|x| buffer[(x, browser.filter_area.y)].symbol())
        .collect();
    assert!(
        title.contains("unknown field") && title.contains("column 29"),
        "{title}"
    );
    browser.query.clear();
    browser.filter();
    assert_eq!(
        browser.matches.len(),
        1,
        "initial filtering must not change normal expansion"
    );
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
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(browser.table_area.x + 1, browser.table_area.y + 3)].symbol(),
        ">"
    );
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
fn escaped_at_is_literal_in_plain_and_scoped_values() {
    use super::super::rows::{Paint, Piece};
    let mut browser = fixture(false);
    let row = &mut browser.nodes[0].row;
    row.cells[Column::Name.position()] = vec![Piece::new("Contact @name 中文", Paint::Plain)];
    row.cells[Column::Filename.position()] =
        vec![Piece::new(r"C:\mods\user@host.jar", Paint::Plain)];
    for query in [
        r"\@name",
        r"@name Contact \@name 中文",
        r"@name \@name @file user\@host.jar",
        r"@file C:\mods\user\@host.jar",
    ] {
        let parsed = filter::Filter::parse(query);
        assert!(!parsed.invalid(), "{query}");
        assert!(parsed.matches(row), "{query}");
    }
    let parsed = filter::Filter::parse(r"\@name @file user\@host.jar");
    assert_eq!(
        parsed
            .syntax
            .iter()
            .filter(|part| part.kind == filter::SyntaxKind::Field)
            .count(),
        1
    );
    let field = parsed
        .syntax
        .iter()
        .find(|part| part.kind == filter::SyntaxKind::Field)
        .unwrap();
    assert_eq!(&parsed.as_str()[field.range.clone()], "@file");
    assert!(!filter::Filter::parse("@name @host").matches(row));
}

#[test]
fn two_line_items_align_files_with_mod_ids_and_click_as_one_item() {
    use super::super::rows::{Paint, Piece};
    use crossterm::event::MouseButton;
    let mut browser = fixture(true);
    browser.nodes[0].row.cells[Column::Modid.position()] = vec![Piece::new("anchor", Paint::Plain)];
    browser.nodes[0].row.cells[Column::Filename.position()] =
        vec![Piece::new("sample.jar", Paint::Plain)];
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut page = 0;
    terminal
        .draw(|frame| {
            page = view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    let area = browser.table_area;
    let header = area.y + 1;
    let first = area.y + 3;
    let buffer = terminal.backend().buffer();
    let x = (area.x + 1..area.right() - 1)
        .find(|&x| {
            (0..6)
                .map(|offset| buffer[(x + offset, header)].symbol())
                .collect::<String>()
                == "Mod ID"
        })
        .unwrap();
    let at = |y, length| {
        (0..length)
            .map(|offset| buffer[(x + offset, y)].symbol())
            .collect::<String>()
    };
    assert_eq!(at(first, 6), "anchor");
    assert_eq!(at(first + 1, 10), "sample.jar");
    assert_eq!(buffer[(x, first)].bg, buffer[(x, first + 1)].bg);
    assert_eq!(page, (area.height.saturating_sub(4) / 2).max(1) as usize);
    for (y, expected) in [(first, 0), (first + 1, 0), (first + 2, 1), (first + 3, 1)] {
        input::handle(
            &mut browser,
            &summary(),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
            page,
        );
        assert_eq!(browser.table.selected(), Some(expected));
    }
    for columns in [
        vec![Column::Filename],
        vec![Column::Name, Column::Filename],
        vec![Column::Modid, Column::Filename],
    ] {
        for width in [1, 20, 80] {
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal
                .draw(|frame| {
                    view::draw(frame, &mut browser, &columns, &summary());
                })
                .unwrap();
        }
    }
}

#[test]
fn source_badges_and_copyable_details_belong_only_to_outer_archives() {
    use kako_mods_lib::source::{
        FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef, SourceId,
    };
    let mut browser = fixture(true);
    let matched = FileMatch {
        reference: FileRef {
            project: ProjectRef {
                source: SourceId("modrinth".into()),
                id: ProjectId("project-123".into()),
            },
            id: FileId("version-456".into()),
        },
        filename: "verified.jar".into(),
        size: 123,
        hashes: Vec::new(),
        role: MatchedFileRole::Primary,
    };
    browser.apply_identification(identify::Update {
        files: vec![0],
        platform: identify::Platform::Modrinth,
        state: identify::State::Matched(matched),
    });
    browser.apply_identification(identify::Update {
        files: vec![0],
        platform: identify::Platform::CurseForge,
        state: identify::State::Loading,
    });
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let y = browser.table_area.y + 4;
    let checks: Vec<_> = (0..30)
        .filter(|&x| buffer[(x, y)].symbol() == "✓")
        .collect();
    assert_eq!(checks.len(), 1, "matched badge is below Index");
    assert_eq!(
        buffer[(checks[0], y)].fg,
        crate::theme::widgets::source("modrinth")
            .fg
            .unwrap_or_default()
    );
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Modrinth: ✓ Identified · Project ID: project-123"));
    assert!(text.contains("CurseForge:"));
    assert!(text.contains("Identifying…"));
    let value = browser.detail_value(DETAIL_FIELDS[4]).unwrap();
    for expected in [
        "project-123",
        "version-456",
        "verified.jar",
        "Primary",
        "123 bytes",
    ] {
        assert!(value.contains(expected));
    }
    // File's fallback position must not paint over the badges when Mod ID is hidden.
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                &mut browser,
                &[Column::Index, Column::Filename],
                &summary(),
            );
        })
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(checks[0], y)].symbol(), "✓");
    let nested = browser
        .matches
        .iter()
        .position(|&index| !browser.nodes[index].outer)
        .unwrap();
    browser.select(nested);
    assert_eq!(
        browser.detail_value(DETAIL_FIELDS[4]).unwrap(),
        "Not checked (nested JAR)"
    );
    browser.details = true;
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!text.contains("project-123"));
    assert!(text.contains("Modrinth: Not checked"));
}

#[test]
fn enter_opens_the_matched_details_platform_only_and_preserves_selection() {
    use crossterm::event::{KeyCode, KeyEvent};
    use kako_mods_lib::source::{
        FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef, SourceId,
    };
    let mut browser = fixture(true);
    let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    browser.focus = Focus::Details;
    browser.query = "@file complex".into();
    browser.filter();
    for platform in identify::Platform::ALL {
        let reference = FileRef {
            project: ProjectRef {
                source: SourceId(platform.id().into()),
                id: ProjectId(format!("{}-project", platform.id())),
            },
            id: FileId(format!("{}-version", platform.id())),
        };
        browser.field = 4 + platform.index();
        // Pending, failed and unmatched fields retain normal Details behavior.
        for state in [
            identify::State::Loading,
            identify::State::Unmatched,
            identify::State::Failed("offline".into()),
        ] {
            browser.identification[0][platform.index()] = state;
            assert!(!matches!(
                input::handle(&mut browser, &summary(), enter.clone(), 10),
                input::Update::OpenFiles(_)
            ));
            browser.focus = Focus::Details;
        }
        browser.identification[0][platform.index()] = identify::State::Matched(FileMatch {
            reference: reference.clone(),
            filename: "matched.jar".into(),
            size: 1,
            hashes: Vec::new(),
            role: MatchedFileRole::Additional,
        });
        let position = browser.table.selected();
        let field = browser.field;
        let full_details = browser.details;
        let action = input::handle(&mut browser, &summary(), enter.clone(), 10);
        assert!(matches!(action, input::Update::OpenFiles(found) if found == reference));
        assert_eq!(browser.table.selected(), position);
        assert_eq!(browser.field, field);
        assert_eq!(browser.details, full_details);
        assert_eq!(browser.query, "@file complex");
    }
    let nested = browser
        .matches
        .iter()
        .position(|&index| !browser.nodes[index].outer)
        .unwrap();
    browser.select(nested);
    assert!(!matches!(
        input::handle(&mut browser, &summary(), enter.clone(), 10),
        input::Update::OpenFiles(_)
    ));
    browser.select(0);
    browser.focus = Focus::Mods;
    assert!(!matches!(
        input::handle(&mut browser, &summary(), enter, 10),
        input::Update::OpenFiles(_)
    ));
}

#[test]
fn two_line_source_entries_highlight_and_click_as_one_field_and_scroll_together() {
    use crossterm::event::{KeyCode, KeyEvent, MouseButton};
    use kako_mods_lib::source::{
        FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef, SourceId,
    };

    let mut browser = fixture(false);
    browser.focus = Focus::Details;
    browser.field = 4;
    for platform in identify::Platform::ALL {
        browser.identification[0][platform.index()] = identify::State::Matched(FileMatch {
            reference: FileRef {
                project: ProjectRef {
                    source: SourceId(platform.id().into()),
                    id: ProjectId("project".into()),
                },
                id: FileId("version".into()),
            },
            filename: "release.jar".into(),
            size: 1,
            hashes: vec![],
            role: MatchedFileRole::Primary,
        });
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    assert_eq!(browser.detail_offset, 0);
    let top = browser.detail_area.y + 1;
    let buffer = terminal.backend().buffer();
    let line = |y| {
        (browser.detail_area.x..browser.detail_area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    assert!(line(top + 4).contains("Modrinth: ✓ Identified"));
    let heading = line(top + 4);
    for part in ["Project ID: project", "File ID: version"] {
        let column = heading.find(part).unwrap();
        let x = browser.detail_area.x
            + unicode_width::UnicodeWidthStr::width(&heading[..column]) as u16;
        assert_eq!(
            buffer[(x, top + 4)].fg,
            crate::theme::widgets::style(crate::theme::Role::Muted)
                .fg
                .unwrap_or_default()
        );
        assert_eq!(
            buffer[(x, top + 4)]
                .modifier
                .contains(ratatui::style::Modifier::DIM),
            crate::theme::widgets::style(crate::theme::Role::Muted)
                .add_modifier
                .contains(ratatui::style::Modifier::DIM)
        );
    }
    assert!(line(top + 5).contains("Enter to view release files →"));
    assert!(line(top + 6).contains("CurseForge: ✓ Identified"));
    assert!(line(top + 7).contains("Enter to view release files →"));
    let x = browser.detail_area.x + 2;
    assert_eq!(buffer[(x, top + 4)].bg, buffer[(x, top + 5)].bg);
    if std::env::var_os("NO_COLOR").is_none() {
        assert_ne!(buffer[(x, top + 4)].bg, ratatui::style::Color::Reset);
        assert_ne!(buffer[(x, top + 5)].bg, buffer[(x, top + 6)].bg);
    }
    for (row, field) in [(4, 4), (5, 4), (6, 5), (7, 5)] {
        input::handle(
            &mut browser,
            &summary(),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: top + row,
                modifiers: KeyModifiers::NONE,
            }),
            10,
        );
        assert_eq!(browser.field, field);
        assert!(matches!(
            input::handle(
                &mut browser,
                &summary(),
                Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                10
            ),
            input::Update::OpenFiles(_)
        ));
    }
    browser.move_by(-1);
    assert_eq!(browser.field, 4);
    browser.move_by(1);
    assert_eq!(browser.field, 5);
    // The compact list keeps both lines of a selected source visible after scrolling.
    let mut short = Terminal::new(TestBackend::new(80, 11)).unwrap();
    short
        .draw(|frame| {
            view::draw(frame, &mut browser, &Column::ALL, &summary());
        })
        .unwrap();
    assert!(browser.detail_offset > 0);
    let text = short
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("CurseForge: ✓ Identified"));
    assert!(text.contains("Enter to view release files →"));
    let field_top: usize = DETAIL_FIELDS
        .iter()
        .skip(browser.detail_offset)
        .take(5 - browser.detail_offset)
        .map(|field| field.height())
        .sum();
    assert_eq!(browser.detail_field_at(field_top as u16), Some(5));
    assert_eq!(browser.detail_field_at(field_top as u16 + 1), Some(5));
}

#[test]
fn unavailable_source_entries_explain_their_status_without_an_enter_action() {
    let mut browser = fixture(false);
    browser.focus = Focus::Details;
    browser.details = true;
    for (state, status, reason) in [
        (identify::State::Queued, "Waiting", "Identification queued"),
        (
            identify::State::Loading,
            "Identifying…",
            "Looking up this file",
        ),
        (
            identify::State::Unmatched,
            "No match",
            "No matching release file found",
        ),
        (
            identify::State::Failed("Connection refused".into()),
            "Failed",
            "Connection refused",
        ),
    ] {
        for platform in identify::Platform::ALL {
            browser.identification[0][platform.index()] = state.clone();
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| {
                view::draw(frame, &mut browser, &Column::ALL, &summary());
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains(status));
        assert!(text.contains(reason));
        assert!(!text.contains("Enter to view release files"));
    }
}
