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

use super::{
    Browser, Focus,
    input::{self, Action},
    view,
};

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
fn search_scrolls_with_unicode_cursor_inside_viewport() {
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
    assert!(rows(&terminal)[1].contains("@loader fabric"));
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    assert!(cursor.x < 26 && cursor.y == 1);
    assert_eq!(browser.draft, source);
}

#[test]
fn selection_stays_visible_across_resizing_and_detail_expansion() {
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
        categories: vec!["technology".into()],
        loaders: [
            "quilt",
            "fabric",
            "forge",
            "liteloader",
            "neoforge",
            "forge",
        ]
        .map(str::to_owned)
        .into(),
        game_versions: vec!["1.20.1".into(), "1.21.1".into()],
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
        let result_rows =
            &rendered[usize::from(browser.table_area.y)..usize::from(browser.table_area.bottom())];
        assert!(result_rows.iter().any(|row| row.contains("机械19")));
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
        assert!(browser.detail_offset > 0);
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
        browser.field = 4;
        browser.detail_offset = 0;
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
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
                categories: Vec::new(),
                loaders: Vec::new(),
                game_versions: Vec::new(),
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
    assert_eq!(
        input::handle(
            &mut browser,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y + 1),
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

#[test]
fn wrapped_version_rows_preserve_full_values_and_field_navigation() {
    let mut versions: Vec<_> = (1..=20)
        .rev()
        .map(|patch| format!("1.20.{patch}"))
        .collect();
    versions.extend(["1.20", "22.1", "22.2.1", "1.19.4", "24w14a", "1.19.4"].map(str::to_owned));
    let page = SearchPage {
        projects: vec![ProjectInfo {
            reference: ProjectRef {
                source: SourceId("modrinth".into()),
                id: ProjectId("test".into()),
            },
            name: "Create".into(),
            summary: String::new(),
            slug: Some("create".into()),
            categories: vec!["technology".into()],
            loaders: vec!["forge".into()],
            game_versions: versions.clone(),
            icon_url: None,
            website_url: None,
        }],
        offset: 0,
        total: Some(1),
    };
    let mut browser = Browser::new("create");
    browser.completion.finish_versions(Ok(versions));
    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    let query = SearchQuery::default();
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    browser.focus = Focus::Details;
    browser.full_details = true;
    browser.field = 5;
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    let version_rows: Vec<_> = browser
        .detail_fields
        .iter()
        .enumerate()
        .filter_map(|(row, &field)| (field == 5).then_some(row))
        .collect();
    assert!(version_rows.len() > 2);
    let full = view::field_value(&page.projects[0], 5).1;
    for &row in &version_rows {
        browser.select_detail_row(row);
        assert_eq!(browser.field, 5);
        assert_eq!(view::field_value(&page.projects[0], browser.field).1, full);
    }
    browser.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &page, 20);
    assert_eq!(browser.field, 6);
    browser.handle(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &page, 20);
    assert_eq!(browser.field, 5);
    assert_eq!(browser.detail_row, *version_rows.last().unwrap());
    terminal
        .resize(ratatui::layout::Rect::new(28, 16, 28, 16))
        .unwrap();
    terminal
        .draw(|frame| view::draw(frame, "modrinth", &query, &page, &mut browser, ""))
        .unwrap();
    browser.handle(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), &page, 20);
    assert_eq!(browser.field, view::FIELD_COUNT - 1);
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
