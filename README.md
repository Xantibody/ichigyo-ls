# ichigyo-ls

textlint を wrap して Diagnostics と Code Action (QuickFix) を提供する Language Server。

## Features

- **Diagnostics** — `textDocument/didOpen` / `textDocument/didSave` で textlint を実行し、診断結果を publish
- **QuickFix Code Actions** — textlint の `fix` 情報から `textDocument/codeAction` で TextEdit を生成
- **Position encoding negotiation** — クライアントがサポートする position encoding (UTF-16 / UTF-32 / UTF-8) をネゴシエーション

## Requirements

- [textlint](https://textlint.github.io/) がインストール済みで `PATH` に存在すること

## Install

### Nix flake

```bash
# ビルド
nix build github:ryuaizawa/ichigyo-ls

# overlay として利用
# flake.nix の inputs に追加し、packages.default を参照
```

### cargo install

```bash
cargo install --git https://github.com/ryuaizawa/ichigyo-ls.git
```

## Neovim 設定例 (0.11+)

```lua
vim.lsp.config("ichigyo_ls", {
  cmd = { "ichigyo-ls" },
  filetypes = { "markdown" },
  root_markers = { ".textlintrc", ".textlintrc.json" },
})

vim.lsp.enable("ichigyo_ls")
```

## 仕組み

1. `didOpen` / `didSave` を受け取ると `textlint --format json <file>` を実行
2. JSON 出力をパースし、`publishDiagnostics` で診断結果をエディタに送信
3. `codeAction` リクエスト時、`fix` フィールドを持つメッセージから `fix.range` を Position に変換し TextEdit を生成

## Development

```bash
# Nix 開発シェル (textlint 込み)
nix develop

# テスト
cargo test

# 静的解析
cargo clippy

# フォーマット
cargo fmt --check
```

## Acknowledgements

- [textlint](https://github.com/textlint/textlint) — pluggable natural language linter
- [efm-langserver](https://github.com/mattn/efm-langserver) — general purpose language server
