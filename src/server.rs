use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::textlint::{self, TextlintMessage, TextlintRunner};

pub struct Backend<R: TextlintRunner> {
    client: Client,
    runner: R,
    root_dir: OnceLock<PathBuf>,
    /// URI → (ファイル内容, Vec<TextlintMessage>) を保持。
    /// code_action で fix 情報を参照するために使う。
    state: DashMap<Url, (String, Vec<TextlintMessage>)>,
}

impl<R: TextlintRunner> Backend<R> {
    pub fn new(client: Client, runner: R) -> Self {
        Self {
            client,
            runner,
            root_dir: OnceLock::new(),
            state: DashMap::new(),
        }
    }

    async fn lint_and_publish(&self, uri: &Url, text: &str) {
        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(()) => return,
        };

        let work_dir = match self.root_dir.get() {
            Some(d) => d.clone(),
            None => match path.parent() {
                Some(p) => p.to_path_buf(),
                None => return,
            },
        };

        let results = match self.runner.run(&path, &work_dir).await {
            Ok(r) => r,
            Err(_) => return,
        };

        let messages: Vec<TextlintMessage> = results.into_iter().flat_map(|r| r.messages).collect();

        let diagnostics: Vec<Diagnostic> = messages
            .iter()
            .map(|msg| {
                let line = msg.line.saturating_sub(1);
                let col = msg.column.saturating_sub(1);
                Diagnostic {
                    range: Range {
                        start: Position::new(line, col),
                        end: Position::new(line, col),
                    },
                    severity: Some(match msg.severity {
                        1 => DiagnosticSeverity::WARNING,
                        _ => DiagnosticSeverity::ERROR,
                    }),
                    source: Some("textlint".to_string()),
                    code: Some(NumberOrString::String(msg.rule_id.clone())),
                    message: msg.message.clone(),
                    ..Default::default()
                }
            })
            .collect();

        self.state.insert(uri.clone(), (text.to_string(), messages));
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl<R: TextlintRunner> LanguageServer for Backend<R> {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(root_uri) = params.root_uri {
            if let Ok(path) = root_uri.to_file_path() {
                let _ = self.root_dir.set(path);
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.lint_and_publish(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // TextDocumentSyncKind::FULL なので content_changes[0] に全文が入る
        if let Some(change) = params.content_changes.into_iter().next() {
            if let Some(mut entry) = self.state.get_mut(&uri) {
                entry.0 = change.text;
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = if let Some(text) = params.text {
            text
        } else if let Some(entry) = self.state.get(&uri) {
            entry.0.clone()
        } else {
            return;
        };
        self.lint_and_publish(&uri, &text).await;
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let entry = match self.state.get(uri) {
            Some(e) => e,
            None => return Ok(None),
        };
        let (text, messages) = entry.value();
        let request_range = params.range;

        let mut actions = Vec::new();

        for msg in messages {
            let fix = match &msg.fix {
                Some(f) => f,
                None => continue,
            };

            let msg_line = msg.line.saturating_sub(1);
            if msg_line < request_range.start.line || msg_line > request_range.end.line {
                continue;
            }

            let start = textlint::utf16_offset_to_position(text, fix.range[0]);
            let end = textlint::utf16_offset_to_position(text, fix.range[1]);

            let edit_range = Range {
                start: Position::new(start.line, start.character),
                end: Position::new(end.line, end.character),
            };

            let mut changes = HashMap::new();
            changes.insert(
                uri.clone(),
                vec![TextEdit {
                    range: edit_range,
                    new_text: fix.text.clone(),
                }],
            );

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Fix: {} ({})", msg.message, msg.rule_id),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textlint::{FixCommand, TextlintResult};
    use std::path::Path;
    use std::sync::Mutex;
    use tower_lsp::LspService;

    struct MockRunner {
        results: Mutex<Vec<TextlintResult>>,
    }

    impl MockRunner {
        fn new(results: Vec<TextlintResult>) -> Self {
            Self {
                results: Mutex::new(results),
            }
        }
    }

    #[async_trait::async_trait]
    impl TextlintRunner for MockRunner {
        async fn run(
            &self,
            _file_path: &Path,
            _work_dir: &Path,
        ) -> anyhow::Result<Vec<TextlintResult>> {
            let results = self.results.lock().unwrap().clone();
            Ok(results)
        }
    }

    #[tokio::test]
    async fn initialize_returns_expected_capabilities() {
        let runner = MockRunner::new(vec![]);
        let (service, _) = LspService::new(|client| Backend::new(client, runner));

        let params = InitializeParams::default();
        let result = service.inner().initialize(params).await.unwrap();

        assert!(result.capabilities.code_action_provider.is_some());
        assert!(result.capabilities.text_document_sync.is_some());
    }

    #[tokio::test]
    async fn code_action_returns_quickfix_for_fixable_message() {
        let results = vec![TextlintResult {
            file_path: "./test.md".to_string(),
            messages: vec![TextlintMessage {
                rule_id: "no-doubled-joshi".to_string(),
                message: "助詞の重複".to_string(),
                line: 1,
                column: 5,
                severity: 2,
                fix: Some(FixCommand {
                    range: [6, 7],
                    text: "けれど".to_string(),
                }),
            }],
        }];

        let runner = MockRunner::new(results);
        let (service, _) = LspService::new(|client| Backend::new(client, runner));
        let backend = service.inner();

        // simulate lint state
        let uri = Url::from_file_path("/tmp/test.md").unwrap();
        let text = "test がが error";
        backend.state.insert(
            uri.clone(),
            (
                text.to_string(),
                vec![TextlintMessage {
                    rule_id: "no-doubled-joshi".to_string(),
                    message: "助詞の重複".to_string(),
                    line: 1,
                    column: 5,
                    severity: 2,
                    fix: Some(FixCommand {
                        range: [6, 7],
                        text: "けれど".to_string(),
                    }),
                }],
            ),
        );

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier::new(uri),
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 10),
            },
            context: CodeActionContext::default(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = backend.code_action(params).await.unwrap();
        let actions = result.unwrap();
        assert_eq!(actions.len(), 1);

        if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
            assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
            assert!(action.title.contains("no-doubled-joshi"));
        } else {
            panic!("expected CodeAction");
        }
    }

    #[tokio::test]
    async fn code_action_returns_none_for_no_fix() {
        let runner = MockRunner::new(vec![]);
        let (service, _) = LspService::new(|client| Backend::new(client, runner));
        let backend = service.inner();

        let uri = Url::from_file_path("/tmp/test.md").unwrap();
        backend.state.insert(
            uri.clone(),
            (
                "text".to_string(),
                vec![TextlintMessage {
                    rule_id: "max-ten".to_string(),
                    message: "読点が多い".to_string(),
                    line: 1,
                    column: 1,
                    severity: 1,
                    fix: None,
                }],
            ),
        );

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier::new(uri),
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 5),
            },
            context: CodeActionContext::default(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = backend.code_action(params).await.unwrap();
        assert!(result.is_none());
    }
}
