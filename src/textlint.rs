use std::path::Path;

use serde::Deserialize;

/// textlint を実行して結果を返すトレイト。テスト時にモック可能。
#[async_trait::async_trait]
pub trait TextlintRunner: Send + Sync + 'static {
    async fn run(&self, file_path: &Path) -> anyhow::Result<Vec<TextlintResult>>;
}

/// 実際に textlint コマンドを呼び出す実装。
pub struct CommandRunner;

#[async_trait::async_trait]
impl TextlintRunner for CommandRunner {
    async fn run(&self, file_path: &Path) -> anyhow::Result<Vec<TextlintResult>> {
        let output = tokio::process::Command::new("textlint")
            .args(["--format", "json"])
            .arg(file_path)
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

/// LSP の Position 相当。line / character ともに 0-based。
/// character は UTF-16 コードユニット単位。
#[derive(Debug, PartialEq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// UTF-8 バイトオフセットを LSP Position (line, character) に変換する。
/// character は UTF-16 コードユニット数。
pub fn byte_offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut line_start_byte = 0usize;

    for (i, b) in text.as_bytes().iter().enumerate() {
        if i == offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start_byte = i + 1;
        }
    }

    // 現在行の先頭から offset までの UTF-16 コードユニット数を算出
    let line_slice = &text[line_start_byte..offset];
    let character = line_slice.chars().map(|c| c.len_utf16() as u32).sum();

    Position { line, character }
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
    fn byte_offset_to_position_ascii_single_line() {
        let text = "hello world";
        // offset 6 = 'w' → line 0, character 6
        let pos = byte_offset_to_position(text, 6);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 6);
    }

    #[test]
    fn byte_offset_to_position_ascii_multi_line() {
        let text = "hello\nworld\nfoo";
        // offset 6 = 'w' (2行目先頭) → line 1, character 0
        let pos = byte_offset_to_position(text, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        // offset 11 = '\n' (2行目末尾) → line 1, character 5
        let pos = byte_offset_to_position(text, 11);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 5);

        // offset 12 = 'f' (3行目先頭) → line 2, character 0
        let pos = byte_offset_to_position(text, 12);
        assert_eq!(pos.line, 2);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn byte_offset_to_position_japanese() {
        // 'あ' = 3 bytes UTF-8, 1 code unit UTF-16
        let text = "あいう";
        // offset 0 = 'あ' → line 0, character 0
        let pos = byte_offset_to_position(text, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        // offset 3 = 'い' → line 0, character 1
        let pos = byte_offset_to_position(text, 3);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 1);

        // offset 6 = 'う' → line 0, character 2
        let pos = byte_offset_to_position(text, 6);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 2);
    }

    #[test]
    fn byte_offset_to_position_surrogate_pair() {
        // '𠮷' (U+20BB7) = 4 bytes UTF-8, 2 code units UTF-16
        let text = "a𠮷b";
        // offset 0 = 'a' → line 0, character 0
        let pos = byte_offset_to_position(text, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        // offset 1 = '𠮷' → line 0, character 1
        let pos = byte_offset_to_position(text, 1);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 1);

        // offset 5 = 'b' → line 0, character 3 (𠮷 は UTF-16 で 2 code units)
        let pos = byte_offset_to_position(text, 5);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);
    }
}
