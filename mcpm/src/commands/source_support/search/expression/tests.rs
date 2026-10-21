//! Expression regressions for precedence, exact alternatives, quoting and error columns.

use kako_mods_lib::source::{SearchQuery, SearchSort};

use super::Expression;

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
