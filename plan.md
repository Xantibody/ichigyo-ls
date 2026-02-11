# textlint-lsp: 別レポとして作成

## やること

1. `gh repo create Xantibody/textlint-lsp --public` → clone
2. README.md に実行計画書を記載 → initial commit & push
3. dotfiles側の変更は textlint-lsp 実装完了後に別途実施

## README.md の内容（実行計画書）

以下の設計内容をそのままREADME.mdとして記載:

- Context（なぜ作るのか）
- textlint JSONのfix情報の仕組み
- ライブラリ選定（tower-lsp, lsp-types, tokio, serde）
- ファイル構成（Cargo.toml, src/main.rs, src/textlint.rs）
- LSPサーバー設計（Backend構造体, LSPメソッド, code_actionフロー, index→Position変換）
- 実装順序（TDD: パーサーテスト → LSPハンドラー → 動作確認）
- 検証方法

## dotfiles側の変更（後日）

| ファイル | 変更内容 |
|---------|---------|
| `flake.nix` | input追加: `textlint-lsp.url = "github:Xantibody/textlint-lsp"` |
| `hosts/overlays.nix` | textlint-lspパッケージ参照追加 |
| `nixcats/default.nix` | lspsAndRuntimeDepsにtextlint-lsp追加、efm-langserver削除 |
| `lsp.lua` | efm設定をtextlint-lsp設定に置換 |
| `neovim.nix` | efm-langserver削除 |
| `file.nix` | efm-langserver symlink削除 |
| `configs/efm-langserver/` | 削除 |
| `scripts/textlint-range-fix/` | 削除 |
