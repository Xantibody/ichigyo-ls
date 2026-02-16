# ichigyo-ls

textlint の診断結果を LSP Diagnostic + Code Action (QuickFix) として提供する Language Server。

## Context

textlint は強力な自然言語リンターだが、エディタ統合の手段が限られている。
現状は efm-langserver 経由で textlint を呼び出す構成が一般的だが、以下の問題がある:

- efm-langserver は汎用ツールのため、textlint の `fix` 情報を Code Action に変換できない
- 別途スクリプトで fix を適用する必要があり、ワークフローが煩雑

ichigyo-ls は textlint の JSON 出力を直接パースし、LSP の Diagnostic と Code Action (QuickFix) を提供する専用サーバーとして、この問題を解決する。

## textlint JSON 出力形式

textlint を `--format json` で実行すると以下の構造が得られる:

```json
[
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
]
```

重要なポイント:
- `line` / `column` は 1-based
- `fix.range` は 0-based の文字オフセット（JavaScript 文字列インデックス = UTF-16 コードユニット）
- `fix` フィールドはオプショナル（fix 不可能なルールでは存在しない）
- `severity`: `1` = warning, `2` = error

## ライブラリ選定

| crate | 用途 |
|-------|------|
| `tower-lsp` | LSP サーバーフレームワーク（Backend trait 実装） |
| `lsp-types` | LSP プロトコルの型定義（`tower-lsp` が re-export） |
| `tokio` | async ランタイム |
| `serde` / `serde_json` | textlint JSON 出力のデシリアライズ |

## ファイル構成

```
ichigyo-ls/
├── Cargo.toml
├── src/
│   ├── main.rs          # エントリポイント、Server 起動
│   └── textlint.rs      # textlint JSON パーサー、型定義
├── flake.nix            # Nix 開発環境
├── justfile             # タスクランナー
└── cargo.toml.example   # clippy lint 設定
```

## LSP サーバー設計

### Backend 構造体

```rust
struct Backend {
    client: Client,
    /// URI → Vec<TextlintMessage> のマッピング
    /// code_action で fix 情報を参照するために保持
    diagnostics_map: DashMap<Url, Vec<TextlintMessage>>,
}
```

### 使用する LSP メソッド

| メソッド | 方向 | 用途 |
|---------|------|------|
| `initialize` | Client → Server | capabilities 宣言 |
| `textDocument/didOpen` | Client → Server | textlint 実行 → Diagnostic 発行 |
| `textDocument/didSave` | Client → Server | textlint 再実行 → Diagnostic 更新 |
| `textDocument/codeAction` | Client → Server | fix 付き Diagnostic に QuickFix を返す |
| `publishDiagnostics` | Server → Client | 診断結果をエディタに送信 |

### initialize で宣言する capabilities

```rust
InitializeResult {
    capabilities: ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..Default::default()
    },
    ..Default::default()
}
```

### textlint 実行フロー

```
didOpen / didSave
  → tokio::process::Command で textlint --format json <file> を実行
  → stdout を TextlintResult としてパース
  → messages を diagnostics_map に保存
  → publishDiagnostics で Diagnostic を発行
```

### code_action フロー

```
textDocument/codeAction リクエスト受信
  → diagnostics_map から該当ファイルの messages を取得
  → リクエストの range と重なる Diagnostic を絞り込み
  → fix フィールドを持つ message に対して:
    → fix.range (UTF-16 オフセット) を Position (line, character) に変換
    → WorkspaceEdit を構築
    → CodeAction { kind: QuickFix, edit: WorkspaceEdit } を返す
```

### UTF-16 オフセット → Position 変換

textlint の `fix.range` は JavaScript 文字列インデックス（UTF-16 コードユニット単位）。
LSP の Position も `character` が UTF-16 コードユニット単位なので、行内オフセットはそのまま使える。

変換ロジック:

```
fn utf16_offset_to_position(text: &str, offset: usize) -> Position:
  1. text を先頭から UTF-16 コードユニットを数えながら走査
  2. 改行文字で line をインクリメントし、行頭の UTF-16 オフセットを記録
  3. offset に到達したら、行頭からの差分を character とする
  4. Position { line, character } を返す
```

## 実装順序（TDD）

### Phase 1: textlint JSON パーサー

1. `TextlintResult` / `TextlintMessage` / `FixCommand` の型定義
2. テスト: JSON 文字列 → 構造体デシリアライズ
3. テスト: fix なしメッセージのハンドリング
4. テスト: 複数メッセージ / 複数ファイルのパース

### Phase 2: バイトオフセット変換

1. `byte_offset_to_position` 関数の実装
2. テスト: ASCII テキストでの変換
3. テスト: マルチバイト文字（日本語）での変換
4. テスト: UTF-16 サロゲートペアが必要な文字での変換

### Phase 3: LSP ハンドラー

1. Backend 構造体と initialize の実装
2. didOpen / didSave → textlint 実行 → publishDiagnostics
3. code_action → QuickFix 生成
4. 統合テスト: tower-lsp の test utilities を使用

### Phase 4: 動作確認

1. `cargo build --release`
2. Neovim の `lspconfig` で ichigyo-ls を設定
3. Markdown ファイルで Diagnostic 表示と QuickFix 適用を確認

## 検証方法

```bash
# ユニットテスト
cargo test

# clippy
cargo clippy

# フォーマットチェック
cargo fmt --check

# 手動検証: textlint JSON をパイプで渡す
echo '[{"filePath":"test.md","messages":[{"type":"lint","ruleId":"test","message":"test","line":1,"column":1,"severity":2,"fix":{"range":[0,4],"text":"fixed"}}]}]' | cargo run -- --test-parse
```
