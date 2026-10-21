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
fn compact_list_details_and_pinned_selection_share_layout_without_filter() {
    let page = page(&["new", "pinned", "other"]);
    let pinned = file("pinned");
    let mut browser = Browser::new(true, 3);
    let output = render(120, 32, &page, Some(&pinned), &mut browser);
    assert!(output.contains("Selected"));
    assert!(output.contains("Details · Selected"));
    assert!(output.contains("Name pinned"));
    assert!(output.contains("version-new"));
    assert!(output.contains("version-other"));
    assert!(!output.contains("Filter"));
    assert!(browser.pinned_area.bottom() <= browser.table_area.y);
    assert!(browser.table_area.bottom() <= browser.detail_area.y);
    // The pinned publication retains its original position in the provider page.
    let table_lines = output
        .lines()
        .skip(browser.table_area.y as usize)
        .take(browser.table_area.height as usize)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(table_lines.contains("21"));
    assert!(table_lines.contains("23"));
    assert!(table_lines.contains("version-pinned"));
    assert!(table_lines.contains("22"));
    assert!(!table_lines.contains(".jar"));
    let lines = table_lines.lines().collect::<Vec<_>>();
    let name = lines
        .iter()
        .position(|line| line.contains("Name new"))
        .unwrap();
    assert!(lines[name].contains("1.21.1"));
    assert!(lines[name].contains("fabric"));
    assert!(lines[1].contains("Name"));
    assert!(lines[1].contains("Minecraft"));
    assert!(lines[2].contains("Version"));
    assert!(lines[2].contains("Release"));
    assert!(lines[name + 1].contains("version-new"));
    assert!(lines[name + 1].contains("release"));
    assert!(!lines[name + 1].contains("Version:"));
    assert!(!lines[name + 1].contains("Release:"));
    assert_eq!(lines[1].find("Name"), lines[2].find("Version"));
    assert_eq!(
        lines[2].find("Version"),
        lines[name + 1].find("version-new")
    );
    assert_eq!(lines[1].find("Minecraft"), lines[2].find("Release"));
    assert_eq!(lines[2].find("Release"), lines[name + 1].find("release"));
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
fn pagination_is_explicit_and_viewport_keys_never_fetch() {
    let page = page(&["one", "two", "three"]);
    let mut browser = Browser::new(false, 3);
    browser.visible_rows = 2;
    assert_eq!(
        key(&mut browser, KeyCode::PageDown, &page, None),
        input::Action::Redraw
    );
    assert_eq!(browser.table.selected(), Some(2));
    assert_eq!(
        key(&mut browser, KeyCode::Down, &page, None),
        input::Action::Redraw
    );
    assert_eq!(
        key(&mut browser, KeyCode::Right, &page, None),
        input::Action::Page(40)
    );
    assert_eq!(
        key(&mut browser, KeyCode::Left, &page, None),
        input::Action::Page(0)
    );
    let first = FilePage {
        offset: 0,
        total: Some(3),
        ..page.clone()
    };
    assert_eq!(
        key(&mut browser, KeyCode::Left, &first, None),
        input::Action::Redraw
    );
    assert_eq!(
        key(&mut browser, KeyCode::Right, &first, None),
        input::Action::Redraw
    );
    let unknown = FilePage {
        total: None,
        ..page.clone()
    };
    assert_eq!(
        key(&mut browser, KeyCode::Char('n'), &unknown, None),
        input::Action::Page(40)
    );
    let empty = FilePage {
        files: vec![],
        ..unknown
    };
    assert_eq!(
        key(&mut browser, KeyCode::Right, &empty, None),
        input::Action::Redraw
    );
}

#[test]
fn focus_full_details_field_navigation_and_return() {
    let page = page(&["one"]);
    let mut browser = Browser::new(false, 1);
    key(&mut browser, KeyCode::Tab, &page, None);
    assert!(browser.focus == Focus::Details);
    key(&mut browser, KeyCode::End, &page, None);
    assert_eq!(browser.field, view::FIELD_COUNT - 1);
    key(&mut browser, KeyCode::Enter, &page, None);
    assert!(browser.full_details);
    let output = render(80, 16, &page, None, &mut browser);
    assert!(output.contains("Hashes:"));
    assert_eq!(browser.table_area, Rect::default());
    assert!(browser.detail_offset > 0);
    assert_eq!(
        view::field_value(&page.files[0], 8),
        ("Filename", "one.jar".into())
    );
    key(&mut browser, KeyCode::Esc, &page, None);
    assert!(!browser.full_details);
    assert!(browser.focus == Focus::Files);
    assert_eq!(
        key(&mut browser, KeyCode::Esc, &page, None),
        input::Action::Quit
    );
    for code in [KeyCode::Char('F'), KeyCode::Char('f'), KeyCode::Char('r')] {
        key(&mut browser, code, &page, None);
        assert!(browser.focus == Focus::Files);
        assert_eq!(browser.table.selected(), Some(0));
    }
    assert_eq!(
        key(&mut browser, KeyCode::Char(']'), &page, None),
        input::Action::Project(false)
    );
    assert_eq!(
        key(&mut browser, KeyCode::Char('['), &page, None),
        input::Action::Project(true)
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

#[test]
fn resizing_empty_pages_and_long_unicode_values_stay_bounded() {
    for ids in [&[][..], &["one"][..]] {
        let mut page = page(ids);
        if let Some(file) = page.files.first_mut() {
            file.name = "中文👩‍💻长名称".repeat(40);
            file.filename = "很长的文件名".repeat(40);
        }
        let pinned = file("pinned");
        for size in [(1, 1), (27, 9), (28, 10), (40, 18), (80, 24), (160, 50)] {
            for pin in [None, Some(&pinned)] {
                for details in [false, true] {
                    let mut browser = Browser::new(pin.is_some(), ids.len());
                    browser.full_details = details;
                    let output = render(size.0, size.1, &page, pin, &mut browser);
                    assert!(!output.contains("Filter"));
                    key(&mut browser, KeyCode::Home, &page, pin);
                    key(&mut browser, KeyCode::End, &page, pin);
                }
            }
        }
    }
}
