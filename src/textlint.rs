use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct TextlintResult {
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub messages: Vec<TextlintMessage>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct TextlintMessage {
    #[serde(rename = "ruleId")]
    pub rule_id: String,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub severity: u32,
    pub fix: Option<FixCommand>,
}

#[derive(Debug, Deserialize, PartialEq)]
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
        assert_eq!(results[1].messages[0].fix.as_ref().unwrap().range, [100, 110]);
    }

    #[test]
    fn deserialize_empty_messages() {
        let json = r#"[{"filePath": "./clean.md", "messages": []}]"#;

        let results: Vec<TextlintResult> = serde_json::from_str(json).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].messages.is_empty());
    }
}
