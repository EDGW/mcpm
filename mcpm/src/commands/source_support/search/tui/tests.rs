//! Regression coverage for deferred searches, Unicode editing, and bounded layouts.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kako_mods_lib::source::{
    ProjectId, ProjectInfo, ProjectRef, SearchPage, SearchQuery, SourceId,
};
use ratatui::{
    Terminal,
    backend::{Backend, TestBackend},
};
use unicode_width::UnicodeWidthStr;

use super::{Browser, Focus, input::Action, view};

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
    assert_eq!(browser.selected, 0);
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
    browser.selected = 7;
    browser.result_offset = 4;
    browser.draft = "机械动力 @version (1.21.1 |)".into();
    let error = browser.prepare(&defaults).unwrap_err();
    assert_eq!(
        browser.draft[..browser.cursor].chars().count() + 1,
        error.column
    );
    assert!(browser.parse_error.is_some());
    assert_eq!(browser.committed, "机械动力 @loader fabric");
    assert_eq!((browser.selected, browser.result_offset), (7, 4));

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
fn result_list_keeps_selection_visible_and_details_scroll_to_full_url() {
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
        browser.selected = 19;
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        let rendered = rows(&terminal);
        let detail_row = rendered
            .iter()
            .position(|row| row.contains("Details"))
            .unwrap();
        let result_rows = &rendered[5..detail_row];
        assert!(
            result_rows
                .iter()
                .any(|row| row.contains("机械19") && row.contains("mod19"))
        );
        assert!(!result_rows.iter().any(|row| row.contains("curseforge.com")
            || row.contains("detailed")
            || row.contains("328085")));
        assert!(browser.detail_limit > 0);

        browser.focus = Focus::Details;
        browser.handle(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            &page,
            query.limit,
        );
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        let details: String = rows(&terminal)[detail_row + 1..usize::from(height) - 4]
            .iter()
            .map(|row| row.trim().trim_matches('│').trim())
            .collect();
        assert!(details.contains("mc-mods/create"));
        assert_eq!(browser.detail_scroll, browser.detail_limit);

        browser.focus = Focus::Search;
        terminal
            .draw(|frame| view::draw(frame, "curseforge", &query, &page, &mut browser, ""))
            .unwrap();
        let cursor = terminal.backend_mut().get_cursor_position().unwrap();
        assert!(cursor.x < width - 1 && cursor.y == 1);
    }
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
