use std::path::Path;

use serde::Deserialize;

/// textlint を実行して結果を返すトレイト。テスト時にモック可能。
#[async_trait::async_trait]
pub trait TextlintRunner: Send + Sync + 'static {
    async fn run(&self, file_path: &Path, work_dir: &Path) -> anyhow::Result<Vec<TextlintResult>>;
}

/// 実際に textlint コマンドを呼び出す実装。
pub struct CommandRunner;

#[async_trait::async_trait]
impl TextlintRunner for CommandRunner {
    async fn run(&self, file_path: &Path, work_dir: &Path) -> anyhow::Result<Vec<TextlintResult>> {
        let output = tokio::process::Command::new("textlint")
            .args(["--format", "json"])
            .arg(file_path)
            .current_dir(work_dir)
            .output()
            .await?;

        // textlint は lint エラーがあると exit code 1 を返すが、stdout に JSON が出る
        let stdout = String::from_utf8(output.stdout)?;
        if stdout.is_empty() {
            return Ok(vec![]);
        }
        let results: Vec<TextlintResult> = serde_json::from_str(&stdout)?;
        Ok(results)
    }
}

/// LSP の Position.character で使うエンコーディング。
/// クライアントとの negotiation 結果に基づいて選択する。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}

/// LSP の Position 相当。line / character ともに 0-based。
#[derive(Debug, PartialEq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// textlint の文字オフセット（UTF-16 コードユニット単位）を
/// 指定されたエンコーディングの Position に変換する。
pub fn offset_to_position(text: &str, offset: usize, encoding: PositionEncoding) -> Position {
    let mut line = 0u32;
    let mut utf16_count = 0usize;
    let mut line_start_utf16 = 0usize;
    let mut line_start_byte = 0usize;
    let mut line_start_chars = 0usize;
    let mut byte_count = 0usize;
    let mut char_count = 0usize;

    for ch in text.chars() {
        if utf16_count == offset {
            break;
        }
        let utf16_len = ch.len_utf16();
        let utf8_len = ch.len_utf8();
        if ch == '\n' {
            line += 1;
            line_start_utf16 = utf16_count + utf16_len;
            line_start_byte = byte_count + utf8_len;
            line_start_chars = char_count + 1;
        }
        utf16_count += utf16_len;
        byte_count += utf8_len;
        char_count += 1;
    }

    let character = match encoding {
        PositionEncoding::Utf8 => (byte_count - line_start_byte) as u32,
        PositionEncoding::Utf16 => (utf16_count - line_start_utf16) as u32,
        PositionEncoding::Utf32 => (char_count - line_start_chars) as u32,
    };

    Position { line, character }
}

/// textlint の column (1-based, UTF-16 コードユニット) を
/// 指定されたエンコーディングの character offset (0-based) に変換する。
pub fn textlint_column_to_character(
    text: &str,
    line_0based: u32,
    column_1based: u32,
    encoding: PositionEncoding,
) -> u32 {
    if encoding == PositionEncoding::Utf16 {
        return column_1based.saturating_sub(1);
    }

    // 対象行の先頭バイトインデックスを探す
    let line_start_byte = if line_0based == 0 {
        0
    } else {
        let mut remaining = line_0based;
        text.char_indices()
            .find_map(|(i, ch)| {
                if ch == '\n' {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(i + ch.len_utf8());
                    }
                }
                None
            })
            .unwrap_or(text.len())
    };

    // 行先頭から column_1based - 1 個の UTF-16 code units を歩いて
    // 指定エンコーディングでのオフセットを計算する
    let target_utf16 = column_1based.saturating_sub(1) as usize;
    let mut utf16_walked = 0usize;
    let mut result = 0u32;

    for ch in text[line_start_byte..].chars() {
        if utf16_walked >= target_utf16 || ch == '\n' {
            break;
        }
        match encoding {
            PositionEncoding::Utf8 => result += ch.len_utf8() as u32,
            PositionEncoding::Utf32 => result += 1,
            PositionEncoding::Utf16 => unreachable!(),
        }
        utf16_walked += ch.len_utf16();
    }

    result
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TextlintResult {
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub messages: Vec<TextlintMessage>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TextlintMessage {
    #[serde(rename = "ruleId")]
    pub rule_id: String,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub severity: u32,
    pub fix: Option<FixCommand>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct FixCommand {
    pub range: [usize; 2],
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_single_message_with_fix() {
        let json = r#"[
          {
            "filePath": "./README.md",
            "messages": [
              {
                "type": "lint",
                "ruleId": "no-doubled-joshi",
                "message": "一文に二回以上利用されている助詞 \"が\" がみつかりました。",
                "line": 3,
                "column": 12,
                "severity": 2,
                "fix": {
                  "range": [24, 27],
                  "text": "けれど"
                }
              }
            ]
          }
        ]"#;

        let results: Vec<TextlintResult> = serde_json::from_str(json).unwrap();
        assert_eq!(results.len(), 1);

        let result = &results[0];
        assert_eq!(result.file_path, "./README.md");
        assert_eq!(result.messages.len(), 1);

        let msg = &result.messages[0];
        assert_eq!(msg.rule_id, "no-doubled-joshi");
        assert_eq!(msg.line, 3);
        assert_eq!(msg.column, 12);
        assert_eq!(msg.severity, 2);

        let fix = msg.fix.as_ref().unwrap();
        assert_eq!(fix.range, [24, 27]);
        assert_eq!(fix.text, "けれど");
    }

    #[test]
    fn deserialize_message_without_fix() {
        let json = r#"[
          {
            "filePath": "./doc.md",
            "messages": [
              {
                "type": "lint",
                "ruleId": "max-ten",
                "message": "一つの文で\"、\"を3つ以上使用しています。",
                "line": 1,
                "column": 1,
                "severity": 1
              }
            ]
          }
        ]"#;

        let results: Vec<TextlintResult> = serde_json::from_str(json).unwrap();
        let msg = &results[0].messages[0];
        assert_eq!(msg.rule_id, "max-ten");
        assert_eq!(msg.severity, 1);
        assert!(msg.fix.is_none());
    }

    #[test]
    fn deserialize_multiple_messages_across_files() {
        let json = r#"[
          {
            "filePath": "./a.md",
            "messages": [
              {
                "type": "lint",
                "ruleId": "rule-a",
                "message": "error a",
                "line": 1,
                "column": 1,
                "severity": 2,
                "fix": { "range": [0, 1], "text": "A" }
              },
              {
                "type": "lint",
                "ruleId": "rule-b",
                "message": "error b",
                "line": 2,
                "column": 5,
                "severity": 1
              }
            ]
          },
          {
            "filePath": "./b.md",
            "messages": [
              {
                "type": "lint",
                "ruleId": "rule-c",
                "message": "error c",
                "line": 10,
                "column": 3,
                "severity": 2,
                "fix": { "range": [100, 110], "text": "fixed" }
              }
            ]
          }
        ]"#;

        let results: Vec<TextlintResult> = serde_json::from_str(json).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].messages.len(), 2);
        assert_eq!(results[1].messages.len(), 1);
        assert!(results[0].messages[0].fix.is_some());
        assert!(results[0].messages[1].fix.is_none());
        assert_eq!(
            results[1].messages[0].fix.as_ref().unwrap().range,
            [100, 110]
        );
    }

    #[test]
    fn deserialize_empty_messages() {
        let json = r#"[{"filePath": "./clean.md", "messages": []}]"#;

        let results: Vec<TextlintResult> = serde_json::from_str(json).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].messages.is_empty());
    }

    #[test]
    fn offset_to_position_ascii_single_line() {
        let text = "hello world";
        // offset 6 = 'w' → line 0, character 6 (同じ for all encodings)
        for enc in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            let pos = offset_to_position(text, 6, enc);
            assert_eq!(pos.line, 0);
            assert_eq!(pos.character, 6);
        }
    }

    #[test]
    fn offset_to_position_ascii_multi_line() {
        let text = "hello\nworld\nfoo";
        // ASCII なので全エンコーディングで同じ結果
        for enc in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            let pos = offset_to_position(text, 6, enc);
            assert_eq!(pos.line, 1, "enc={enc:?}");
            assert_eq!(pos.character, 0, "enc={enc:?}");

            let pos = offset_to_position(text, 11, enc);
            assert_eq!(pos.line, 1, "enc={enc:?}");
            assert_eq!(pos.character, 5, "enc={enc:?}");

            let pos = offset_to_position(text, 12, enc);
            assert_eq!(pos.line, 2, "enc={enc:?}");
            assert_eq!(pos.character, 0, "enc={enc:?}");
        }
    }

    #[test]
    fn offset_to_position_japanese_utf16() {
        let text = "あいう";
        let pos = offset_to_position(text, 1, PositionEncoding::Utf16);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 1);
    }

    #[test]
    fn offset_to_position_japanese_utf8() {
        // 'あ' = 3 bytes UTF-8
        let text = "あいう";
        // offset 1 (UTF-16) = 'い' → UTF-8 byte offset from line start = 3
        let pos = offset_to_position(text, 1, PositionEncoding::Utf8);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);

        // offset 2 (UTF-16) = 'う' → UTF-8 byte offset = 6
        let pos = offset_to_position(text, 2, PositionEncoding::Utf8);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 6);
    }

    #[test]
    fn offset_to_position_japanese_utf32() {
        let text = "あいう";
        // UTF-32 は code point 数 = character 数
        let pos = offset_to_position(text, 1, PositionEncoding::Utf32);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 1);
    }

    #[test]
    fn offset_to_position_japanese_multi_line_utf8() {
        let text = "あいう\nかきく";
        // offset 4 (UTF-16) = 'か' (2行目先頭) → UTF-8: line 1, char 0
        let pos = offset_to_position(text, 4, PositionEncoding::Utf8);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        // offset 5 (UTF-16) = 'き' → UTF-8: line 1, char 3
        let pos = offset_to_position(text, 5, PositionEncoding::Utf8);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 3);
    }

    #[test]
    fn offset_to_position_surrogate_pair() {
        // '𠮷' (U+20BB7) = 4 bytes UTF-8, 2 UTF-16 code units
        let text = "a𠮷b";
        // UTF-16 encoding
        let pos = offset_to_position(text, 3, PositionEncoding::Utf16);
        assert_eq!(pos.character, 3); // 'a'(1) + '𠮷'(2) = 3

        // UTF-8 encoding
        let pos = offset_to_position(text, 3, PositionEncoding::Utf8);
        assert_eq!(pos.character, 5); // 'a'(1) + '𠮷'(4) = 5

        // UTF-32 encoding
        let pos = offset_to_position(text, 3, PositionEncoding::Utf32);
        assert_eq!(pos.character, 2); // 'a'(1) + '𠮷'(1) = 2
    }

    #[test]
    fn textlint_column_to_character_utf16() {
        let text = "あいう";
        // column 2 (1-based) → character 1 (0-based) in UTF-16
        assert_eq!(
            textlint_column_to_character(text, 0, 2, PositionEncoding::Utf16),
            1
        );
    }

    #[test]
    fn textlint_column_to_character_utf8() {
        let text = "あいう";
        // column 2 (1-based) → 'い' is at byte offset 3
        assert_eq!(
            textlint_column_to_character(text, 0, 2, PositionEncoding::Utf8),
            3
        );
    }

    #[test]
    fn textlint_column_to_character_utf32() {
        let text = "あいう";
        // column 2 (1-based) → code point 1
        assert_eq!(
            textlint_column_to_character(text, 0, 2, PositionEncoding::Utf32),
            1
        );
    }

    #[test]
    fn textlint_column_to_character_second_line() {
        let text = "abc\nあいう";
        // line 1, column 2 (1-based) → 'い'
        assert_eq!(
            textlint_column_to_character(text, 1, 2, PositionEncoding::Utf8),
            3
        );
        assert_eq!(
            textlint_column_to_character(text, 1, 2, PositionEncoding::Utf16),
            1
        );
        assert_eq!(
            textlint_column_to_character(text, 1, 2, PositionEncoding::Utf32),
            1
        );
    }
}
