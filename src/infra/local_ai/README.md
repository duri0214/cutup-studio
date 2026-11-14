# ローカル解析の準備（Windows）

素材の台詞を文字で探すため、音声を STT（音声認識）で文字起こしします。必要に応じて、その文字から Ollama が内容や反応の候補を作ります。どちらも確認・修正するための候補です。現在、解析候補は既存の検索にはまだ反映されません。

| 必要なもの | 用途 | 必要なとき |
| --- | --- | --- |
| FFmpeg | 動画・音声を文字起こし用の音声に変換 | 文字起こし |
| whisper.cpp と音声認識モデル | 音声を文字と時刻に変換 | 文字起こし |
| Ollama とローカルモデル | 文字起こしから説明候補を作成 | 説明候補も作る場合 |

どれもアプリの通常起動や手入力には不要です。素材はクラウドに送信せず、Ollama の API キーも不要です。

## 1. FFmpeg

PowerShell で `ffmpeg -version` を実行し、バージョンが出れば次に進みます。見つからない場合は次を実行し、新しい PowerShell を開いて再確認します。

```powershell
winget install "FFmpeg (Essentials Build)"
ffmpeg -version
```

`winget` を使えない場合は [FFmpeg 配布ページ](https://www.gyan.dev/ffmpeg/builds/) の `ffmpeg-release-essentials.zip` を展開し、`bin\ffmpeg.exe` の絶対パスを控えます。アプリに同梱しません。

## 2. whisper.cpp と音声認識モデル

**次の導入コマンドは初回だけ実行し、導入後は2回目を実行しないでください。** Windows x64 用の whisper.cpp v1.7.6 と、日本語を含む小型モデルをユーザー領域に保存します。ダウンロードには通信と空き容量が必要です。導入済みの場合は、次の「導入後の確認」に進んでください。

再実行すると、展開先に同名のファイルがあるため `ExpandArchiveFileExists` エラーが表示されることがあります。その場合は上書きせず、下の「導入後の確認」で実行ファイルとモデルを確認してください。

```powershell
$cutupWhisperDir = Join-Path $env:LOCALAPPDATA 'cutup-studio\whisper'
New-Item -ItemType Directory -Force -Path $cutupWhisperDir | Out-Null
Invoke-WebRequest 'https://github.com/ggml-org/whisper.cpp/releases/download/v1.7.6/whisper-bin-x64.zip' -OutFile (Join-Path $cutupWhisperDir 'whisper.zip')
Expand-Archive -LiteralPath (Join-Path $cutupWhisperDir 'whisper.zip') -DestinationPath $cutupWhisperDir
Invoke-WebRequest 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin' -OutFile (Join-Path $cutupWhisperDir 'ggml-tiny.bin')
```

### 導入後の確認

何度でも実行できます。ファイルのダウンロードや書き換えは行いません。

```powershell
$cutupWhisperDir = Join-Path $env:LOCALAPPDATA 'cutup-studio\whisper'
& (Join-Path $cutupWhisperDir 'Release\whisper-cli.exe') --help 2>&1 | Select-String '^usage:'
Test-Path -LiteralPath (Join-Path $cutupWhisperDir 'ggml-tiny.bin') -PathType Leaf
Write-Output (Join-Path $cutupWhisperDir 'Release\whisper-cli.exe')
Write-Output (Join-Path $cutupWhisperDir 'ggml-tiny.bin')
```

`usage:` で始まる1行と `True` が表示されれば、実行ファイルが起動し、モデルファイルが存在します。これはモデルの読み込みや文字起こしの成否までは確認しません。最後に表示される2行が実行ファイルとモデルのパスです。この v1.7.6 の Windows 版は `--version` に対応していません。小型モデルは誤認識するため、結果を確認してください。不要になったらアプリ終了後に `$env:LOCALAPPDATA\cutup-studio\whisper` を削除できます。

## 3. Ollama（説明候補も作る場合）

1. [Ollama ダウンロード](https://ollama.com/download) を開き、`Windows` → `Download manually` を選んで `OllamaSetup.exe` を保存し、ダブルクリックしてインストールします。通常は管理者権限不要です。既定のインストール先は `%LOCALAPPDATA%\Programs\Ollama` です。
2. スタートメニューから Ollama を起動します。インストール前から開いていた PowerShell では、まず次を実行して、このウィンドウの PATH に Ollama を追加します。既定のインストール先が異なる場合は、そのフォルダに置き換えてください。

   ```powershell
   $env:Path += ";$env:LOCALAPPDATA\Programs\Ollama"
   ollama --version
   ```

   バージョンが表示されることを確認してから、次のコマンドでモデルを取得します。`gemma4:e2b` は動作確認の例で、取得には数 GB の空き容量と通信が必要です。

   ```powershell
   ollama pull gemma4:e2b
   ollama list
   ollama run gemma4:e2b "こんにちは、と一言だけ返してください"
   ```

   モデル名と応答が表示されれば準備完了です。アプリはローカル API `http://127.0.0.1:11434` に接続します。クラウドモデルは使用できません。

   この PC にモデルを一度取得すれば、削除しない限り `ollama pull` や確認用の `ollama run` を毎回実行する必要はありません。アプリで Ollama を使う解析をするときは、Ollama がバックグラウンドで起動している必要があります。停止している場合はスタートメニューから起動してください。手入力や、Ollama を使わない文字起こしでは起動不要です。

   `$env:Path += ...` は現在の PowerShell ウィンドウだけに有効です。通常は Ollama のインストーラーがユーザーの PATH に登録するため、PowerShell を起動しているターミナルアプリごと終了して開き直すと、新しいウィンドウにも反映されます。ユーザー PATH に登録がない場合は、Windows で「環境変数を編集」と検索し、「ユーザー環境変数」の `Path` → `編集` → `新規` から `C:\Users\<ユーザー名>\AppData\Local\Programs\Ollama`（既定のインストール先）を追加し、`OK` で保存してターミナルアプリを開き直します。既に同じ項目があれば追加不要です。
3. Ollama を停止するときは通知領域の Ollama を終了します。確認用モデルは `ollama rm gemma4:e2b` で削除できます。本体は Windows の「インストールされているアプリ」から削除します。保存先を変更したモデルは別途管理してください。

### インストール失敗時

#### `ollama` がコマンドとして認識されない

手順 2 で PATH を追加しても見つからなければ、`& "$env:LOCALAPPDATA\Programs\Ollama\ollama.exe" --version` で実行ファイルを直接確認します。別の場所にインストールした場合はその場所を指定してください。実行ファイル自体が見つからない場合は、手順 1 のインストールを確認します。

#### `Installation failed with exit code 5`

`irm https://ollama.com/install.ps1 | iex` の `Installation failed with exit code 5` は原因そのものではありません。スクリプトはインストーラーを非表示で実行するため、まず上記の `OllamaSetup.exe` を通常実行し、表示されたエラーを確認してください。

最新のインストールログは PowerShell で次のように確認できます。

```powershell
Get-ChildItem $env:TEMP -Filter 'Setup Log*.txt' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 | Get-Content -Tail 40
```

確認例: `ollama app.exe` のインストール中に `An error occurred while trying to read the source file` と `指定されたファイルが見つかりません` が記録され、処理が取り消されたケースがありました。このログだけでは権限不足とは確認できません。新しくダウンロードしたインストーラーを通常実行し、同じエラーならログの該当行を確認します。Windows セキュリティなどにブロックの通知がある場合は、その内容も確認します。

ログや画面に `Access is denied` / `アクセスが拒否されました` と明示された場合は、保存先の権限を確認し、必要に応じて `OllamaSetup.exe` を右クリックして「管理者として実行」で再試行できます。管理者での実行は今回のエラーの原因や解決を保証しません。公式の通常インストールはユーザー権限で動きます。

## 4. アプリで使う

`cargo run` で起動して `DEMO` を選ぶと、素材の読み込み後に右側に `素材の情報を登録` が開きます。右側の `解析` タブを選び、`素材` で対象を選びます。画面を閉じた場合は、左側の `素材の情報を登録` ボタンから再び開けます。`音声認識` で `Whisper` を選びます。FFmpeg は Whisper の前処理として自動で使います。実行ファイルは `実行ファイルとモデルの設定` または 03 の `音声エンコード` の `実行ファイルを選択` で指定でき、開いているカタログで共通です。説明候補も必要なら `テキスト解析` で `Ollama / gemma4:e2b` を選び、不要なら `なし` にします。モデルの一覧は選択欄を開くと自動で取得します。選択した Whisper と Ollama には `ローカル` と表示されます。Ollama が停止中なら、`テキスト解析` の右にある Ollama のアイコンを押して起動します。Windows では時計の近くの通知領域に Ollama のアイコンが表示されます。`文字起こしを開始` を押すと解析が始まります。

Ollama のサーバーがすでに動いている場合は、そのサーバーに接続します。通知領域にアイコンがない場合は、別の方法で起動されたサーバーを停止してから、Ollama のアイコンを押してください。

既定の場所にない実行ファイルやモデルは、`実行ファイルとモデルの設定` を開き、各項目の `選択` から指定します。必要なファイルが見つからない間は開始できません。

`音声の言語` は既定では `自動検出` です。短い日本語音声を別の言語と誤認識した場合は `日本語` を選び、再度 `文字起こしを開始` を押してください。文字起こしの品質は素材と音声認識モデルに依存します。Ollama の要約も文字起こしの誤りに影響されます。

候補テキストは画面で修正できます。同じ素材と設定では保存済み結果を表示します。同じ名前のモデルを更新した場合など、再実行するときは `同じ素材と設定でも再解析する` を選んでください。再解析が成功すると以前の候補と修正は置き換わります。

## 将来の API 方式

現在の音声認識とテキスト解析はローカル方式のみで、外部 API は選べません。将来は、それぞれ外部 API を使う方式も選べるようにする余地があります。その場合は、処理先を `ローカル` / `API` と区別し、利用者が方式を選べるようにします。

API 方式を追加するときは、送信する音声・テキスト、送信先、料金や認証を利用者に分かるようにしたうえで、明示的な操作で実行します。現在の Issue #5 はクラウドへの素材送信を対象外としており、API 対応は未実装です。

## コードの配置

`local_stt.rs` は FFmpeg・whisper.cpp、`ollama.rs` は Ollama への接続を担当します。別のローカル LLM を導入するときは、この `local_ai/` 内に実装を追加します。解析候補の値と保存はそれぞれ `domain/valueobject/catalog.rs` と `infra/sqlite_catalog.rs` にあります。

参考: [Ollama の Windows 仕様](https://docs.ollama.com/windows)、[インストーラー終了コード](https://jrsoftware.org/ishelp/topic_setupexitcodes.htm)
