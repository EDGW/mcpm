//! Pinned publication selection and mouse mapping across rendered rows.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use kako_mods_lib::source::{FileId, ProjectId, ReleaseType, SourceId};
use ratatui::{Terminal, backend::TestBackend};

fn file(id: &str) -> ProjectFile {
    ProjectFile {
        reference: FileRef {
            project: ProjectRef {
                source: SourceId("modrinth".into()),
                id: ProjectId("project".into()),
            },
            id: FileId(id.into()),
        },
        name: format!("Name {id}"),
        version: Some(format!("version-{id}")),
        release_type: ReleaseType::Release,
        game_versions: vec!["1.21.1".into()],
        loaders: vec!["fabric".into()],
        filename: format!("{id}.jar"),
        size: 123,
        download_url: None,
        hashes: vec![],
        dependencies: vec![],
    }
}
fn page(ids: &[&str]) -> FilePage {
    FilePage {
        files: ids.iter().map(|id| file(id)).collect(),
        offset: 20,
        total: Some(100),
    }
}
fn key(
    browser: &mut Browser,
    code: KeyCode,
    page: &FilePage,
    pinned: Option<&ProjectFile>,
) -> input::Action {
    input::handle(
        browser,
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        page,
        pinned,
        20,
    )
}
fn render(
    width: u16,
    height: u16,
    page: &FilePage,
    pinned: Option<&ProjectFile>,
    browser: &mut Browser,
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                "Project",
                &file("ref").reference.project,
                page,
                pinned,
                &FileQuery::default(),
                0,
                1,
                browser,
            )
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn pinned_selection_preserves_provider_indices_and_navigation_order() {
    let page = page(&["new", "pinned", "other"]);
    let pinned = file("pinned");
    let mut browser = Browser::new(true, 3);
    let output = render(120, 32, &page, Some(&pinned), &mut browser);
    // The pinned publication retains its original position in the provider page.
    let table_lines = output
        .lines()
        .skip(browser.table_area.y as usize)
        .take(browser.table_area.height as usize)
        .collect::<Vec<_>>();
    let mut previous = None;
    for (name, index) in [("new", "21"), ("pinned", "22"), ("other", "23")] {
        let row = table_lines
            .iter()
            .position(|line| line.contains(&format!("Name {name}")))
            .unwrap();
        let prefix = table_lines[row].split("Name").next().unwrap();
        assert_eq!(
            prefix
                .split(|ch: char| !ch.is_ascii_digit())
                .find(|part| !part.is_empty()),
            Some(index)
        );
        assert!(previous.is_none_or(|previous| previous < row));
        previous = Some(row);
    }
    key(&mut browser, KeyCode::Down, &page, Some(&pinned));
    assert_eq!(
        browser.file(&page, Some(&pinned)).unwrap().reference.id.0,
        "new"
    );
    key(&mut browser, KeyCode::Up, &page, Some(&pinned));
    assert!(browser.pinned);
    key(&mut browser, KeyCode::End, &page, Some(&pinned));
    assert_eq!(
        browser.file(&page, Some(&pinned)).unwrap().reference.id.0,
        "other"
    );
}

#[test]
fn mouse_selects_rows_fields_and_pinned_file() {
    let page = page(&["one", "two"]);
    let pinned = file("pinned");
    let mut browser = Browser::new(true, 2);
    render(100, 32, &page, Some(&pinned), &mut browser);
    let click = |column, row| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    let area = browser.table_area;
    input::handle(
        &mut browser,
        click(area.x + 2, area.y + 1 + view::TABLE_HEADER_HEIGHT + 2),
        &page,
        Some(&pinned),
        20,
    );
    assert_eq!(browser.table.selected(), Some(1));
    assert!(!browser.pinned);
    let area = browser.detail_area;
    input::handle(
        &mut browser,
        click(area.x + 2, area.y + 3),
        &page,
        Some(&pinned),
        20,
    );
    assert_eq!(browser.field, 2);
    assert!(browser.focus == Focus::Details);
    let area = browser.pinned_area;
    input::handle(
        &mut browser,
        click(area.x + 2, area.y + 1),
        &page,
        Some(&pinned),
        20,
    );
    assert!(browser.pinned);
    assert!(browser.focus == Focus::Files);
}
