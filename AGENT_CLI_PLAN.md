# Agent CLI 改修計画

調査で確認した6つの関連領域を順次実装し、検証が通った単位でコミットする。
公開、push、crate version の更新はこの計画の対象に含めない。

## 改修単位

1. 実行結果と復旧情報：strict post-hook 失敗時も操作結果を保持し、失敗段階・完了済み処理・復旧資材・構造化警告を共通契約にする。adopt、transfer、削除、配置、completion install の失敗情報も保持する。
2. CLI の発見性：全コマンドと引数の help、前提・副作用・例、引数解析の一貫性、説明に依存しない動的補完、実定義から生成する describe を整備する。
3. 作業先と状態取得：曖昧な branch を拒否して候補を返す共通解決、明示 path、-C の実行コンテキスト、一覧・単一詳細・全件詳細の分離を実装する。
4. 設定と診断：CLI の明示指定を設定より優先し、無効な設定項目を整理する。context は設定の出所と実効値、doctor は未初期化・設定不正・依存機能・保留中復旧を診断する。verbose と PR の unknown 理由を観測可能にする。
5. 事前検査と削除：check / dry-run を副作用のない検査経路へ分離する。拒否理由・判定材料・保留中復旧を表示し、実行は対象を再検証する。del と gone のガードを明示し、PR と HEAD の対応を検証する。
6. プロセス制御：exec の timeout・出力上限・stdin・signal・省略情報を追加する。子孫プロセスを終了し、メタデータ復旧中の Git にも共通 runner の制限を適用する。

セッション予約の新設は別設計とし、今回は既存 lock owner にセッション固有値を使う運用例を追加する。
過去の Rust 移行 fixture を現在の describe の定義元には使わない。
JSON 契約を変更する場合は現行の単一契約へ更新し、旧 renderer や fallback は追加しない。

## DoD

### 機能完了条件

- [ ] 全23既存コマンドと追加コマンドの対象・前提・副作用・出力を help / describe から取得できる。
- [ ] strict post-hook 後の部分成功、バッチ内の失敗、復旧資材、失敗段階、警告を JSON で判別できる。
- [ ] shared branch の曖昧な対象は候補付きで拒否され、明示 path / -C で対象を指定できる。
- [ ] path は不要な詳細取得をせず、status と削除再検証は対象を限定して取得する。
- [ ] context / doctor が設定の出所・実効値・依存機能の状態を説明し、doctor は設定不正や未初期化でも診断を返す。
- [ ] 事前検査は hook、stash、repo lock 記録、メタデータ保存、自動復旧を行わず、実行直前は最新状態で判定する。
- [ ] exec と復旧中 Git の待ち時間が制御され、timeout・signal・出力省略を判別できる。

### テスト完了条件

- [ ] 引数エラーの JSON 判定、コマンド欠落、help と動的補完接続の回帰テストが通る。
- [ ] post-hook 後の結果保持、transfer / adopt / 削除 / 配置の途中失敗を検証する。
- [ ] shared branch、-C の伝播、対象別の Git コマンド数、検査前後の無変更を検証する。
- [ ] 設定優先順位、不正設定での doctor、PR の判定材料と取得失敗理由を検証する。
- [ ] timeout 時の子孫終了、stdin、signal 終了、出力上限を検証する。
- [ ] cargo fmt、clippy、全 target / feature のテスト、package 内容検証が通る。

### 運用反映条件

- [ ] 日英 README、vw / vde-worktree、生成済み zsh / fish 補完、describe と JSON 契約が一致する。
- [ ] 新しい実行時ファイル・テストを Cargo package と allowlist に反映する。
- [ ] 各改修単位を検証後にコミットし、最後に未コミット差分がないことを確認する。

## 検証記録

- 着手時：HEAD 7f0b9ab、作業ツリーに差分なし。全 target / feature の既存テストが成功し、同梱補完は生成結果と一致した。

- 改修1：JSON schema 3、strict post-hook 後の data 保持、構造化 hook 警告、adopt / gone の対象別エラー詳細、transfer・extract・mv・配置の復旧情報を実装。Rust 1.89.0 で全290テストと clippy が成功。

- 改修2：全引数の help、コマンドごとの前提・副作用・例、describe、引数解析の共通化、説明文に依存しない zsh/fish 動的補完を実装。既存受け入れテストに実出力の schema 検証を追加。全295テスト、clippy、両 shell の構文・候補取得、Cargo package allowlist 66ファイルの一致を確認。

- 改修3：-C、明示 worktree path、共有 branch の候補付き拒否、一覧と単一詳細の分離を実装。new / switch / get / lock / transfer の不要な全件詳細を除去し、del / gone は最新一覧の一意性を確認して対象だけを再検証する。実リポジトリで path の詳細取得0件、status 1件、del 2件、gone の候補別取得を確認。全299テスト、両 shell の -C を使う候補取得、Cargo package allowlist 67ファイルの一致を確認。

- 改修4：context / doctor、設定項目ごとの出所と CLI 優先順位、verbose、PR unknown の理由、構造化メタデータ警告、復旧 journal の無変更観測を実装。未使用 TTL 設定を削除し、初期化判定を必要な状態ディレクトリに統一。設定不正・未初期化・制御文字 path の診断、ファイル内容と更新時刻の不変性を検証。全305テスト、clippy、補完構文、Cargo package allowlist 69ファイルの一致を確認。

- 改修5：check / --dry-run を14種類の lifecycle 操作へ追加。通常の gone / adopt プレビューを含め、hook・stash・lock・Git index・メタデータ・復旧を変更しない経路を実装。検査と判定材料には同じ snapshot を使用し、PR HEAD 照合と削除直前の取り直しを実装。全308テスト、追加の引数伝播テスト、clippy、補完構文、Cargo package allowlist 70ファイルの一致を確認。復旧完了後の別処理失敗と復旧バッチ途中失敗でも完了情報を保持する。
