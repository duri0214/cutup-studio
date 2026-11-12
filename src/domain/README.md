# ローカル解析の設計と準備

Issue #5 の実装に向けたメモです。whisper.cpp と FFmpeg で文字起こし候補を生成し、必要に応じて Ollama のローカルモデルで説明候補を生成します。既存の素材走査・手入力・検索に外部ツールは不要です。

## 切り替えの設計案

利用者が明示的に解析を開始しなければ従来の手入力を使う。現在の文字起こしには whisper.cpp、説明候補には任意で Ollama を使う。Ollama のテキストモデルだけでは音声の文字起こしを行わない。画像解析などの方式は未実装。

| 場所 | 責務の予定 |
| --- | --- |
| `domain/` | 解析対象の範囲、候補、モデル情報、ジョブ状態などの業務上の意味と制約 |
| `application/` | 明示的な開始、方式の選択、素材単位の実行・中断・再実行、候補の保存の調整 |
| `infra/` | Ollama との通信、STT の実行、FFmpeg のプロセス実行、永続化 |
| 組み立て部分 | 設定に応じた具体実装の生成と接続 |

切り替えのための factory が必要になれば組み立て部分に置く。domain は Ollama、FFmpeg、HTTP や実行ファイルのパスを直接参照しない。具体的な解析方式を実装するときに必要な境界だけ定義する。

解析結果は手入力と分けた未確定の候補とし、利用者が確認・修正する。候補にはモデル名・識別用ハッシュ、入力範囲、確度、根拠を記録し、取得できない情報は不明として扱う。素材の指紋とモデルの指定に変更がなければ保存済みの結果を表示する。同じパスや名前のモデルを更新した場合は、画面の「同じ素材と設定でも再解析する」を選ぶ。失敗・中断は素材単位で表示する。

## Windows での準備

以下は外部ツール単体の確認です。モデルの選択やライセンス、必要なメモリ・ディスク容量は実際の解析方式を決める際に確認してください。

### 文字起こし

1. [whisper.cpp v1.7.6 の配布ページ](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.7.6) から `whisper-bin-x64.zip` を取得して展開します。展開先の `Release/whisper-cli.exe` を指定します。
2. [配布モデル一覧](https://huggingface.co/ggerganov/whisper.cpp/tree/main) から日本語を含む多言語モデル `ggml-tiny.bin` を取得し、保存先の絶対パスを控えます。小型モデルは誤認識するため、精度が必要な場合は別の多言語モデルを選んでください。
3. 素材の情報を登録する画面の「解析」で `whisper-cli` の実行ファイル、モデル、FFmpeg のパスを指定し、「文字起こしを開始」を押します。Ollama モデル欄には `gemma4:e2b` のような導入済みローカルモデル名を入力できます。空欄なら Ollama を使用しません。クラウドモデルは使用できません。生成結果は推定であり、内容と区間を確認してください。

PowerShell でユーザー領域に準備する場合の例です。ダウンロードには通信とディスク容量が必要です。

```powershell
$cutupWhisperDir = Join-Path $env:LOCALAPPDATA 'cutup-studio\whisper'
New-Item -ItemType Directory -Force -Path $cutupWhisperDir | Out-Null
Invoke-WebRequest 'https://github.com/ggml-org/whisper.cpp/releases/download/v1.7.6/whisper-bin-x64.zip' -OutFile (Join-Path $cutupWhisperDir 'whisper.zip')
Expand-Archive -LiteralPath (Join-Path $cutupWhisperDir 'whisper.zip') -DestinationPath $cutupWhisperDir
Invoke-WebRequest 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin' -OutFile (Join-Path $cutupWhisperDir 'ggml-tiny.bin')
& (Join-Path $cutupWhisperDir 'Release\whisper-cli.exe') --help
Write-Output (Join-Path $cutupWhisperDir 'Release\whisper-cli.exe')
Write-Output (Join-Path $cutupWhisperDir 'ggml-tiny.bin')
```

最後の2行が画面に入力する `whisper-cli` と `Whisper モデル` のパスです。不要になった場合は、アプリを終了してから `cutup-studio\whisper` フォルダを削除します。

### Ollama

1. [Ollama の Windows 手順](https://docs.ollama.com/windows) からインストーラーを取得して実行します。既定ではユーザー領域にインストールされ、ユーザーの PATH に追加されます。バックグラウンドで起動し、ローカル API は既定で `http://localhost:11434` です。モデルの保存先変更や必要容量も公式手順を参照してください。
2. 新しい PowerShell で以下を実行します。`gemma4:e2b` は接続確認用のローカルテキストモデルの例で、解析用として採用を確定したものではありません。ダウンロードには容量と通信が必要です。

   ```powershell
   ollama --version
   ollama pull gemma4:e2b
   ollama list
   ollama run gemma4:e2b "こんにちは、と一言だけ返してください"
   ```

   バージョン、モデル一覧と応答が出ることを確認します。モデル取得と実行の詳細は [公式クイックスタート](https://docs.ollama.com/quickstart) を参照してください。
3. 終了するときは Windows の通知領域の Ollama を終了します。不要になった確認用モデルは `ollama rm gemma4:e2b` で削除できます。アプリ本体の削除は Windows の「インストールされているアプリ」から行います。保存先を変更したモデルは別途管理してください。

### FFmpeg

1. [FFmpeg のダウンロードページ](https://ffmpeg.org/download.html) に案内されている Windows ビルドを選び、配布元の条件を確認して取得・展開します。初期配布物には同梱しません。
2. 展開した `bin/ffmpeg.exe` の絶対パスを控え、PowerShell で次の例のパスを実際の場所に置き換えて確認します。PATH の変更は不要です。

   ```powershell
   & 'C:\tools\ffmpeg\bin\ffmpeg.exe' -version
   ```

   バージョンとビルド情報が表示されれば実行できます。不要になった展開フォルダは削除できます。再配布を検討する場合は、そのビルドの構成とライセンス条件を別途確認します。

## 実装時の確認

- 外部ツール未導入と「なし」の双方で、起動・素材走査・手入力・検索・構成操作ができる。
- 各方式について必要なモデルとツールがない場合の案内、および失敗・中断・再実行を確認する。
- 実素材で候補の生成から確認・修正まで検証し、モデルと入力範囲を記録する。テキストモデルの応答確認だけを文字起こしの検証とみなさない。
