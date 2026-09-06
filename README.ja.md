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

## コマンドの調べ方

各コマンドの `--help` は、対象・前提・副作用・引数・実行例を説明します。
機械向けの定義は、repository 内外から `describe` で取得できます。

```bash
vw describe --json
vw describe exec --json
```

`describe` は実際の CLI 引数定義、制約、動的補完の候補種別、共通 envelope と各コマンドの結果の JSON Schema を返します。
単一コマンドを指定すると応答を小さくできます。
受け入れテストでは、実際の成功・部分成功の出力をこの schema と照合します。
`fixtures/rust-migration/` は過去の移行時の記録で、現行の公開契約は `describe` で取得します。
明示的な `--help` と `--version` は常にテキスト表示です。
`vw --json` のように option だけでコマンドがない入力は、終了コード3の引数エラーになります。

## 実行ディレクトリと worktree の指定

`-C DIRECTORY`（`--directory`）で、repository と設定を解決するディレクトリを指定できます。
相対指定の `--worktree` と補完のインストール先も、このディレクトリを基準にします。
hook は対象 repository / worktree のコンテキストで動作し、動的なシェル補完にも `-C` が伝わります。

```bash
vw -C /projects/repo status --json
vw -C /projects/repo path --worktree .worktree/topic --json
vw -C /projects/repo exec --worktree .worktree/topic -- cargo test
vw -C /projects/repo copy --worktree .worktree/topic .env.local
```

`status`・`path`・`exec`・`copy`・`link` は `--worktree PATH` を受け付けます。
登録済み worktree の内部ディレクトリも指定できます。
branch と path は同時に指定できません。
同じ branch に複数の worktree がある場合は、`details.candidates` に候補を含むエラーを返します。
`switch` と `get` も曖昧な既存の割り当てを拒否します。
`lock` と `unlock` は branch 単位のメタデータを保護するため、その branch の全 worktree に所有権が適用されます。

`path` は worktree 一覧だけを読み、base branch や GitHub の照会を必要としません。
`status` は選択した1件だけを詳しく調べます。
削除は最新の一覧で branch の一意性を確認してから、対象1件のガードを再検証します。
明示 path では detached worktree も選択でき、その場合の `path` / `exec` JSON の `branch` は `null` です。
末尾を含む UTF-8 の空白は保持し、非 UTF-8 と制御文字は1行 path の契約で拒否します。
`copy` / `link` の対象は、明示 `--worktree`、`WT_WORKTREE_PATH`、現在の worktree の順で決まります。
環境変数の相対 path は実行ディレクトリを基準にします。

## 診断と実効設定

```bash
vw context --json
vw doctor --json --no-gh
```

`context` は repository の各 path、初期化状態、管理ルート、base branch、保留中のメタデータ journal を返します。
`config.effective` は CLI の上書きを含む実効値で、`config.sources` は各項目の既定値・設定ファイル・CLI 引数を示します。
設定ファイルは global、repository、実行ディレクトリに近い設定の順に適用します。
明示 `--hooks` / `--gh` は設定の無効化を上書きし、肯定・否定の CLI flag は最後の指定が優先されます。
`--fzf-arg` は設定済み引数へ追加し、両方の出所を記録します。

`doctor` は Git 外や設定不正でも、独立した診断項目を返します。
保留中の transaction を復旧せずに読み、repository lock の取得や hook の実行は行いません。
必要な設定・初期化などに問題がある場合は終了コード4を返し、`data` に診断結果を保持して `healthy` を `false` にします。
任意の依存コマンドの不足は warning です。
依存コマンドの検査には各5秒の制限を設けています。
GitHub 連携が有効なら認証も確認するため通信が発生する場合があり、`--no-gh` でこの検査を省略できます。
`context` は GitHub を照会しません。

`--verbose` は解決したコンテキストと実行結果を stderr に出し、繰り返すと実効設定も表示します。
stdout の JSON は1個のオブジェクトを維持します。
メタデータの警告は envelope の `warnings` 配列からも取得できます。

PR 取得の失敗理由は `pr.diagnostic` に入り、`disabled`、`dependency_missing`、`authentication_required`、`command_failed`、`timed_out`、`invalid_response`、`not_observed` を区別します。
取得できた終了コードとメッセージも保持します。
認証要求の判定は [GitHub CLI の終了コード契約](https://cli.github.com/manual/gh_help_exit-codes) に従い、通信障害などのコマンド失敗は元の診断メッセージを残します。
PR 状態が不明な場合の `merged.byPR` は引き続き `null` です。

使われていなかった `locks.staleLockTTLSeconds` は削除し、不明な設定キーとして拒否します。
repository lock の有効期間は OS の lock が管理し、`locks.timeoutMs` は待機時間を指定します。
初期化完了には `vw init` が作成する hooks・logs・locks・state ディレクトリが必要です。

## 変更を伴わない事前検査と削除の判定材料

```bash
vw check --json -- del feature/topic
vw del feature/topic --dry-run --json
vw check --json -- gone --apply
vw new feature/topic --dry-run --json
```

`check -- COMMAND ...` と `--dry-run` は、`init`・`new`・`switch`・`get`・`adopt`・`mv`・`del`・`gone`・`extract`・`absorb`・`unabsorb`・`use`・`lock`・`unlock` の14操作を検査します。
出力形式や実行ディレクトリの option は、`check` より前、または `check` の `--` より前に指定します。
検査は hook、stash、repository lock、メタデータ保存、Git index の更新、自動復旧を実行しません。
有効な GitHub 照会と `get` の remote branch 確認は通信を伴う場合があり、前者は `--no-gh` で省略できます。

結果は `allowed`、`target`、予定する `effects`、`rejections`、`pendingRecoveries`、`requiresRevalidation: true` を含みます。
バッチ操作の候補は `plannedResult` に入り、削除の `evidence` は検査に使った同一 snapshot と、独立に確認できる拒否理由を返します。
実行できない場合もこの情報を保持し、最初の拒否理由に対応する非ゼロの終了コードを返します。
検査は作業先を予約せず、hook や外部プロセスの成功も保証しません。
実行時は最新の状態を再検証します。
`new --dry-run` が生成した branch 名を使う場合は、実行時にその名前を明示してください。

通常の `gone` / `adopt` も変更を伴わないプレビューです。
`--dry-run` を指定すると詳細な検査形式になり、通常のプレビューと `--apply` は各コマンドの結果形式を返します。
保留中の復旧がある検査は journal を変更せずに拒否します。
実際の lifecycle 操作では repository lock を取得し、復旧してから計画を作ります。
完了した復旧は、後続の操作が失敗しても `METADATA_RECOVERY_COMPLETED` の構造化 warning に残します。
復旧バッチの途中で失敗した場合は `error.details.completedRecoveries` に完了分を保持します。

`list`・`status`・`del`・`gone` は同じ GitHub 有効化設定を使います。
マージ済み PR を `merged.byPR: true` の根拠にできるのは、`pr.headOid` と現在の worktree の HEAD が一致する場合だけです。
PR の HEAD が不明なら `head_unavailable`、異なれば `head_mismatch` を診断に残し、`byPR` は `null` にします。
削除は pre-hook 後にもこの情報を取り直します。
一致する PR があれば squash merge 後も削除でき、Git ancestry が true である必要はありません。
`del` は upstream への未送信や不明状態も拒否し、明示した override でのみ許可します。
`gone` は upstream-ahead をガードに使いません。
両方とも dirty、lock、merge、管理対象 path、branch の一意性を確認します。

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

補完スクリプトは Rust binary から動的候補を取得します。
候補との接続にはコマンド名と引数名を使い、help の説明文には依存しません。
`switch` と `use` の候補には、まだ worktree を持たないローカル branch も含みます。

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

- `--dry-run`: lifecycle 操作を変更せずに検査する

- `--json`: 機械可読の単一 JSON 出力
- `-C <directory>` / `--directory <directory>`: 実行コンテキストの基準ディレクトリ
- `--worktree <path>`: status・path・exec・copy・link の明示対象
- `--verbose`: コンテキストと結果の診断。繰り返すと実効設定も表示
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

`--json` 指定時、stdout はschema version 3の単一 JSON objectです。

共通成功フィールド:

- `schemaVersion`
- `command`
- `status`
- `repoRoot`
- `data`
- `error`
- `warnings`

エラー時:

- `status: "error"`
- `data`は通常`null`で、部分成功を返すcommandでは完了済みresultを保持
- `error.code`
- `error.message`
- `error.details`
- `error.execution`: `phase`、`state`、`completed`、`recovery`

`warnings` は常に配列で、各診断は `error` と同じフィールドを持ちます。
hook の診断には `details.hook`、`details.phase`、`details.logPath` を含め、human 向けの警告も stderr に出します。
strict post-hook が失敗した場合も `data` を保持し、操作本体と状態保存が完了していれば `error.execution.state` は `applied` になります。
pre-hook 失敗時の `rolledBack` はコマンド自身の staging の復元を表し、hook が独自に行った副作用の取消しは保証しません。

phase は `parse`、`resolve`、`configure`、`lock`、`recover`、`preflight`、`stage`、`preHook`、`apply`、`finalize`、`postHook`、`process`、`unknown` です。
state は `notStarted`、`rolledBack`、`applied`、`partial`、`recoveryRequired`、`unknown` です。
`completed` は完了を観測した処理、`recovery` は残っている stash OID・path・失敗した後処理などを表します。
`unknown` は結果が確定していないことを表します。いずれの state も、無条件に再実行してよいという意味ではありません。
一括操作は、成功済みの結果とともに対象ごとの `details` と `execution` を保持します。

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
