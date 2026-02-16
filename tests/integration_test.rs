use std::path::Path;

use ichigyo_ls::textlint::{self, CommandRunner, PositionEncoding, TextlintRunner};

const FIXTURE: &str = include_str!("fixtures/sample.md");

fn fixture_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sample.md"
    ))
}

fn work_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn textlint_parses_fixture() {
    let runner = CommandRunner;
    let results = runner.run(fixture_path(), work_dir()).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(
        !results[0].messages.is_empty(),
        "textlint should find errors in fixture"
    );

    // "ふたつ => 2つ" ルールが含まれているか
    let futatsu = results[0]
        .messages
        .iter()
        .find(|m| m.message.contains("ふたつ"))
        .expect("should find 'ふたつ' error");

    assert_eq!(futatsu.rule_id, "prh");
    let fix = futatsu.fix.as_ref().expect("should have fix");

    // fix.range でスライスした文字列が "ふたつ" であることを検証
    let runes: Vec<char> = FIXTURE.chars().collect();
    let sliced: String = runes[fix.range[0]..fix.range[1]].iter().collect();
    assert_eq!(
        sliced, "ふたつ",
        "fix.range should point to 'ふたつ' in fixture"
    );
    assert_eq!(fix.text, "2つ");
}

#[tokio::test]
async fn fix_range_converts_to_correct_position() {
    let runner = CommandRunner;
    let results = runner.run(fixture_path(), work_dir()).await.unwrap();

    let futatsu = results[0]
        .messages
        .iter()
        .find(|m| m.message.contains("ふたつ"))
        .unwrap();
    let fix = futatsu.fix.as_ref().unwrap();

    let start = textlint::offset_to_position(FIXTURE, fix.range[0], PositionEncoding::Utf16);
    let end = textlint::offset_to_position(FIXTURE, fix.range[1], PositionEncoding::Utf16);

    // textlint は line:3 (1-based) → 0-based で line 2
    assert_eq!(
        start.line,
        futatsu.line - 1,
        "start line should match textlint line (0-based)"
    );
    assert_eq!(end.line, futatsu.line - 1, "end should be on same line");

    // column:1 (1-based) → 0-based で character 0
    assert_eq!(
        start.character,
        futatsu.column - 1,
        "start character should match textlint column (0-based)"
    );

    // "ふたつ" は 3 文字なので end.character = start.character + 3
    assert_eq!(end.character, start.character + 3);
}

#[tokio::test]
async fn applying_text_edit_produces_correct_result() {
    let runner = CommandRunner;
    let results = runner.run(fixture_path(), work_dir()).await.unwrap();

    let futatsu = results[0]
        .messages
        .iter()
        .find(|m| m.message.contains("ふたつ"))
        .unwrap();
    let fix = futatsu.fix.as_ref().unwrap();

    // fix.range を使って手動でテキストを置換
    let runes: Vec<char> = FIXTURE.chars().collect();
    let mut result_text = String::new();
    result_text.extend(&runes[..fix.range[0]]);
    result_text.push_str(&fix.text);
    result_text.extend(&runes[fix.range[1]..]);

    // "ふたつ" が "2つ" に置換されているか
    assert!(
        result_text.contains("2つの項目がある"),
        "should replace ふたつ with 2つ"
    );
    assert!(
        !result_text.contains("ふたつ"),
        "should not contain ふたつ anymore"
    );
}
