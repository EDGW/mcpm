//! Boolean, escaping, diagnostic and Unicode regressions for the shared filter language.

use super::*;

fn document(name: &str, id: &str, file: &str, loaders: &str) -> Document {
    let text = ["", name, id, file, loaders].map(str::to_owned);
    let values = text
        .each_ref()
        .map(|value| std::iter::once(0..value.len()).collect());
    Document { text, values }
}

fn matches(source: &str, doc: &Document) -> bool {
    let filter: Filter = source
        .parse()
        .unwrap_or_else(|error| panic!("{source}: {error}"));
    filter
        .expression
        .unwrap()
        .is_none_or(|expr| expr.matches(doc))
}

#[test]
fn boolean_precedence_explicit_and_implicit_and_and_field_scope() {
    let doc = document(
        "Fabric API 中文",
        "fabric_api",
        "mods/user@host.jar",
        "Fabric",
    );
    for source in [
        "",
        "Fabric API",
        "Fabric & API",
        "@name 'Fabric API' & @modid =fabric_api",
        "@modid fabric @name ~'^Fabric.*API'",
        "missing | fabric api",
        "(missing | fabric) & api",
        "@name Fabric fabric_api",
        "@loaders =fabric",
        "@file user\\@HOST.jar",
    ] {
        assert!(matches(source, &doc), "{source}");
    }
    for source in [
        "@name =fabric",
        "@name fabric_api",
        "@file 'Fabric API'",
        "missing & fabric | absent",
        "(missing | fabric) & absent",
        "@name =''",
        "@name ~'(?-i)^fabric'",
    ] {
        assert!(!matches(source, &doc), "{source}");
    }
    assert!(matches("fabric | missing absent", &doc));
    assert!(!matches("(fabric | missing) absent", &doc));
    assert!(matches("@modid =''", &document("empty id", "", "x", "")));
}

#[test]
fn every_syntax_character_can_be_literal_and_regex_escapes_are_preserved() {
    let file = "a(b)&c|d@e=f~g\"h'i\\j k";
    let doc = document("He said \"hello\"", "@name", file, "");
    assert!(matches(r#"@file a\(b\)\&c\|d\@e\=f\~g\"h\'i\\j\ k"#, &doc));
    assert!(matches(r#"@name "He said \"hello\"""#, &doc));
    assert!(matches("'@name'", &doc));
    assert!(matches("@file \"a(b)&c|d@e=f~g\\\"h'i\\\\j k\"", &doc));
    assert!(matches(r#"@modid ~'^\d+$'"#, &document("", "123", "", "")));
    assert!(matches(
        r#"@file C:\mods\file.jar"#,
        &document("", "", r"C:\mods\file.jar", "")
    ));
    assert!(matches(r"@file foo\q", &document("", "", r"foo\q", "")));
    assert!(matches(r"\=value", &document("=value", "", "", "")));
}

#[test]
fn field_groups_inherit_scope_without_leaking_and_explicit_inner_fields_override_it() {
    let doc = document("Fabric", "fabric_api", "forge.jar", "Forge");
    assert!(matches("@loaders (Forge | Fabric)", &doc));
    assert!(matches("@loaders (=Forge | =Fabric)", &doc));
    assert!(!matches("@loaders (Fabric | NeoForge)", &doc));
    assert!(matches("@loaders Forge | Fabric", &doc));
    assert!(!matches("@loaders (Fabric | NeoForge) @name Fabric", &doc));
    assert!(matches("@loaders (Fabric | @name =Fabric)", &doc));
    assert!(matches(
        "@loaders ((=Forge | =NeoForge) & ~'^For') fabric_api",
        &doc
    ));
    // The OR's second branch is unscoped, so Name may satisfy it.
    assert!(matches("@loaders missing | Fabric", &doc));
    assert!(!matches("@loaders (missing | Fabric)", &doc));
}

#[test]
fn invalid_expressions_report_original_character_columns_and_keep_syntax_spans() {
    for source in [
        "a |",
        "a &",
        "a && b",
        "a || b",
        "()",
        "(a | b",
        "a)",
        "@name",
        "@index x",
        "@ name x",
        "~word",
        "~'['",
        "'unfinished",
        "a\"b\"",
    ] {
        let parsed = Filter::parse(source);
        assert!(parsed.invalid(), "{source}");
        assert!(source.parse::<Filter>().is_err(), "{source}");
        assert!(!parsed.syntax.is_empty());
        for span in parsed.syntax {
            assert!(source.get(span.range).is_some());
        }
    }
    let parsed = Filter::parse("中文 & @unknown value");
    let error = parsed.error().unwrap();
    assert_eq!(&parsed.source[error.range.clone()], "@unknown");
    assert!(error.to_string().contains("column 6"));
    let nested = format!("{}a{}", "(".repeat(65), ")".repeat(65));
    assert!(
        nested
            .parse::<Filter>()
            .unwrap_err()
            .to_string()
            .contains("maximum 64")
    );
}

#[test]
fn only_successful_branches_highlight_and_unicode_ranges_stay_in_original_text() {
    let doc = document(
        "Fabric API préİ 中文 ΟΣ",
        "fabric_api",
        "other.jar",
        "Fabric",
    );
    let filter: Filter = "(@name Fabric & @file missing) | @name API | @name i | @name ος"
        .parse()
        .unwrap();
    let expr = filter.expression.unwrap().unwrap();
    let mut ranges = Vec::new();
    expr.ranges(&doc, Column::Name, &mut ranges);
    let hits: Vec<_> = ranges
        .iter()
        .map(|range| &doc.text[1][range.clone()])
        .collect();
    assert!(hits.contains(&"API"));
    assert!(hits.contains(&"İ"));
    assert!(hits.contains(&"ΟΣ"));
    assert!(!hits.contains(&"Fabric"));
    let matcher = Matcher::Regex(regex::Regex::new("中文|$").unwrap());
    assert_eq!(
        matcher.ranges("中文"),
        std::iter::once(0..6).collect::<Vec<_>>()
    );
    assert!(Matcher::Regex(regex::Regex::new("^").unwrap()).matches("anything"));
    assert!(
        Matcher::Regex(regex::Regex::new("^").unwrap())
            .ranges("anything")
            .is_empty()
    );
}
