<div align="center">

<img src="docs/brand/rushwind-icon.svg" alt="RushWind Admin" width="128">

# RushWind Admin

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.81+-DEA584?logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/tx7do/rushwind-admin/actions/workflows/ci.yml/badge.svg)](https://github.com/tx7do/rushwind-admin/actions/workflows/ci.yml)

[English](./README.en-US.md) | [中文](./README.md) | **日本語**

</div>

---

## プロジェクトのハイライト

- **マルチフロントエンド**：`Vue3 Vben`（Ant Design Vue）、`Vue3 Element Plus`、`React19 Antd` の 3 種類のフロントエンドを提供し、チームの好みに合わせて選択可能
- **エンタープライズ RBAC**：マルチテナント、マルチロール、マルチ部署、メニュー/ボタン/データレベルの権限制御に対応
- **契約駆動コード生成**：Protobuf が唯一の API 契約——284 ルート、546 件のエラーステータスマッピング、556 個のサービスインターフェースをビルド時に決定論的に生成、手書きルートはゼロ。3 フロントエンドの TypeScript クライアントはバイト単位で同一
- **セキュリティと等級保護コンプライアンス**：等保 2.0 の技術要件に基づき、180 日間の監査ログ保持・アーカイブ、パスワードポリシー三種セット、TOTP MFA、パスワードのアプリケーション層暗号化、動的 RBAC とマルチテナント分離を内蔵。詳しくは[セキュリティと等級保護コンプライアンス](#セキュリティと等級保護コンプライアンス)
- **本番 ready の基盤**：JWT RS256 認証、統一 4 フィールドエラーエンベロープ、CORS、SSE プッシュ（:7789 /events）、非同期タスクキュー（Postgres キュー worker + cron プロデューサー）
- **品質ゲート**：fmt / clippy / test / 契約同期の 4 ゲート（ubuntu + windows マトリクス）

## デモ

| フロントエンド | デモ |
|---------|--------|
| Vue3 Vben | <https://vben.admin.gowind.cloud> |
| Vue3 Element Plus | <https://ele.admin.gowind.cloud> |
| React | <https://react.admin.gowind.cloud> |

- バックエンド Swagger：<https://api.demo.admin.gowind.cloud/docs/>
- デフォルトアカウント：`admin` / `Abcd@1234`

---

## スクリーンショット

### 管理コンソール

3 種類のフロントエンドは本リポジトリのバックエンドにゼロ改修で接続できます。以下は React 版フロントエンド + REST :7788 の実際の画面です。

**ログイン** — 画像認証コードとテナント番号（空欄ならプラットフォームログイン）、パスワードはアプリケーション層で暗号化して送信

![ログイン](./docs/screenshots/console-login.png)

**ダッシュボード** — ユーザー / ロール / ログイン / 操作監査の集計と、ログイン推移・比率グラフ

![ダッシュボード](./docs/screenshots/console-dashboard.png)

**ユーザー管理** — 組織ツリーと高度な検索、複数ロール・複数部署・状態の管理

![ユーザー管理](./docs/screenshots/console-users.png)

**メニュー管理** — ディレクトリ / メニュー / ボタンの 3 種ノードと、権限識別子・ルート・コンポーネントパスの対応

![メニュー管理](./docs/screenshots/console-menus.png)

**操作監査ログ** — 操作種別・対象リソース・リクエスト ID を成功 / 失敗ともに記録

![操作監査ログ](./docs/screenshots/console-op-audit.png)

**オンラインユーザー** — セッションとデバイスの一覧、強制ログアウトに対応

![オンラインユーザー](./docs/screenshots/console-online.png)

### API ドキュメント

バックエンドが API ドキュメントを自前で配信します（`server.yaml` の `enable_swagger` / `enable_redoc` スイッチ、生の仕様は `/q/openapi.yaml`）。別途ドキュメントサイトをデプロイする必要はありません。

**Swagger UI** — `/q/swagger-ui`：全エンドポイントをサービス単位でグループ化し、認証エントリとその場で試せるコンソールを備えます

![Swagger UI](./docs/screenshots/swagger-ui.png)

**ReDoc** — `/q/redoc`：3 カラム構成のリファレンス。パラメータの説明とリクエスト / レスポンス例を並べて表示します

![ReDoc](./docs/screenshots/redoc.png)

---

## 技術スタック

<table>
<tr><th>レイヤー</th><th>技術</th></tr>
<tr><td><strong>バックエンド</strong></td><td><code>Rust</code>（edition 2021 / MSRV 1.81） · <code>RushWind</code> フレームワーク · <code>axum 0.8</code> · <code>tokio</code></td></tr>
<tr><td><strong>契約パイプライン</strong></td><td><code>Protobuf</code> · <code>buf + protox</code> · <code>prost / prost-reflect / pbjson</code> · <code>rushwind-gen-http</code>（ルート生成）</td></tr>
<tr><td><strong>ストレージ</strong></td><td><code>SeaORM</code> · <code>PostgreSQL</code> · <code>Redis</code>（セッション / 認証コード / pub-sub）</td></tr>
<tr><td><strong>認可・認証</strong></td><td><code>JWT RS256</code>（rushwind-authn-jwt） · <code>動的 RBAC</code>（ポリシーは DB 保存、ホットリロード）</td></tr>
<tr><td><strong>Vue Vben 版</strong></td><td><code>Vue 3</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Ant Design Vue</code> · <code>Vben Admin</code></td></tr>
<tr><td><strong>Vue Element 版</strong></td><td><code>Vue 3</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Element Plus</code>（軽量版）</td></tr>
<tr><td><strong>React 版</strong></td><td><code>React 19</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Zustand</code> · <code>Ant Design V6</code>（UMI 不使用）</td></tr>
<tr><td><strong>品質ゲート</strong></td><td><code>cargo fmt / clippy / test</code> · <code>GitHub Actions</code>（ubuntu + windows マトリクス）</td></tr>
</table>

---

## アーキテクチャ

### 契約駆動の生成チェーン

Protobuf が唯一の API 契約です。ルート、エラーステータステーブル、サービスインターフェース、マウントグルー、フロントエンド TypeScript クライアントは、すべてビルド時に契約から決定論的に生成されます。リポジトリに手書きルートは 1 本もなく、契約を 1 か所変えればバックエンドと 3 フロントエンドが同時に追従します。

```text
backend/api/protos/              proto 契約（唯一の事実源、MANIFEST 検証ゲート、手編集禁止）
        │  buf + protox が注釈ディスクリプタへコンパイル
        ▼
backend/crates/proto             build.rs がビルド時に生成
        ├── ROUTES ルートテーブル（284 本、additional_bindings を含む）
        ├── エラーステータステーブル（546 件の reason → HTTP ステータス対応）
        ├── サービス trait（556 メソッド）+ マウントグルー
        └── protojson コーデック（ゴールデンテストで固定）
        ▼
backend/services/admin-api       生成された trait に手書きサービス実装を接続
```

### 1 リクエストの経路

```text
ブラウザ → edge（CORS / リクエスト予算）→ バインディング層（protojson body + form query、束縛失敗は認証に先立って 400 を返す）
        → 認証ゲート（JWT RS256 検証 + Redis セッションチェック → テナントチェック → authz 認可評価）
        → 生成 handler → Service（ビジネスロジック）→ Repo（クエリ述語は Viewer から派生）
        → ストレージ層テナントルール（プラットフォームビューは全件 / テナントビューは tenant_id を強制 / システムビューはバイパス）→ PostgreSQL
```

横断的関心事はビジネスコードには書きません：認証は認証ゲート、行レベル分離は Viewer 派生のクエリ述語、フィールドレベル権限はサーバー側レスポンストリミング、6 種類の監査はミドルウェア層——ビジネスコードからは「見えない」のに、すべての経路がその適用下にあります。

### 1 ライフサイクル、4 つの transport

| チャネル | ポート | 用途 |
|------|------|------|
| REST | :7788 | 管理面ルートのすべて |
| SSE | :7789 `/events` | 社内メッセージ / 通知のサーバープッシュ、ストリームはユーザー帰属で検証、接続は独立に認証 |
| タスク worker | —（リスニングポートなし） | Postgres キューを消費：メッセージブロードキャストの扇出、監査アーカイブ、定時バックアップなどの非同期ジョブ |
| cron プロデューサー | —（リスニングポートなし） | 周期タスク投入：テナント期限スキャン、監査アーカイブ、sys_tasks テーブル駆動のユーザータスク |

### 生成物と手書き物

| 位置 | 生成方式 | 手編集可否 |
|------|---------|---------|
| `backend/api/protos/**` | `sync-protos.sh` が契約ソースから同期（MANIFEST 検証ゲート） | ❌ ソース変更後に再同期 |
| `backend/crates/proto` の生成面 | `build.rs` がビルド時に決定論的に再生成 | ❌ |
| フロントエンド `src/api/generated/**` | 契約から生成された TypeScript クライアント | ❌ |
| `services/admin-api/src/services/**`、`src/data/**` | 手書きのビジネスロジックとデータ層 | ✅ |

層ごとの詳しい解説は[チュートリアル第 1 章：アーキテクチャ概観](./docs/tutorial/01-architecture-overview.md)を参照。

---

## セキュリティと等級保護コンプライアンス（等保 2.0）

本プロジェクトのセキュリティ機能は『ネットワーク安全等級保護 2.0』（レベル 2/3）の技術要件に基づいて設計されており、高いプライバシー性が求められるエンタープライズ私的導入シーンでそのまま使えます：

| 等保技術要件 | 実装 |
|------------|---------|
| **安全監査** | 6 種類の監査ログをフルカバー：ログイン / 操作 / API / データアクセス / 権限変更 / ポリシー評価。IP 所在地と trace_id を記録。保持・アーカイブポリシーは調整可能（既定は DB 内 180 日、期限超過データは JSONL アーカイブとして書き出し） |
| **身份認証** | パスワード複雑度（8 文字以上・小文字/大文字/数字/記号の 4 種中 3 種）、過去パスワード再利用チェック（既定近接 3 件）、パスワード有効期限（既定 90 日）、しきい値は「パラメータ管理」コンソールで調整可能；TOTP 多要素認証（MFA）；画像認証コード；ログイン失敗レート制限（IP + ユーザー名の二軸）；設定可能なログイン制限ポリシー |
| **アクセス制御** | 動的 RBAC エンジン、ロール—権限—API マッピングは DB に保存し、権限変更は即時ホットリロード；メニュー/ボタンレベルの権限制御、ロール単位の行レベルデータ範囲とフィールドレベル権限（ブラックリスト項目はレスポンスから自動トリミング）；認可判定ごとにポリシー評価ログを記録 |
| **マルチテナント分離** | ストレージ層でのデータ分離：読み取りクエリにテナントフィルタを自動注入、書き込みはテナント偽装を防止、更新/削除はテナント述語を強制；テナントリクエストは `(path, method)` で Api テーブルにより fail-closed 検証（権限ポイントがなければ拒否）；プランのモジュールホワイトリストと期限切れ読み取り専用ポリシー |
| **データ機密性** | ログインパスワードのアプリケーション層 AES 暗号化伝送、bcrypt ハッシュ保存；JWT RS256 非対称署名；refresh token は HttpOnly Cookie；トランスポート層 TLS はデプロイ層で終端 |
| **データバックアップ・リカバリ** | 定時フルバックアップ（pg_dump、既定 30 世代保持・自動ローテーション）、Docker コンテナ / ローカル直結の両モードに対応 |
| **フロントエンドセキュリティ** | 3 フロントエンドとも本番ビルドで CSP、X-Frame-Options、HSTS などのセキュリティヘッダーを有効化 |

> **注記**：等保評価には技術要件以外に管理制度・物理環境・人員組織などの非ソフトウェア範疇も含まれます。本プロジェクトがカバーするのは技術措置の部分であり、私的導入の等保評価準備を直接支援できますが、完全な等保評価プロセスの代替にはなりません。

---

## 現在の進捗

プロジェクトは [docs/development-plan.md](./docs/development-plan.md) のフェーズに沿って進行しており、契約サーフェスは完了、サービス実装はモジュール単位で漸進的に展開中です。

**完了済み**

- 契約パイプライン：proto 同期（MANIFEST 検証ゲート + ドリフト検知）→ 注釈ディスクリプタ → ビルド時の決定論的生成（ルート / エラーステータステーブル / サービスインターフェース / マウントグルー）
- REST :7788 アセンブリ：284 ルート、8 エンドポイントの認証不要ホワイトリスト、JWT RS256 認証ゲート、CORS、統一 4 フィールドエラーエンベロープ（code / reason / message / metadata）
- コーデック整合：protojson のリクエスト束縛とレスポンス出力（64 ビット整数の文字列化、presence 省略、well-known 型セマンティクス）、ゴールデンテストで固定
- 実サービス実装が組織 / 権限 / 認証 / メッセージ / 監査 / 監視の全ドメインをカバー、さらに ce8ab49 vintage で移植された新ドメイン：OIDC SSO シングルサインオンとテナントホワイトラベル、通知設定とテンプレート、チャネル拡張（39 EXT チャネルの schema レジストリ + settings 保存）、監視アラート（しきい値エバリュエータ + クールダウン/リカバリの状態機械）、システムタスク巡検、BI ダッシュボード（楽観ロック + 内蔵テンプレート）、印刷テンプレート（サーバーサイド HTML→PDF レンダリング）、AI 全域（プロバイダ / 会話 / メッセージ / 使用量 / ナレッジベース RAG / Agent グラフエンジン / コンテンツ生成）
- NL2SQL（スマートデータ照会）は明示的に NOT_IMPLEMENTED を宣言：4 層の読み取り専用ガードチェーン（テーブルホワイトリスト / LIMIT クランプ / ステートメント読み取り専用検証 / テナント事後 fail-closed）が全体として揃うまで、生成 SQL の実行は解放しない

**進行中 / 計画**

- 発信配信：SMTP / WEBHOOK / SMS / EXT チャネルの実 sender と台帳実投（通知ドメインの既存滞留）、アラート周期スキャン、ユーザーごとの希望言語でレンダリングされるトランザクションメール
- SSE ビジネス側プッシュ（社内メッセージ / 通知のリアルタイム配信）、監査ログアーカイブ
- ファイルドメインのナレッジベース投入（UploadDocFile）

---

## クイックスタート

### 環境要件

| ツール | バージョン |
|------|------|
| Rust | stable（ワークスペース `rust-version = 1.81`） |
| buf | 最新版（`curl -fsSL https://buf.build/install.sh | sh`、または [GitHub releases](https://github.com/bufbuild/buf/releases) の単一バイナリ）；`cargo build` 時に PATH 必須（アノテーションクロージャのコンパイル） |
| bash | 同期スクリプトの実行（Windows は Git Bash 推奨） |
| Docker | 20.0+（ローカルミドルウェア） |
| Node.js + pnpm | 各フロントエンドの `package.json` `engines` に準拠（現在の制約交集 ≥ 20.19.0）、pnpm >= 10.0.0 |

### 依存関係の解決

フレームワークおよびユーティリティの依存（RushWind フレームワーク、rust-utils）は Git 依存としてリビジョン固定されており、本リポジトリのクローンだけでビルドできます。兄弟リポジトリは不要です。フレームワークをローカルで改修する場合のみ、`rushwind/` を兄弟ディレクトリにクローンし、ワークスペースの `[patch]` セクションで一時的にローカルパスを指してください（フレームワーク貢献者向けワークフロー）。

### バックエンド起動

```shell
cd backend
cargo run -p admin-api   # バイナリ admin-api、REST :7788 で待受
```

- 起動時に PostgreSQL と Redis に接続します。設定は `backend/services/admin-api/assets/`（`auth.yaml` / `data.yaml` / `oss.yaml`）
- `assets/jwt_public_key.pem` と `auth.yaml` 埋め込み鍵は**デモ用鍵**です。本番導入時は必ず交換してください
- 起動シーケンス：マイグレーション → シード → REST（:7788）+ SSE（:7789、`/events`）+ タスクワーカー + cron の 4 transport を同一ライフサイクルで起動

### 契約同期

proto 契約はスクリプトによって上流の契約ソースから同期され、MANIFEST 検証ゲートで守られています：

```shell
bash backend/api/sync-protos.sh          # proto を同期し MANIFEST を再構築
bash backend/api/sync-protos.sh --check  # 検証ゲート（CI と同一）
```

同期元の既定パスと上書き方法はスクリプト先頭の説明（`backend/api/sync-protos.sh`）を参照。`backend/api/protos/` は**手で編集しないでください**。

### 品質ゲート

```shell
cd backend
cargo fmt -p proto -p auth -p admin-api -p admin-diff -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

CI（ubuntu + windows マトリクス）は同じ 4 ゲートを実行します：fmt / clippy / test / 契約同期チェック。詳細は [.github/workflows/ci.yml](./.github/workflows/ci.yml) を参照。

### フロントエンド起動

フロントエンドとバックエンドは無変更互換契約です：API ベース URL を本バックエンド（REST :7788）に向けるだけで、フロントエンドのコードは一切変更不要です。三つのフロントエンドすべて本リポジトリに同期スナップショットとして同梱（RushWind ブランドオーバーレイ + 双マニフェスト検証ゲートで手編集を防止）され、dev プロキシは既定で :7788 を向いています：

| フロントエンド | ディレクトリ | 状態 | 起動コマンド | ポート |
|---------|------|------|---------|------|
| React | `frontend/admin/react` | ✅ 同梱スナップショット + ブランドオーバーレイ | `pnpm dev` | 5888 |
| Vue Element | `frontend/admin/vue-element` | ✅ 同梱スナップショット + ブランドオーバーレイ | `pnpm dev` | 5777 |
| Vue Vben | `frontend/admin/vue-vben` | ✅ 同梱スナップショット + ブランドオーバーレイ | `pnpm dev:antd` | 5666 |

```shell
# React版（本リポジトリ frontend/admin/react）
cd frontend/admin/react
pnpm install
pnpm dev            # :5888、REST :7788 へプロキシ

# Vue Element版（本リポジトリ frontend/admin/vue-element）
cd frontend/admin/vue-element
pnpm install
pnpm dev            # :5777、REST :7788 へプロキシ

# Vue Vben版（本リポジトリ frontend/admin/vue-vben、モノレポ、ルートで実行）
cd frontend/admin/vue-vben
pnpm install
pnpm dev:antd       # :5666、REST :7788 へプロキシ
```

---

## 機能一覧

すべての機能は単一サービス（admin-api）内で提供され、3 フロントエンドは機能的に同型です。以下「組織と権限 / システム機能 / メッセージとログ / 個人センター」の 4 グループで列挙します。

> 各一覧ページ（業務データと監査ログ）は現在の絞り込み条件でページング集約エクスポートに対応、CSV / XLSX 形式（上限 1 万行）。バックエンドのモジュール別実装状況は[現在の進捗](#現在の進捗)を参照。

### 組織と権限

| 機能 | 説明 |
|------|-----|
| ユーザー管理 | ユーザーの管理・照会、高度な検索と部署連動のユーザー一覧に対応。有効/無効化、上司の設定/解除、パスワードリセット、マルチロール・マルチ部署・上司の設定、ワンクリックユーザーログインなど |
| テナント管理 | テナントの管理。新規作成時にテナント部署・既定ロール・管理者を自動初期化。プラン設定、有効/無効化、テナント管理者へのワンクリックログインに対応 |
| プランとクォータ管理 | テナントのサブスクリプションプランとリソースクォータ（モジュールホワイトリスト、使用量上限）の管理、プランとクォータ項目の CRUD に対応 |
| ロール管理 | ロールとロールグループの管理、ロール連動のユーザー一覧、メニュー認可、行レベルデータ範囲（5 段階 / カスタム組織ユニット集合）とフィールドレベル権限（ブラックリスト項目集合）、一括メンバー追加・削除 |
| 権限管理 | 権限グループ・メニュー・権限ポイントの管理、ツリー表示に対応 |
| 組織管理 | 組織の管理、ツリー表示に対応 |
| 役職管理 | ユーザー役職の管理、役職はユーザーのタグとして機能；Excel インポートに対応（クライアントテンプレートダウンロード、既存 API による行単位作成、行レベルエラーレポート、所属組織列は組織名で完全一致照合） |
| メニュー管理 | システムメニュー・操作権限・ボタン権限識別子の設定（ディレクトリ・メニュー・ボタン）；メニュー同期に対応（3 フロントエンド対応、トランザクション再構築 or 増分マージの 2 モード、マージモードはフルパス一致で原位置更新し既存メニュー ID とロール認可を保持） |

### システム機能

| 機能 | 説明 |
|------|-----|
| API 管理 | API の管理と同期機能、主に権限ポイント新規作成時のエンドポイント選択に使用。ツリー表示、操作ログのリクエスト/レスポンス記録設定に対応 |
| 辞書管理 | データ辞書の大カテゴリと小カテゴリの管理、大カテゴリ→小カテゴリの連動、サーバーサイド複数列ソート、インポート・エクスポートに対応 |
| タスクスケジューリング | タスクと実行ログの管理・照会、新規・変更・削除・起動・一時停止・即時実行に対応 |
| ファイル管理 | ファイルアップロードの管理、照会・OSS/ローカルへのアップロード・ダウンロード・URL コピー・削除・画像プレビューに対応 |
| ログインポリシー | ログイン制限ポリシーの管理、対象ユーザーの制限タイプ・方式・値・理由を設定 |
| アカウントログイン | ユーザー名 / メール / 電話番号のマルチ識別子ログイン、画像認証コード・ログインポリシー・TOTP MFA と組み合わせ可能；OIDC シングルサインオン（`SSO_OIDC_*` 環境変数で有効化、discovery キャッシュ + state の単回消費によるリプレイ防止 + メールによるローカルアカウントマッピング、自動プロビジョニング可）とテナントホワイトラベル照会（ログイン前にテナント番号で名称 / ロゴを取得、未命中と未設定は同一回答で列挙防止） |
| 多要素認証（MFA） | TOTP ベースの多要素認証、ログインチャレンジ、個人センターでのバインド管理、管理者による MFA リセットの救済パスを含む |
| パスワード復旧 | メール認証コードによるパスワード復旧：コードは 10 分間・1 回限り有効、成功時に全セッション失効、ユーザー列挙防止のためサイレント処理 |
| 通知チャネル | 通知チャネル（EMAIL / WEBHOOK / SMS と 39 の EXT 拡張チャネル）の管理。拡張チャネル設定は schema に応じて動的レンダリング（機密フィールドは暗号化保存・一覧はマスキング表示・空欄は保持）、テスト送信前にタイプ別の設定存在プレチェックを実施 |
| 通知テンプレート | プラットフォーム全体の `{{var}}` プレースホルダテンプレート管理、コードはグローバル一意、変数セットによる試しレンダリングに対応（未認識プレースホルダは名前単位でエラー） |
| 通知設定 | ユーザー自身のサイレント時間帯（0 時をまたぐ窓に対応）と社内メッセージのカテゴリ別配信停止、読み書きは現在の操作者にアンカー |
| サービス監視 | サービス実行時メトリクス（CPU コア数、メモリ、稼働時間など）の読み取り専用表示、自動リフレッシュ |
| スクリプトシステム | スクリプトレベルのプラグインシステム（Lua / JavaScript、DB を事実のソース、管理画面の変更は即時反映）：エンティティライフサイクルフック（before は否決可能 / after は非同期）、定時タスク、HTTP 外部送信（ドメインホワイトリスト fail-closed）、ドライランと実行ログ |
| パラメータ管理 | プラットフォーム全体のシステムパラメータのキーバリュー管理（業務辞書とは別物）、内蔵パラメータは起動時にシード・削除禁止・変更可；サービス側はキャッシュ accessor 経由で読み取り、マルチインスタンス構成では Redis pub/sub で変更をブロードキャストし各インスタンスのキャッシュを無効化 |
| マシン認証情報（AK/SK） | テナント単位の AccessKey / SecretKey 管理：作成時に Secret を 1 回だけ表示、有効/無効化・削除・ローテーションリセットに対応（ローテーション後の旧 Secret は即時無効）；AK/Secret はトークン交換エンドポイントでテナントスコープのマシン JWT（machine ロール、access トークンのみ）と交換可能、交換エンドポイントは IP + AK 単位でレート制限 |
| BI ダッシュボード | デザイナーが生成したダッシュボード / ビッグスクリーン / テンプレートの管理：デザイナー schema を原文のまま透過、コードはテナント内一意、更新は楽観ロックのバージョン検証付き、内蔵テンプレートは削除禁止・ワンクリック複製でテナント資産化、起動時にベースラインテンプレートをシード |
| 印刷テンプレート | 印刷テンプレート管理 + サーバーサイド PDF レンダリング：フロントエンドカーネルが最終ドキュメント HTML を生成し、バックエンドはローカルの Chrome/Edge（CHROME_PATH 探索 / Edge フォールバック）でゼロマージン印刷。「プレビューの通りに印刷」され、バックエンドはテンプレートの再レイアウトも解析もしません |
| 監視アラート | メトリクスしきい値アラートルール（goroutine / メモリ / DB 接続数 / DB 接続性 / Redis 容量）、手動即時評価に対応；アラート→リカバリのペア通知、越限継続中はクールダウンで再送制御、収集失敗回では firing 状態をクリアしない（ジッターをリカバリと誤判定しない） |
| システムタスク巡検 | システムレベル常駐タスクの読み取り専用巡検：スケジューラエントリ（cron / 前回 / 次回エンキュー）+ タスクタイプ別のキュー状態と最近の失敗明細、worker が一度も起動していない場合はエラーではなく空面で回答 |
| AI アシスタント | LLM プロバイダ管理（OpenAI 互換 / ローカルモデル、API Key 暗号化保存 + マスキング表示）、会話とメッセージ、ナレッジベース（ドキュメントのチャンク分割・ベクトル化登録、pgvector コサイン検索）、Agent アプリ（5 ノードのグラフオーケストレーションエンジン：start / knowledge / llm / condition / end、テンプレートと条件式、実行トレースと token 使用量を記録）、コンテンツ生成（4 シナリオ）、使用量台帳と月次サマリー |
| 言語管理 | システム対応言語の管理、言語名・言語コード・ローカル名・有効状態と既定状態を設定 |

### メッセージとログ

| 機能 | 説明 |
|------|-----|
| メッセージ分類 | メッセージ分類の管理、2 レベルのカスタム分類に対応、メッセージ管理の分類選択に使用 |
| メッセージ管理 | メッセージの管理、送信範囲（全ユーザー / 指定ユーザー）別送信と取り消しに対応、全ユーザーブロードキャストは非同期タスクキューで配信（レジューム可能・冪等）、既読状態と既読時刻を確認可能 |
| 社内メッセージ | 社内メッセージの管理、詳細表示・削除・既読化・すべて既読に対応 |
| ログインログ | ログインログの一覧照会、成功・失敗を記録、IP 所在地記録に対応 |
| 操作ログ | 操作ログの一覧照会、正常・異常な操作を記録、IP 所在地記録とリソース特定、詳細表示に対応 |
| API ログ | API ログの一覧照会、操作者・リクエストパス・メソッド・成功状態を記録、IP 所在地記録に対応 |
| データログ | データアクセスログの一覧照会、SQL の語彙マスキング、関連テーブル名とデータ分類の自動抽出 |
| 権限ログ | 権限変更ログの一覧照会、操作者・対象オブジェクト・理由を記録、リクエストスナップショットを保持 |
| ポリシー評価ログ | ポリシー評価ログの一覧照会、認可判定の結果と評価コンテキストを記録、trace_id による関連調査に対応 |
| Redis キャッシュ監視 | Redis キャッシュ監視、Redis INFO・DBSIZE・スローログの読み取り専用表示、書き込み操作なし |

### 個人センター

| 機能 | 説明 |
|------|-----|
| 個人センター | 個人情報の表示・変更、最終ログイン情報の確認、パスワード変更、メールバインド・再バインド（認証コード検証）など |

---

## プロジェクト構成

```text
rushwind-admin/
├── backend/
│   ├── api/                        # API 契約ツリー（唯一の契約ソース）
│   │   ├── protos/                 # proto 契約コピー（MANIFEST.sha256 検証ゲート）
│   │   ├── buf.lock                # buf 依存の commit ピン止め（6 BSR モジュール、上流 lock と同源）
│   │   └── sync-protos.sh          # 契約同期・検証スクリプト
│   ├── crates/                     # 共有 crate（proto 契約生成 crate、auth 認証ゲート）
│   └── services/
│       └── admin-api/              # Admin サービス crate（src/ モジュールツリー + assets 埋め込みリソース）
├── frontend/                       # 3 フロントエンド同期スナップショット（sync-frontend.sh + 双マニフェストゲート + RushWind ブランドオーバーレイ）
├── docs/                           # プロジェクトドキュメント（チュートリアル tutorial / 索引 README / binding-spec / development-plan / operator-matrix / screenshots）
└── .github/workflows/              # CI（fmt / clippy / test / 契約同期ゲート）
```

---

## ドキュメント

チュートリアルとリファレンスは [docs/README.md](./docs/README.md) から：

- **チュートリアル層**（[docs/tutorial/](./docs/tutorial/README.md)）—— 全 8 章の段階的学習：アーキテクチャ概観 → ローカル起動 → 契約とコード生成 → 最初のサービスモジュール → 権限モデル → マルチテナント分離 → 監査とコンプライアンス → デプロイ
- **リファレンス層** —— [binding-spec](./docs/binding-spec.md)（ワイヤ契約の整列仕様）などのエンジニアリング基準。該当サブシステムに手を入れる前の必読文書

---

## 関連プロジェクト

- **[rushwind](https://github.com/tx7do/rushwind)** —— RushWind フレームワーク monorepo（http / http-binding / authn / transport-axum / gen-http など）
- **[rust-utils](https://github.com/tx7do/rust-utils)** —— Rust ユーティリティライブラリ（`query_parser` など）

## コミュニティとコントリビューション

RushWind Admin への貢献を歓迎します：

- [コントリビューションガイド](./CONTRIBUTING.md) —— 兄弟クローン配置、契約同期の規約、コミット規約と PR フロー
- [セキュリティポリシー](./SECURITY.md) —— 脆弱性の報告フローと対象範囲

## お問い合わせ

- WeChat（個人）：`yang_lin_bo`（備考：`rushwind-admin`）
- 掘金コラム：<https://juejin.cn/column/7541283508041826367>

## 謝辞

[![JetBrains](https://resources.jetbrains.com/storage/products/company/brand/logos/jb_beam.svg)](https://jb.gg/OpenSource)

JetBrains に無料のオープンソースライセンスを提供いただき、感謝します。
