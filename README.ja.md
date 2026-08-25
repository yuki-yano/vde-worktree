# vde-worktree

`vde-worktree` は、人間とコーディングエージェントの両方を想定した、安全な Git worktree 管理 CLI です。

利用できるコマンド名:

- `vde-worktree`
- `vw`（エイリアス）

英語版ドキュメント: `README.md`

## このツールで解決すること

- 管理対象 worktree を設定可能なルート配下に集約（デフォルト: `.worktree/`）
- `switch` を冪等にして、同じ指示を繰り返しても破綻しにくくする
- `del` / `gone` の破壊操作に安全ガードを入れる
- Agent 向けに安定した JSON 出力を提供
- hooks ベースで運用を拡張しやすくする

## 動作要件

- Rust 1.89以降（sourceから導入する場合）
- `fzf`（`cd` に必須）
- `gh`（PR 状態判定に任意）

対応platformはmacOS arm64、macOS x86_64、Linux x86_64です。

## インストール / ビルド

crates.ioからインストール:

```bash
cargo install vde-worktree --locked
```

このrepositoryの現在のsourceからローカルインストール:

```bash
cargo install --path . --locked
```

すでにインストール済みのlocal buildを置き換える場合:

```bash
cargo install --path . --locked --force
```

通常は`~/.cargo/bin`に`vw`と`vde-worktree`が配置されます。

```bash
~/.cargo/bin/vw --version
```

`vw`が見つからない場合は、`~/.cargo/bin`を`PATH`へ追加してshellを`rehash`してください。

ローカルビルド:

```bash
cargo build --locked
```

開発時の検証:

```bash
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

実行時にJavaScript runtimeは不要です。

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

補完スクリプトはRust binaryだけで動的候補を取得します。

`--install`は同一filesystemのtransaction directoryを経由して補完ファイルをatomic replaceします。rename後のdirectory syncが失敗した場合は、errorを返す前に以前のfileを復元します。

## 管理ディレクトリ

`vw init` 実行後に次を管理します:

- `<worktreeRoot>/`（管理対象 worktree ルート、デフォルト: `.worktree/`）
- `.vde/worktree/hooks/`
- `.vde/worktree/logs/`
- `.vde/worktree/locks/`
- `.vde/worktree/state/`

またGit common directoryの`info/exclude`（通常は`.git/info/exclude`）に管理対象エントリを冪等で追記します。

## `.worktreeinclude`

`new`、`switch`、`get`でworktreeを新規作成するとき、repository rootのregular file
`.worktreeinclude`に指定したlocal fileをprimary worktreeからコピーします。patternは否定を含む
`.gitignore`構文です。未追跡かつGit ignoredで、`.worktreeinclude`に一致するsymlinkではないregular
fileだけが対象です。tracked file、symlink、既存destination、destinationの親が非directoryであるpath、
管理worktree root、`.vde/worktree/`配下は除外します。既存worktreeを再利用するときは処理しません。

コピーはtransactionalです。コピーまたはcleanupが失敗すると、新規worktreeと、安全に削除できる
場合はこの操作で作成したbranchをロールバックします。実行中に対象fileが変化すると作成全体が失敗して
ロールバックされるため、安定したlocal設定fileを指定してください。

## 全体ルール

- 多くの書き込み系コマンドは `init` 実行済みが前提
- Git ref、worktree、repository local stateを変更するworktree lifecycle commandは内部のrepo lockで排他制御（`exec`、`invoke`、`copy`、`link`は取得しない）
- `--json` 指定時、stdout は単一 JSON オブジェクトのみ
- ログや警告は stderr に出力
- 非TTYで unsafe 操作を行う場合は `--allow-unsafe` が必要

## グローバルオプション

- `--json`: 機械可読の単一 JSON 出力
- `--verbose`: 詳細ログ
- `--hooks` / `--no-hooks`: hookを有効/無効化（無効化は`--allow-unsafe`必須）
- `--gh` / `--no-gh`: `gh`によるPR状態判定を有効/無効化
- `--full-path`: `list`のpath省略を無効化
- `--allow-unsafe`: unsafe 操作の明示同意
- `--strict-post-hooks`: post-hook失敗をwarningではなくerrorにする
- `--hook-timeout-ms <ms>`: hook timeout 上書き
- `--lock-timeout-ms <ms>`: repo lock timeout 上書き
- `--prompt <text>`: `cd`のfzf promptを上書き
- `--fzf-arg <arg>`: 予約option以外のfzf引数を追加（複数回指定可）

## コマンド詳細

### `init`

```bash
vw init
```

機能:

- `<worktreeRoot>/` と `.vde/worktree/*` を作成
- `.git/info/exclude` に管理エントリ追加
- デフォルト hook テンプレートを作成

### `list`

```bash
vw list
vw list --json
vw list --no-gh
vw list --full-path
vw list --json --no-gh --monitor
```

機能:

- Git の porcelain 情報から worktree 一覧を取得
- branch/path/dirty/lock/merged/PR/upstream を表示
- JSON メタデータには non-base branch ごとに `pr.status` と `pr.url` を含む
- テーブル表示では長い `path` は端末幅に合わせて `…` で省略
- `--full-path` でテーブル表示の path 省略を無効化
- `--no-gh` 指定時は PR 状態判定をスキップ（`pr.status` は `unknown`、`merged.byPR` は `null`）
- `--monitor` はmonitor連携向けの内部用・機械可読snapshot profile。`--json --no-gh` が必須で、`--gh` とは併用不可。upstream probeを省略してupstream各fieldをunknown（`null`）にし、lifecycle observationを永続化しない
- 対話ターミナルでは Catppuccin 風の ANSI 色で表示
- `NO_COLOR`指定時と非TTYではANSI色を出力しない

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

- 新しい branch + worktree を管理対象ルート（`paths.worktreeRoot`）に作成
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

### `adopt`

```bash
vw adopt
vw adopt --json
vw adopt --apply
```

機能:

- 管理外の非 primary worktree を検出し、管理対象ルートへの移動候補を作成
- デフォルトは dry-run、`--apply` で `git worktree move` を実行
- スキップ理由（`detached` / `locked` / `target_exists` / `target_conflict`）を出力

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

- primary worktree の現在 branch を管理対象ルート（`paths.worktreeRoot`）へ切り出し
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
- `--from` は vw 管理 worktree 名のみ指定可能（`<worktreeRoot>/...` の path 指定は不可）

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
- `--to` は vw 管理 worktree 名のみ指定可能（`<worktreeRoot>/...` の path 指定は不可）

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
vw exec feature/foo -- cargo test
vw exec feature/foo --json -- cargo test
```

機能:

- 指定 branch の worktree を `cwd` にしてコマンド実行
- shell 展開は使わず引数配列で実行
- human modeは子processのstdin、stdout、stderrを継承
- JSON modeは子processのstdoutとstderrを`data.childStdout`と`data.childStderr`へ格納

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

## Hook契約

hookは`.vde/worktree/hooks/pre-<action>`または`post-<action>`の実行可能ファイルとして配置します。

pre-hookは操作前に存在するsource worktreeまたはrepository root、post-hookは操作後のtarget worktreeをcwdにします。

共通環境変数:

- `WT_REPO_ROOT`: repository root。
- `WT_ACTION`: `new`、`switch`などのaction名。
- `WT_BRANCH`: preflightで確定したtarget branch。対象がなければ空文字。
- `WT_WORKTREE_PATH`: preflightで確定したtarget path。対象がなければ空文字。
- `WT_IS_TTY`: TTYなら`1`、それ以外は`0`。
- `WT_TOOL`: `vde-worktree`。

`mv`は`WT_OLD_BRANCH`と`WT_NEW_BRANCH`、`absorb` / `unabsorb`は`WT_SOURCE`と`WT_TARGET`も渡します。

実行ログは`.vde/worktree/logs/`に保存し、`hook`、`phase`、`start`、`end`、`exitCode`、`timedOut`、`stderr`を記録します。

pre-hook失敗は操作を中止します。post-hook失敗は既定でwarningとし、`--strict-post-hooks`時はerrorにします。timeoutは`--hook-timeout-ms`で指定できます。

### `copy`

```bash
vw copy .envrc .claude/settings.local.json
```

機能:

- repo 相対パスのファイル/ディレクトリを target worktree にコピー
- 主に hook 内で `WT_WORKTREE_PATH` と合わせて使う想定
- path batch全体をprivateなrandom transaction directoryへstagingしてからtargetを変更
- 後続pathのcommitが失敗した場合は、それ以前に反映した全pathをrollback

### `link`

```bash
vw link .envrc
```

機能:

- target worktree 側に symlink を作成
- repository rootのsourceを指すrelative symlinkだけを作成
- symlink作成に失敗した場合はerrorを返し、copyへ暗黙に切り替えない

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
- `--install` 指定時はデフォルトまたは `--path` に補完ファイルをatomic replaceする
- branch、remote branch、hook、管理worktree名の動的候補はRust binaryから取得する
- rename後のdirectory syncが失敗した場合は以前のcompletionを復元する

## merged 判定（ローカル + PR）

各 worktree で次を評価します:

- `merged.byAncestry`: ローカル履歴判定（`git merge-base --is-ancestor`）
- `merged.byPR`: GitHub PR merged 判定（`gh`）
- `merged.overall`: 最終判定
- `pr.status`: PR 状態（`none` / `open` / `merged` / `closed_unmerged` / `unknown`）
- `pr.url`: branch の最新 PR URL（取得不可時は `null`）

`overall` ポリシー:

- `byPR === true` -> `overall = true`（squash/rebase merge を含む）
- `byAncestry === false` -> `overall = false`
- `byAncestry === true` の場合は、分岐の証跡があるときだけ merged 扱い
  - `.vde/worktree/state/branches/*.json` の lifecycle 記録
  - lifecycle がない場合の `git reflog` フォールバック
- 分岐証跡が `baseBranch` に取り込まれていれば `overall = true`
- `byPR === false` または lifecycle が明示的に未取り込みなら `overall = false`
- それ以外は `overall = null`

`byPR` が `null` かつ `pr.status` が `unknown` になる例:

- `gh` 未導入
- `gh auth` 未設定
- API 失敗
- `config.yml` の `github.enabled: false`
- `--no-gh` を指定して実行

## JSON 契約

`--json` 指定時、stdout はschema version 2の単一 JSON objectです。

共通成功フィールド:

- `schemaVersion`
- `command`
- `status`
- `repoRoot`
- `data`
- `error`

エラー時:

- `status: "error"`
- `data`は通常`null`で、部分成功を返すcommandでは完了済みresultを保持
- `error.code`
- `error.message`
- `error.details`

## 設定（config.yml）

設定ファイルは次の順で読み込みます:

- `$XDG_CONFIG_HOME/vde/worktree/config.yml`（fallback: `~/.config/vde/worktree/config.yml`）
- `cwd` から Git 境界（`.git`）まで探索した `.vde/worktree/config.yml`
- `<repoRoot>/.vde/worktree/config.yml`（linked worktree 実行時も参照）

主な設定キー:

```yaml
paths:
  worktreeRoot: .worktree
git:
  baseBranch: null
  baseRemote: origin
github:
  enabled: true
hooks:
  enabled: true
  timeoutMs: 30000
locks:
  timeoutMs: 15000
  staleLockTTLSeconds: 1800
list:
  table:
    columns: [branch, dirty, merged, pr, locked, ahead, behind, path]
selector:
  cd:
    prompt: "worktree> "
    surface: auto # auto | inline | tmux-popup
    tmuxPopupOpts: "80%,70%"
```

補足:

- `paths.worktreeRoot` は repo 相対 path / 絶対 path の両方を指定可能
- 通常repositoryでは`.git`配下（例: `.git/worktrees`）も指定可能
- submoduleでは`.git`がfileになるため、`.git`配下ではなくデフォルトの`.worktree`などを使用する
- `paths.worktreeRoot` が既存ファイルを指す場合は設定エラー

## 現在のスコープ

- built-in TUIは初回Rust releaseに含まない
- `fzf`による対話選択、preview、tmux popupをgraphical表示として提供
