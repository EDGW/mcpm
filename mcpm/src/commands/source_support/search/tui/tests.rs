//! Regression coverage for deferred searches, Unicode editing, and bounded layouts.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use kako_mods_lib::source::{
    ProjectId, ProjectInfo, ProjectRef, SearchPage, SearchQuery, SourceId,
};
use ratatui::{
    Terminal,
    backend::{Backend, TestBackend},
};
use unicode_width::UnicodeWidthStr;

use crate::theme::{Role, widgets};

use super::{
    Browser, Focus,
    input::{self, Action},
    view,
};

#[test]
fn editing_defers_search_until_enter_and_escape_restores_committed_query() {
    let page = SearchPage {
        projects: vec![],
        offset: 40,
        total: Some(40),
    };
    let mut browser = Browser::new("create");
    let mut press = |code| browser.handle(KeyEvent::new(code, KeyModifiers::NONE), &page, 20);
    assert_eq!(press(KeyCode::Char('/')), Action::Redraw);
    for ch in " qjknp/".chars() {
        assert_eq!(press(KeyCode::Char(ch)), Action::Redraw);
    }
    assert_eq!(press(KeyCode::Enter), Action::Search);
    assert_eq!(browser.draft, "create qjknp/");
    assert_eq!(browser.table.selected(), None);
    assert_eq!(
        browser.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &page, 20),
        Action::Redraw
    );
    assert_eq!(browser.draft, "create");
    assert!(browser.focus == Focus::Results);
    browser.focus = Focus::Search;
    browser.draft.clear();
    browser.cursor = 0;
    assert_eq!(
        browser.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &page, 20),
        Action::Search
    );
}

#[test]
fn invalid_expression_preserves_selection_and_can_be_corrected_without_losing_defaults() {
    let defaults = SearchQuery {
        game_versions: vec!["1.20.1".into()],
        loaders: vec!["forge".into()],
        ..Default::default()
    };
    let mut browser = Browser::new("机械动力 @loader fabric");
    browser.focus = Focus::Search;
    browser.table.select(Some(7));
    *browser.table.offset_mut() = 4;
    browser.draft = "机械动力 @version (1.21.1 |)".into();
    let error = browser.prepare(&defaults).unwrap_err();
    assert_eq!(
        browser.draft[..browser.cursor].chars().count() + 1,
        error.column
    );
    assert!(browser.parse_error.is_some());
    assert_eq!(browser.committed, "机械动力 @loader fabric");
    assert_eq!(
        (browser.table.selected(), browser.table.offset()),
        (Some(7), 4)
    );

    browser.draft = "机械动力 @loader *".into();
    let pending = browser.prepare(&defaults).unwrap();
    assert!(browser.parse_error.is_none());
    assert_eq!(pending.game_versions, ["1.20.1"]);
    assert!(pending.loaders.is_empty());
    assert_eq!(pending.offset, 0);
    assert_eq!(browser.committed, "机械动力 @loader fabric");
}

#[test]
fn unicode_editing_keeps_cursor_on_character_boundaries() {
    let page = SearchPage {
        projects: vec![],
        offset: 0,
        total: Some(0),
    };
    let mut browser = Browser::new("机械动力🦀");
    browser.focus = Focus::Search;
    for code in [
        KeyCode::Left,
        KeyCode::Backspace,
        KeyCode::Char('猫'),
        KeyCode::Delete,
    ] {
        assert_eq!(
            browser.handle(KeyEvent::new(code, KeyModifiers::NONE), &page, 20),
            Action::Redraw
        );
        assert!(browser.draft.is_char_boundary(browser.cursor));
    }
    assert_eq!(browser.draft, "机械动猫");
    assert_eq!(browser.cursor, browser.draft.len());
}

#[test]
fn search_editor_highlights_parser_tokens_literals_and_live_errors() {
    use ratatui::style::Modifier;

    let page = SearchPage {
        projects: vec![],
        offset: 0,
        total: Some(0),
    };
    let query = SearchQuery::default();
    let source = r#""机械 @version" \@literal @version (1.20.1 | 1.21.1) @loader *"#;
    let mut browser = Browser::new(source);
    browser.focus = Focus::Search;
    browser.cursor = 0;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    let buffer = terminal.backend().buffer();
    for (token, role) in [
        ("\"", Role::Muted),
        ("机械", Role::Secondary),
        ("@version\"", Role::Secondary),
        ("\\@", Role::Primary),
        ("@version (", Role::Primary),
        ("(", Role::Muted),
        ("1.20.1", Role::Secondary),
        ("|", Role::Important),
        (")", Role::Muted),
        ("@loader", Role::Primary),
        ("*", Role::Important),
    ] {
        let offset = source[..source.find(token).unwrap()].width() as u16;
        assert_eq!(
            buffer[(2 + offset, 1)].fg,
            widgets::style(role).fg.unwrap_or_default(),
            "{token}"
        );
    }
    assert_eq!(browser.committed, source);

    browser.draft = "机械 @unknown value @loader fabric".into();
    // An edit is diagnosed locally before Enter, without changing committed results.
    assert!(browser.parse_error.is_none());
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    let buffer = terminal.backend().buffer();
    for (index, ch) in "@unknown".chars().enumerate() {
        let cell = &buffer[(7 + index as u16, 1)];
        assert_eq!(cell.symbol(), ch.to_string());
        assert_eq!(cell.fg, widgets::style(Role::Danger).fg.unwrap_or_default());
        assert!(cell.modifier.contains(Modifier::UNDERLINED));
    }
    let offset = browser.draft[..browser.draft.find("@loader").unwrap()].width() as u16;
    assert_eq!(
        buffer[(2 + offset, 1)].fg,
        widgets::style(Role::Primary).fg.unwrap_or_default()
    );
    let title = &rows(&terminal)[0];
    assert!(title.contains("unknown field; use @version or @loader at column 4"));
    assert_eq!(
        buffer[(0, 0)].fg,
        widgets::style(Role::Warning).fg.unwrap_or_default()
    );
    assert_eq!(browser.committed, source);

    browser.focus = Focus::Results;
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(0, 0)].fg,
        widgets::style(Role::Warning).fg.unwrap_or_default()
    );

    browser.draft = source.into();
    browser.focus = Focus::Search;
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(0, 0)].fg,
        widgets::style(Role::Primary).fg.unwrap_or_default()
    );
    assert!(!rows(&terminal)[0].contains("column"));

    browser.draft = "@loader ".into();
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    let cell = &terminal.backend().buffer()[(10, 1)];
    assert!(cell.modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn highlighted_search_scrolls_with_unicode_cursor_and_keeps_token_colors() {
    let source = format!("{} @loader fabric", "机械动力🦀".repeat(20));
    let page = SearchPage {
        projects: vec![],
        offset: 0,
        total: None,
    };
    let mut browser = Browser::new(&source);
    browser.focus = Focus::Search;
    let mut terminal = Terminal::new(TestBackend::new(28, 16)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                "modrinth",
                &SearchQuery::default(),
                &page,
                &mut browser,
                "",
            )
        })
        .unwrap();
    let rendered = rows(&terminal);
    for (token, role) in [("@loader", Role::Primary), ("fabric", Role::Secondary)] {
        let offset = rendered[1][..rendered[1].find(token).unwrap()].width() as u16;
        assert_eq!(
            terminal.backend().buffer()[(offset, 1)].fg,
            widgets::style(role).fg.unwrap_or_default()
        );
    }
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    assert!(cursor.x < 26 && cursor.y == 1);
    assert_eq!(browser.draft, source);
}

#[test]
fn shared_table_keeps_selection_visible_and_details_support_fields_and_expansion() {
    let query = SearchQuery {
        query: "机械动力".repeat(30),
        ..Default::default()
    };
    let project = ProjectInfo {
        reference: ProjectRef {
            source: SourceId("curseforge".into()),
            id: ProjectId("328085".into()),
        },
        name: "机械动力 Create".into(),
        summary: "A detailed description of the selected mod. ".repeat(15),
        slug: Some("create".into()),
        icon_url: None,
        website_url: Some(
            "https://www.curseforge.com/minecraft/mc-mods/create"
                .parse()
                .unwrap(),
        ),
    };
    let page = SearchPage {
        projects: (0..20)
            .map(|index| ProjectInfo {
                name: format!("机械{index}"),
                slug: Some(format!("mod{index}")),
                ..project.clone()
            })
            .collect(),
        offset: 0,
        total: Some(20),
    };
    for (width, height) in [(28, 16), (80, 24), (140, 45)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut browser = Browser::new(&query.query);
        browser.select(19, page.projects.len());
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        let rendered = rows(&terminal);
        assert!(rendered[0].contains("Search"));
        let result_rows =
            &rendered[usize::from(browser.table_area.y)..usize::from(browser.table_area.bottom())];
        assert!(
            result_rows
                .iter()
                .any(|row| row.contains("机械19") && row.contains("mod19"))
        );
        assert!(!result_rows.iter().any(|row| row.contains("curseforge.com")
            || row.contains("detailed")
            || row.contains("328085")));
        assert!(browser.table.offset() > 0);

        browser.focus = Focus::Details;
        browser.handle(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            &page,
            query.limit,
        );
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        assert_eq!(browser.field, view::FIELD_COUNT - 1);
        let rendered = rows(&terminal);
        let details = &rendered
            [usize::from(browser.detail_area.y)..usize::from(browser.detail_area.bottom())];
        assert!(details.iter().any(|row| row.contains("Website:")));
        if width >= 80 {
            assert!(details.iter().any(|row| row.contains("mc-mods/create")));
        }
        assert_eq!(
            view::field_value(browser.project(&page).unwrap(), browser.field).1,
            "https://www.curseforge.com/minecraft/mc-mods/create"
        );

        assert_eq!(
            browser.handle(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &page,
                query.limit
            ),
            Action::Redraw
        );
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        assert!(browser.full_details);
        assert_eq!(browser.table_area.height, 0);
        assert!(rows(&terminal)[0].contains("Search"));
        assert_eq!(
            browser.handle(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &page,
                query.limit
            ),
            Action::Redraw
        );
        assert!(!browser.full_details);
        assert!(browser.focus == Focus::Results);
        assert_eq!(browser.table.selected(), Some(19));

        browser.focus = Focus::Search;
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        let cursor = terminal.backend_mut().get_cursor_position().unwrap();
        assert!(cursor.x < width - 1 && cursor.y == 1);
    }
}

#[test]
fn mouse_navigation_uses_rendered_table_offset_and_never_fetches_pages() {
    let page = SearchPage {
        projects: vec![
            ProjectInfo {
                reference: ProjectRef {
                    source: SourceId("modrinth".into()),
                    id: ProjectId("test".into())
                },
                name: "Create".into(),
                summary: String::new(),
                slug: Some("create".into()),
                icon_url: None,
                website_url: None,
            };
            20
        ],
        offset: 20,
        total: Some(100),
    };
    let mut browser = Browser::new("create");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    browser.select(19, page.projects.len());
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                "modrinth",
                &SearchQuery::default(),
                &page,
                &mut browser,
                "",
            )
        })
        .unwrap();
    let mouse = |kind, column, row| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    let x = browser.table_area.x + 1;
    let y = browser.table_area.y + 1 + view::TABLE_HEADER_HEIGHT;
    let offset = browser.table.offset();
    assert_eq!(
        input::handle(
            &mut browser,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y),
            &page,
            20
        ),
        Action::Redraw
    );
    assert_eq!(browser.table.selected(), Some(offset));
    browser.select(19, page.projects.len());
    assert_eq!(
        input::handle(
            &mut browser,
            mouse(MouseEventKind::ScrollDown, x, y),
            &page,
            20
        ),
        Action::Redraw
    );
    assert_eq!(browser.table.selected(), Some(19));
    assert_eq!(
        browser.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &page, 20),
        Action::Page(40)
    );
    let search = browser.search_area;
    assert_eq!(
        input::handle(
            &mut browser,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                search.x + 2,
                search.y + 1
            ),
            &page,
            20
        ),
        Action::Redraw
    );
    assert!(browser.focus == Focus::Search);
}

fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(terminal.size().unwrap().width))
        .map(|row| {
            let mut value = String::new();
            let mut cells = row.iter();
            while let Some(cell) = cells.next() {
                value.push_str(cell.symbol());
                for _ in 1..cell.symbol().width() {
                    cells.next();
                }
            }
            value
        })
        .collect()
}
