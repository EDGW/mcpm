//! Completion regressions for draft-only edits, version caching and popup interaction.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use kako_mods_lib::source::{SearchPage, SearchQuery};
use ratatui::{Terminal, backend::TestBackend};

use super::super::{
    Browser, Focus,
    input::{self, Action},
    view,
};
use crate::theme::{Role, widgets};

fn page() -> SearchPage {
    SearchPage {
        projects: Vec::new(),
        offset: 0,
        total: None,
    }
}

fn press(browser: &mut Browser, code: KeyCode) -> Action {
    browser.handle(KeyEvent::new(code, KeyModifiers::NONE), &page(), 20)
}

#[test]
fn loader_selection_only_edits_the_current_value_and_requires_another_enter_to_search() {
    let mut browser = Browser::new("机械动力 @loader");
    browser.focus = Focus::Search;
    browser.refresh_completion();
    assert_eq!(
        browser.completion.candidates,
        ["forge", "neoforge", "fabric"]
    );
    assert!(browser.completion.needs_versions());
    assert_eq!(press(&mut browser, KeyCode::Down), Action::Redraw);
    assert_eq!(press(&mut browser, KeyCode::Enter), Action::Complete);
    assert_eq!(browser.draft, "机械动力 @loader neoforge");
    assert_eq!(browser.committed, "机械动力 @loader");
    assert!(!browser.completion.open());
    assert_eq!(press(&mut browser, KeyCode::Enter), Action::Search);

    browser.draft = "机械动力 @loader (forge | faXX) @version 1.21.1".into();
    browser.cursor = browser.draft.find("faXX").unwrap() + 2;
    browser.refresh_completion();
    assert_eq!(browser.completion.candidates, ["fabric"]);
    assert_eq!(press(&mut browser, KeyCode::Tab), Action::Complete);
    assert_eq!(
        browser.draft,
        "机械动力 @loader (forge | fabric) @version 1.21.1"
    );
    assert_eq!(
        &browser.draft[..browser.cursor],
        "机械动力 @loader (forge | fabric"
    );
    let pending = browser.prepare(&SearchQuery::default()).unwrap();
    assert_eq!(pending.loaders, ["forge", "fabric"]);

    browser.draft = "@loader ".into();
    browser.cursor = browser.draft.len();
    assert_eq!(press(&mut browser, KeyCode::Esc), Action::Redraw);
    assert!(browser.focus == Focus::Search && !browser.completion.open());
    assert_eq!(press(&mut browser, KeyCode::Esc), Action::Redraw);
    assert!(browser.focus == Focus::Results);
    assert_eq!(browser.draft, browser.committed);
}

#[test]
fn versions_cache_per_session_and_allow_manual_input_after_failure() {
    let mut browser = Browser::new("@version");
    browser.focus = Focus::Search;
    browser.refresh_completion();
    assert!(browser.completion.needs_versions());
    browser.completion.loading();
    for ch in " 1.20".chars() {
        assert_eq!(press(&mut browser, KeyCode::Char(ch)), Action::Redraw);
    }
    assert_eq!(browser.draft, "@version 1.20");
    assert!(!browser.completion.needs_versions());
    browser
        .completion
        .finish_versions(Ok(vec!["1.21.1".into(), "1.20.6".into(), "1.20.1".into()]));
    browser.refresh_completion();
    assert_eq!(browser.completion.candidates, ["1.20.6", "1.20.1"]);
    assert_eq!(press(&mut browser, KeyCode::Down), Action::Redraw);
    assert_eq!(press(&mut browser, KeyCode::Enter), Action::Complete);
    assert_eq!(browser.draft, "@version 1.20.1");
    assert_eq!(press(&mut browser, KeyCode::Enter), Action::Search);
    browser.draft = "@version (1.20.1 | ".into();
    browser.cursor = browser.draft.len();
    browser.refresh_completion();
    assert!(!browser.completion.needs_versions());
    assert_eq!(
        browser.completion.candidates,
        ["1.21.1", "1.20.6", "1.20.1"]
    );

    browser
        .completion
        .finish_versions(Err(anyhow::anyhow!("offline")));
    browser.draft = "@version 9.99.1".into();
    browser.cursor = browser.draft.len();
    browser.refresh_completion();
    assert!(browser.completion.candidates.is_empty());
    assert_eq!(press(&mut browser, KeyCode::Enter), Action::Search);
    assert_eq!(
        browser
            .prepare(&SearchQuery::default())
            .unwrap()
            .game_versions,
        ["9.99.1"]
    );
    assert_eq!(
        browser.handle(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
            &page(),
            20
        ),
        Action::Redraw
    );
    assert!(browser.completion.needs_versions());
}

#[test]
fn popup_styles_mouse_selection_and_resizing_follow_the_visible_candidates() {
    for (width, height) in [(28, 16), (80, 24)] {
        for version in [false, true] {
            let mut browser = Browser::new(if version { "@version " } else { "@loader " });
            browser.focus = Focus::Search;
            if version {
                browser
                    .completion
                    .finish_versions(Ok((0..10).map(|i| format!("1.21.{i}")).collect()));
            }
            browser.refresh_completion();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    view::draw(
                        frame,
                        "modrinth",
                        &SearchQuery::default(),
                        &page(),
                        &mut browser,
                        "",
                    )
                })
                .unwrap();
            let area = browser.completion.area;
            assert_eq!(area.y, browser.search_area.bottom());
            assert!(area.right() <= width && area.bottom() <= height);
            let buffer = terminal.backend().buffer();
            assert_eq!(
                buffer[(area.x, area.y)].fg,
                widgets::style(Role::Secondary).fg.unwrap_or_default()
            );
            if version {
                assert_eq!(
                    buffer[(area.x + 1, area.y + 1)].fg,
                    widgets::style(Role::Normal).fg.unwrap_or_default()
                );
            }
            let mouse = |kind| {
                Event::Mouse(MouseEvent {
                    kind,
                    column: area.x + 1,
                    row: area.y + 2,
                    modifiers: KeyModifiers::NONE,
                })
            };
            assert_eq!(
                input::handle(&mut browser, mouse(MouseEventKind::ScrollDown), &page(), 20),
                Action::Redraw
            );
            assert_eq!(browser.completion.state.selected(), Some(1));
            assert_eq!(
                input::handle(
                    &mut browser,
                    mouse(MouseEventKind::Down(MouseButton::Left)),
                    &page(),
                    20
                ),
                Action::Complete
            );
            assert_eq!(
                browser.draft,
                if version {
                    "@version 1.21.1"
                } else {
                    "@loader neoforge"
                }
            );
            assert!(browser.committed.ends_with(' '));

            terminal
                .resize(ratatui::layout::Rect::new(0, 0, 20, 8))
                .unwrap();
            terminal
                .draw(|frame| {
                    view::draw(
                        frame,
                        "modrinth",
                        &SearchQuery::default(),
                        &page(),
                        &mut browser,
                        "",
                    )
                })
                .unwrap();
            assert_eq!(browser.completion.area.height, 0);
        }
    }
}
