//! Expression regressions for precedence, exact alternatives, quoting and error columns.

use kako_mods_lib::source::{SearchQuery, SearchSort};

use super::Expression;
use super::{CompletionField, completion_context};

#[test]
fn fields_override_defaults_and_star_clears_each_filter_independently() {
    let defaults = SearchQuery {
        game_versions: vec!["1.19.2".into()],
        loaders: vec!["forge".into()],
        sort: SearchSort::Updated,
        offset: 40,
        limit: 10,
        ..Default::default()
    };
    let query =
        Expression::parse("create @version (1.20.1 | 1.21.1 | 1.20.1) @loader (Fabric | NeoForge)")
            .unwrap()
            .apply(&defaults);
    assert_eq!(query.query, "create");
    assert_eq!(query.game_versions, ["1.20.1", "1.21.1"]);
    assert_eq!(query.loaders, ["fabric", "neoforge"]);
    assert_eq!(query.offset, 0);
    assert_eq!(query.limit, 10);
    assert_eq!(query.sort, SearchSort::Updated);

    let query = Expression::parse("@version *").unwrap().apply(&defaults);
    assert!(query.game_versions.is_empty());
    assert_eq!(query.loaders, ["forge"]);
    let query = Expression::parse("@loader *").unwrap().apply(&defaults);
    assert_eq!(query.game_versions, ["1.19.2"]);
    assert!(query.loaders.is_empty());
    let query = Expression::parse("").unwrap().apply(&defaults);
    assert_eq!(query.game_versions, defaults.game_versions);
    assert_eq!(query.loaders, defaults.loaders);
}

#[test]
fn keyword_phrases_remain_one_provider_query_and_quoted_syntax_is_literal() {
    for (expression, expected) in [
        ("Steam n Rails @loader fabric", "Steam n Rails"),
        ("\"Steam 'n' Rails\" @loader fabric", "Steam 'n' Rails"),
        ("'@loader (A | B)' @version 1.21.1", "@loader (A | B)"),
        (r"user\@host \(mod\)", "user@host (mod)"),
        (r#""He said \"hello\"""#, "He said \"hello\""),
        ("机械动力 @version 1.21.1", "机械动力"),
    ] {
        let query = Expression::parse(expression)
            .unwrap()
            .apply(&SearchQuery::default());
        assert_eq!(query.query, expected);
    }
}

#[test]
fn unsupported_or_incomplete_expressions_report_character_columns() {
    for expression in [
        "@unknown fabric",
        "@loader",
        "@version ()",
        "@version (1.20.1 |)",
        "@version (1.20.1 1.21.1)",
        "@version (1.20.1 | (1.21.1))",
        "@loader fabric @loader forge",
        "@version >=1.20",
        "@loader ~fabric",
        "@version (1.20.1 | *)",
        "@loader fabric extra",
        "create | sodium",
        "@version (1.20.1",
        "\"unclosed",
        "@loader ''",
        "trailing\\",
        "@loader @version 1.21.1",
        "@loader fabric & @version 1.21.1",
    ] {
        let error = Expression::parse(expression).unwrap_err();
        assert!(
            (1..=expression.chars().count() + 1).contains(&error.column),
            "{expression}"
        );
    }
    let error = Expression::parse("机械 @unknown x").unwrap_err();
    assert_eq!(error.column, 4);
}

#[test]
fn draft_inspection_matches_submission_and_keeps_ranges_on_utf8_boundaries() {
    for source in [
        r#""机械 @loader" \@literal @version (1.20.1 | 1.21.1) @loader *"#,
        r#""He said \"hello\"" @loader Fabric"#,
        "机械 @unknown x @loader fabric",
        "@version (1.20.1 |) @loader fabric",
        "@loader fabric & @version 1.21.1",
        "🦀\\",
    ] {
        // Every character boundary is a possible draft while typing or deleting.
        for end in source
            .char_indices()
            .map(|(index, _)| index)
            .chain([source.len()])
        {
            let draft = &source[..end];
            let (parsed, syntax) = Expression::inspect(draft);
            assert_eq!(parsed, Expression::parse(draft), "{draft}");
            for range in syntax
                .iter()
                .map(|span| &span.range)
                .chain(parsed.as_ref().err().map(|error| &error.range))
            {
                assert!(
                    range.start <= range.end && range.end <= draft.len(),
                    "{draft}"
                );
                assert!(
                    draft.is_char_boundary(range.start) && draft.is_char_boundary(range.end),
                    "{draft}"
                );
            }
            if !draft.trim().is_empty() {
                assert!(!syntax.is_empty(), "{draft}");
            }
        }
    }
}

#[test]
fn completion_targets_only_field_literals_and_keeps_quotes_and_escapes_literal() {
    for (source, cursor, expected) in [
        ("@loader", 7, Some((CompletionField::Loader, ""))),
        ("@loader ", 8, Some((CompletionField::Loader, ""))),
        ("@loader faXX", 10, Some((CompletionField::Loader, "fa"))),
        ("@loader (forge | ", 17, Some((CompletionField::Loader, ""))),
        (
            "@version (1.20.1 | 1.2)",
            22,
            Some((CompletionField::Version, "1.2")),
        ),
        ("'@loader '", 9, None),
        (r"\@loader ", 9, None),
        ("@loader 'fa'", 11, None),
        ("@loader *", 9, None),
        ("@loader forge ", 14, None),
        ("@loader (forge) ", 16, None),
        ("@unknown ", 9, None),
    ] {
        let context = completion_context(source, cursor);
        assert_eq!(
            context
                .as_ref()
                .map(|context| (context.field, context.prefix.as_str())),
            expected,
            "{source} at {cursor}"
        );
    }
    let source = "机械 @version (1.20.1 | 1.2x) @loader fabric";
    let cursor = source.find("1.2x").unwrap() + 3;
    let context = completion_context(source, cursor).unwrap();
    assert_eq!(&source[context.range], "1.2x");
}
