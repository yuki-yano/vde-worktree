# vde-worktree

`vde-worktree` は、人間とコーディングエージェントの両方を想定した、安全な Git worktree 管理 CLI です。

利用できるコマンド名:

- `vde-worktree`
- `vw`（エイリアス）

英語版ドキュメント: `README.md`

## このツールで解決すること

- worktree をリポジトリ配下 `.worktree/` に統一配置
- `switch` を冪等にして、同じ指示を繰り返しても破綻しにくくする
- `del` / `gone` の破壊操作に安全ガードを入れる
- Agent 向けに安定した JSON 出力を提供
- hooks ベースで運用を拡張しやすくする

## 動作要件

- Node.js 22+
- pnpm 10+
- `fzf`（`cd` に必須）
- `gh`（PR merged 判定に任意）

## インストール / ビルド

グローバルインストール:

```bash
npm install -g vde-worktree
```

ローカルビルド:

```bash
pnpm install
pnpm run build
```

開発時の検証:

```bash
pnpm run ci
```

## クイックスタート

```bash
vw init
vw switch feature/foo
cd "$(vw cd)"
```

`vw cd` は選択した worktree の path を出力するコマンドです。親シェルのディレクトリは直接変更できません。

## シェル補完

コマンドから補完スクリプトを出力:

```bash
vw completion zsh
vw completion fish
```

デフォルトの配置先にインストール:

```bash
vw completion zsh --install
vw completion fish --install
```

カスタム配置先にインストール:

```bash
vw completion zsh --install --path ~/.zsh/completions/_vw
vw completion fish --install --path ~/.config/fish/completions/vw.fish
```

zsh は `fpath` に補完ディレクトリを追加して `compinit` を実行してください:

```bash
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit && compinit
```

## 管理ディレクトリ

`vw init` 実行後に次を管理します:

- `.worktree/`（worktree 実体）
- `.vde/worktree/hooks/`
- `.vde/worktree/logs/`
- `.vde/worktree/locks/`
- `.vde/worktree/state/`

また `.git/info/exclude` に管理対象エントリを冪等で追記します。

## 全体ルール

- 多くの書き込み系コマンドは `init` 実行済みが前提
- 書き込み時は内部の repo lock で排他制御
- `--json` 指定時、stdout は単一 JSON オブジェクトのみ
- ログや警告は stderr に出力
- 非TTYで unsafe 操作を行う場合は `--allow-unsafe` が必要

## グローバルオプション

- `--json`: 機械可読の単一 JSON 出力
- `--verbose`: 詳細ログ
- `--no-hooks`: 今回のみ hook 無効化（`--allow-unsafe` 必須）
- `--allow-unsafe`: unsafe 操作の明示同意
- `--hook-timeout-ms <ms>`: hook timeout 上書き
- `--lock-timeout-ms <ms>`: repo lock timeout 上書き

## コマンド詳細

### `init`

```bash
vw init
```

機能:

- `.worktree/` と `.vde/worktree/*` を作成
- `.git/info/exclude` に管理エントリ追加
- デフォルト hook テンプレートを作成

### `list`

```bash
vw list
vw list --json
```

機能:

- Git の porcelain 情報から worktree 一覧を取得
- branch/path/dirty/lock/merged/upstream を表示
- 対話ターミナルでは Catppuccin 風の ANSI 色で表示

### `status`

```bash
vw status
vw status feature/foo
vw status --json
```

機能:

- 対象 worktree 1件の状態を表示
- branch 指定なしなら現在 `cwd` から該当 worktree を解決

### `path`

```bash
vw path feature/foo
vw path feature/foo --json
```

機能:

- 指定 branch の絶対 worktree path を返す

### `new`

```bash
vw new
vw new feature/foo
```

機能:

- 新しい branch + worktree を `.worktree/` に作成
- branch 省略時は `wip-xxxxxx` を自動生成

### `switch`

```bash
vw switch feature/foo
```

機能:

- 指定 branch の worktree があれば再利用、なければ作成
- 冪等な branch 入口コマンド

### `mv`

```bash
vw mv feature/new-name
```

機能:

- 現在の非primary worktree の branch 名と path をリネーム
- detached HEAD では実行不可

### `del`

```bash
vw del
vw del feature/foo
vw del feature/foo --force-unmerged --allow-unpushed --allow-unsafe
```

機能:

- worktree と branch を安全に削除
- デフォルトで dirty / locked / unmerged(unknown含む) / unpushed(unknown含む) を拒否

主な解除フラグ:

- `--force-dirty`
- `--allow-unpushed`
- `--force-unmerged`
- `--force-locked`
- `--force`（上記を一括有効）

### `gone`

```bash
vw gone
vw gone --apply
vw gone --json
```

機能:

- 一括クリーンアップ候補の抽出/削除
- デフォルトは dry-run
- `--apply` で削除実行

### `get`

```bash
vw get origin/feature/foo
```

機能:

- remote branch を fetch
- ローカル追跡 branch がなければ作成
- worktree を作成/再利用

### `extract`

```bash
vw extract --current
vw extract --current --stash
```

機能:

- primary worktree の現在 branch を `.worktree/` 側へ切り出し
- primary を base branch に戻す
- dirty 状態で切り出す場合は `--stash` を使用

現状の制約:

- 実装は primary worktree の抽出フローが中心

### `absorb`

```bash
vw absorb feature/foo --allow-agent --allow-unsafe
vw absorb feature/foo --from feature/foo --keep-stash --allow-agent --allow-unsafe
```

機能:

- 非 primary worktree の変更（未コミット含む）を primary worktree に移す
- source worktree を stash し、primary で checkout 後に stash を apply する
- `--from` は vw 管理 worktree 名のみ指定可能（`.worktree/` プレフィックスは不可）

安全条件:

- primary が dirty なら拒否
- 非TTYでは `--allow-agent` と `--allow-unsafe` の両方が必要
- `--keep-stash` を付けると apply 後も stash を残す

### `unabsorb`

```bash
vw unabsorb feature/foo --allow-agent --allow-unsafe
vw unabsorb feature/foo --to feature/foo --keep-stash --allow-agent --allow-unsafe
```

機能:

- primary worktree の変更（未コミット含む）を非 primary worktree に戻す
- primary の変更を stash し、target worktree に stash を apply する
- `--to` は vw 管理 worktree 名のみ指定可能（`.worktree/` プレフィックスは不可）

安全条件:

- primary worktree が対象 branch 上である必要がある
- primary が clean なら拒否
- target worktree が dirty なら拒否
- 非TTYでは `--allow-agent` と `--allow-unsafe` の両方が必要
- `--keep-stash` を付けると apply 後も stash を残す

### `use`

```bash
vw use feature/foo
vw use feature/foo --allow-shared
vw use feature/foo --allow-agent --allow-unsafe
```

機能:

- primary worktree を指定 branch に checkout
- primary context を固定したい用途向け

安全条件:

- primary が dirty なら拒否
- 対象 branch が他 worktree で使用中なら `--allow-shared` が必要（指定時は警告を表示）
- 非TTYでは `--allow-agent` と `--allow-unsafe` の両方が必要

### `exec`

```bash
vw exec feature/foo -- pnpm test
vw exec feature/foo --json -- pnpm test
```

機能:

- 指定 branch の worktree を `cwd` にしてコマンド実行
- shell 展開は使わず引数配列で実行

終了コード:

- 子プロセス成功: `0`
- 子プロセス失敗: `21`（JSON では `CHILD_PROCESS_FAILED`）

### `invoke`

```bash
vw invoke post-switch
vw invoke pre-new -- --arg1 --arg2
```

機能:

- `pre-*` / `post-*` hook を手動実行
- hook デバッグ用

### `copy`

```bash
vw copy .envrc .claude/settings.local.json
```

機能:

- repo 相対パスのファイル/ディレクトリを target worktree にコピー
- 主に hook 内で `WT_WORKTREE_PATH` と合わせて使う想定

### `link`

```bash
vw link .envrc
vw link .envrc --no-fallback
```

機能:

- target worktree 側に symlink を作成
- Windows では `--no-fallback` がない場合、copy にフォールバック可

### `lock` / `unlock`

```bash
vw lock feature/foo --owner codex --reason "agent in progress"
vw unlock feature/foo --owner codex
vw unlock feature/foo --force
```

機能:

- `lock`: `.vde/worktree/locks/` に lock 情報を保存
- `unlock`: lock を解除（owner 不一致時は `--force` 必須）

### `cd`

```bash
cd "$(vw cd)"
```

機能:

- `fzf` で worktree を対話選択
- Picker では worktree の branch 名 + 最小 state（dirty / merged / lock）を表示
- preview で path と states（dirty / locked / merged / upstream）を表示
- 対話ターミナルでは Picker/preview を Catppuccin 風 ANSI 色で表示
- 選択した絶対 path を stdout に出力

### `completion`

```bash
vw completion zsh
vw completion fish
vw completion zsh --install
```

機能:

- zsh / fish 向け補完スクリプトを出力
- `--install` 指定時はデフォルトまたは `--path` に補完ファイルを書き込む

## merged 判定（ローカル + PR）

各 worktree で次を評価します:

- `merged.byAncestry`: ローカル履歴判定（`git merge-base --is-ancestor`）
- `merged.byPR`: GitHub PR merged 判定（`gh`）
- `merged.overall`: 最終判定

`overall` ポリシー:

- `byPR === true` -> `overall = true`
- `byPR === false` -> `overall = false`
- `byPR === null` -> `byAncestry` にフォールバック

`byPR` が `null` になる例:

- `gh` 未導入
- `gh auth` 未設定
- API 失敗
- `git config vde-worktree.enableGh false`

## JSON 契約

`--json` 指定時、stdout は常に単一 JSON オブジェクトです。

共通成功フィールド:

- `schemaVersion`
- `command`
- `status`
- `repoRoot`

エラー時:

- `status: "error"`
- `code`
- `message`
- `details`

## 設定キー（git config）

- `vde-worktree.baseBranch`
- `vde-worktree.baseRemote`
- `vde-worktree.enableGh`
- `vde-worktree.hooksEnabled`
- `vde-worktree.hookTimeoutMs`
- `vde-worktree.lockTimeoutMs`
- `vde-worktree.staleLockTTLSeconds`

## 現在のスコープ

- Ink ベースの `tui` は未実装
