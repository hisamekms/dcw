[English](README.md) | [日本語](README.ja.md)

# dcw – Devcontainer Wrapper

> **アルファ版**: 本プロジェクトはアルファ版です。API やコマンド体系に破壊的変更が入る可能性があります。

`devcontainer` CLI をラップし、以下の機能を追加する Rust 製 CLI ツールです。

- **動的ポートフォワーディング** — socat ベースの Docker sidecar コンテナでポートをホストに公開
- **ポート自動監視** — コンテナ内の新しい LISTEN ポートを検出し、自動的にフォワード
- **設定マージ** — `devcontainer.local.json` を `devcontainer.json` に deep merge
- **ライフサイクル管理** — `up` / `exec` / `down` による devcontainer の操作

## 典型的な使い方

```sh
$ dcw up                          # devcontainer を起動（watch はデフォルトで有効）

# コンテナ内でサービスがポートを LISTEN
$ dcw exec -- python -m http.server 8080 &
# => dcw がポート 8080 を検出し、自動的にホストへフォワード

$ curl localhost:8080             # ホストからアクセス
# => 200 OK

$ dcw down                        # すべて停止（watcher、sidecar、コンテナ）
```

## インストール

**クイックインストール** (Linux x86_64/aarch64, macOS Apple Silicon):

```sh
curl -fsSL https://raw.githubusercontent.com/hisamekms/dcw/main/install.sh | bash
```

**インストール先を変更:**

```sh
DCW_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/hisamekms/dcw/main/install.sh | bash
```

**バージョンを指定:**

```sh
VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/dcw/main/install.sh | bash
```

## ソースからビルド

```sh
cargo build --release
```

バイナリは `target/release/dcw` に生成されます。

## 使い方

### `dcw up`

devcontainer を起動します。

```sh
# 基本的な起動（auto-forward と watch はデフォルトで有効）
dcw up

# コンテナを再ビルド
dcw up --rebuild

# devcontainer.json のポート自動フォワードを無効化
dcw up --auto-forward=false

# ポート自動監視を無効化
dcw up --watch=false

# devcontainer CLI に追加の引数を渡す
dcw up -- --config .devcontainer/custom.json
```

| フラグ | 型 | デフォルト | 説明 |
|--------|-----|-----------|------|
| `--rebuild` | bool | `false` | 既存コンテナを削除して再ビルド |
| `--auto-forward` | bool | `true` | 起動後に `forwardPorts` のポートをフォワード |
| `--watch` | bool | `true` | 新しい LISTEN ポートを検出して自動フォワード |

`--` 以降の引数は `devcontainer up` にそのまま渡されます。

### `dcw down`

devcontainer を停止します。以下の順序でクリーンアップを実行します。

1. ポート watcher を停止（実行中の場合）
2. ポートフォワーディング用の sidecar コンテナをすべて削除
3. devcontainer を停止

```sh
dcw down
```

### `dcw exec`

devcontainer 内でコマンドを実行します。マージ済み設定ファイルが存在する場合、自動的に適用されます。

```sh
dcw exec -- ls -la
dcw exec -- bash
```

`--` 以降の引数は `devcontainer exec` にそのまま渡されます。

### `dcw port`

ポートフォワードを管理します。

#### `dcw port add`

```sh
# ホストポート 8080 をコンテナポート 8080 にフォワード（バックグラウンド）
dcw port add -d 8080 8080

# 異なるホスト/コンテナポートでフォワード
dcw port add -d 3000 8080
```

| 引数 | 説明 |
|------|------|
| `<host_port>` | ホスト側のポート |
| `<container_port>` | コンテナ側のポート |

| フラグ | 型 | デフォルト | 説明 |
|--------|-----|-----------|------|
| `-d`, `--detach` | bool | `false` | バックグラウンドで実行 |

#### `dcw port remove` (エイリアス: `rm`)

```sh
# 特定のフォワードを削除
dcw port remove 8080

# すべてのフォワードを削除
dcw port rm --all
```

| フラグ | 型 | デフォルト | 説明 |
|--------|-----|-----------|------|
| `--all` | bool | `false` | すべてのポートフォワードを削除 |

#### `dcw port list` (エイリアス: `ls`)

```sh
dcw port list
dcw port ls
```

#### `dcw port watch`

コンテナ内の新しい LISTEN ポートを検出し、自動的にフォワードします。

```sh
# デフォルト設定で監視を開始
dcw port watch

# ポーリング間隔と除外ポートを指定
dcw port watch -i 5 --min-port 3000 -e 5432 -e 6379
```

| フラグ | 型 | デフォルト | 説明 |
|--------|-----|-----------|------|
| `-i`, `--interval` | 秒 | `2` | ポーリング間隔 |
| `--min-port` | u16 | `1024` | フォワード対象の最小ポート番号 |
| `-e`, `--exclude` | u16（複数指定可） | — | 自動フォワードから除外するポート |

### `dcw update`

dcw を最新バージョンに更新します。

```sh
# 最新版に更新
dcw update

# バージョンを指定してインストール
dcw update --version v0.2.0

# 強制的に再インストール
dcw update --force
```

| フラグ | 型 | デフォルト | 説明 |
|--------|-----|-----------|------|
| `--version` | string | 最新版 | 特定のバージョンをインストール |
| `--force` | bool | `false` | 最新版でも強制的に更新 |

## devcontainer.json の `forwardPorts`

`--auto-forward` が有効（デフォルト）の場合、`dcw up` は `.devcontainer/devcontainer.json` から `forwardPorts` を読み取ります。`.devcontainer/devcontainer.local.json` が存在する場合は、先に deep merge してからポートを読み取ります。対応フォーマット:

```jsonc
{
  "forwardPorts": [
    3000,                    // 数値
    "8080",                  // 文字列
    "localhost:9090",        // host:port 形式の文字列
    { "port": 5432 }         // オブジェクト
  ]
}
```

### 設定マージの動作

`devcontainer.local.json` は `devcontainer.json` に deep merge されます。

- **オブジェクト** は再帰的にマージ（local のキーが base を上書き）
- **配列・スカラー値** はそのまま置換（追加ではなく上書き）

マージ結果は XDG ランタイムディレクトリに `devcontainer.merged.json` として書き出され、`dcw exec` で自動的に使用されます。

## 仕組み

### sidecar によるポートフォワーディング

ポートフォワーディングは `alpine/socat` を実行する Docker sidecar コンテナで実現されます。フォワードするポートごとに `pf-<workspace>-c<port>` という名前の sidecar が作成され、以下を行います。

1. devcontainer の Docker ネットワークに参加
2. `-p 127.0.0.1:<port>:<port>` でホストポートを LISTEN
3. socat 経由で devcontainer にトラフィックを転送

> **注意**: devcontainer が複数の Docker ネットワークに接続されている場合、最初に見つかったネットワークが sidecar の通信に使用されます。

sidecar は冪等です。既存のポートに対して `dcw port add` を実行すると、以前の sidecar が置き換えられます。

### ポートの自動監視

`dcw port watch`（および `dcw up --watch`）はコンテナ内の `/proc/net/tcp` と `/proc/net/tcp6` をポーリングし、LISTEN ソケットを検出します。新しいポートが検出されると（`--min-port` 以上かつ `--exclude` に含まれない場合）、sidecar が自動作成されます。ポートが LISTEN を停止すると、対応する sidecar が削除されます。

watcher の PID は XDG ランタイムディレクトリに保存され、`dcw down` 時のクリーンアップで使用されます。

### 設定ファイルのマージ

`.devcontainer/devcontainer.local.json` が存在する場合、`dcw up` は `devcontainer.json` に deep merge し、結果を XDG ランタイムディレクトリ（`$XDG_RUNTIME_DIR/dcw/<workspace>/devcontainer.merged.json`）に書き出します。このマージ済み設定は `devcontainer up` および `devcontainer exec` に `--config` フラグ経由で渡されます。

## 必要なもの

- [devcontainer CLI](https://github.com/devcontainers/cli) (`npm install -g @devcontainers/cli`)
- Docker
