# エンコード

カメラと深度の画素が最終的にどんな動画になるかを決める設定です。既定（ソフトウェア AV1）のままで品質・サイズとも実用になるので、変えたい理由ができるまでこのページは読まなくてかまいません。

設定の入口は 2 つあり、どちらも同じ Encoder を動かします。パイプラインの `video_config` / `depth_config` ブロック（[設定のステージリファレンス](configuration_ja.md#ステージリファレンス)）と、`rebake-cli export` のフラグ（[CLI](cli_ja.md#export)）です。export はスループット重視の既定値を使うので、その差分は[export の既定値](#export-の既定値)にまとめています。

## RGB 動画

### コーデックを選ぶ

サイズとデコード速度の綱引きで選びます。

| 目的 | `codec` | 品質ノブ（目安の範囲） |
|---|---|---|
| いちばん小さく（デコードは遅い） | `AV1`（既定） | `crf` 25〜35 |
| 速くデコード（学習時に読む頻度が高いなら） | `H264` | `crf` 18〜28 |
| その中間 | `H265` | `crf` 22〜32 |
| AMD / Intel GPU で速くエンコード | `H264_VAAPI` `H265_VAAPI` `AV1_VAAPI` | `qp` |
| NVIDIA GPU で速くエンコード | `H264_NVENC` `H265_NVENC` `AV1_NVENC` | `qp` |

品質ノブは 2 系統です。ソフトウェアコーデックは `video_config` 直下の `crf`、ハードウェアコーデックは `codec_config` 内の `qp`。どちらも小さいほど高品質・大きいファイルです。ハードウェアコーデックの既定 `qp` は、プロジェクトの録画で「元と見分けがつかない」水準（VMAF 93 以上）を満たすよう実測して選んだ値なので、まずは既定のままで大丈夫です。

`crf` は 1 つの共有フィールドで、既定の `"34"` は AV1 向けの値です。`H264` / `H265` に切り替えたら、上の表の範囲に合わせて `crf` も設定してください。

### 共通フィールド

最小の指定はフレームレートとコーデックだけです。

```yaml
video_config:
  fps: 30
  codec_config:
    codec: AV1
```

- `fps`（整数、既定 `100`）: 動画のフレームレート。同期の `fps` と同じ値にします。違ってもエラーにならず、行と動画がずれます。
- `gop`（整数、既定 `20`）: キーフレーム間隔。小さいほどフレーム単位の読み出しが速く、ファイルは大きくなります。
- `crf`（文字列、既定 `"34"`）: ソフトウェアコーデックの品質。ハードウェアコーデックは無視します。
- `scaling`（既定 `Bicubic`）: `resize` 時の補間。`Bilinear` / `FastBilinear` は速く、`Lanczos` は鮮明です。
- `resize`（任意、`{width, height}`）: 出力サイズ（両方とも正の偶数）。縦横比を保たずに引き伸ばします。省略すると元のサイズのまま。

### コーデック別の設定値

`codec_config.codec` でコーデックを選び、同じブロックにそのコーデックの設定を書きます。キーの綴り（ハイフンかアンダースコアか）は[キーの綴り](#キーの綴り)を見てください。

`AV1`（ソフトウェア・SVT-AV1）:

| キー | 範囲 | 既定 | 意味 |
|---|---|---|---|
| `preset` | 0〜13 | 10 | エンコードの手間。小さいほど遅く、同じ `crf` でファイルが小さい |
| `lp` | 0〜6 | 自動 | 並列度（おおよそスレッド数） |
| `lookahead` | −1〜120 | 未設定 | 先読みフレーム数。増やすと品質が上がり、メモリを使う |
| `film-grain` | 0〜50 | 未設定 | 粒状感の再合成。実写 8 前後でサイズが下がる |
| `film-grain-denoise` | true/false | 未設定 | `film-grain` 使用時に、合成前の入力をデノイズする |
| `fast-decode` | 0〜2 | 未設定 | 再生側のデコードを軽くする |
| `pin` | 0〜N | 未設定 | Encoder のスレッドを先頭 N コアに固定（0 で無効） |

`H264`（x264）と `H265`（x265）:

| キー | 既定 | 意味 |
|---|---|---|
| `preset` | `medium` | `ultrafast` `superfast` `veryfast` `faster` `fast` `medium` `slow` `slower` `veryslow`。速いほど大きいファイル |
| `tune` | なし | 内容に合わせた調整のリスト。知覚系は 1 つまで（H264: `film` `animation` `grain` `stillimage` `psnr` `ssim`、H265: `psnr` `ssim` `grain` `animation`）、`fastdecode` / `zerolatency` は併用可 |
| `threads` | 自動 | スレッド数 |
| `frame-threads`（H265 のみ） | 自動 | フレーム並列数 |

VA-API（AMD / Intel）。共通キーは `qp`（品質）、`device`（既定 `/dev/dri/renderD128`）、`profile`、`b-depth`（0〜7）、`async-depth`（1〜64）:

| `codec` | `qp` 範囲・既定 | 補足 |
|---|---|---|
| `H264_VAAPI` | 0〜51、21 | `profile` 既定 `high`、`async-depth` 既定 16 |
| `H265_VAAPI` | 0〜51、29 | B フレームなし（AMD のハードウェア制約） |
| `AV1_VAAPI` | 0〜255、110 | 品質は `-global_quality` として渡されます |

NVENC（NVIDIA）。共通キーは `qp`、`gpu`（GPU 番号）、`preset`（`P1` 最速〜`P7` 最高圧縮）、`tune`（`Hq` / `Ll` / `Ull`）、`profile`、`b_frames`（0〜7）、`rc_lookahead`（0〜120）:

| `codec` | `qp` 範囲・既定 | `preset` | `b_frames` |
|---|---|---|---|
| `H264_NVENC` | 0〜51、26 | `P5` | 1 |
| `H265_NVENC` | 0〜51、25 | `P4` | 0 |
| `AV1_NVENC` | 0〜255、130 | `P7` | 0 |

`H264_NVENC` だけは `tune` 既定 `Hq`、`profile` 既定 `high`、`rc_lookahead` 既定 32 で、他の 2 つは未設定が既定です。`b_frames` を 0 より大きくするときは、`gop` を `b_frames + 1` より大きくしてください。

## 深度動画

深度はミリメートルの距離値なので、見た目用の圧縮で値が歪むと使えなくなります。そのため RGB とは別に `depth_config` で設定し、2 つの方式から選びます。

- `FFV1`（無損失）: 16 bit の距離値をそのまま保ちます。ファイルは大きく、コンテナは `.mkv` です。
- それ以外（10 bit 量子化）: `[1, depth_max_mm]` を 10 bit に割り付けます。刻みは約 `depth_max_mm / 1023`、既定の 4092 mm なら約 4 mm。0 と範囲外は「無効」になります。ファイルは小さく `.mp4` です。

```yaml
depth_config:
  fps: 30
  codec_config:
    codec: FFV1
```

- `depth_max_mm`（整数、既定 `4092`）: 量子化で残す最大距離。近距離の精度を上げるなら下げ、遠くまで要るなら上げます。`FFV1` では無視。
- `fps`（整数、既定 `30`）
- `codec_config`: 下の表から選びます。深度に `gop` はありません。

| `codec` | 品質ノブ（範囲） | 既定 |
|---|---|---|
| `FFV1` | なし（無損失） | |
| `AV1`（ソフトウェア） | `crf`（0〜63） | 4（`preset` は 4） |
| `H265_VAAPI` | `qp`（0〜51） | 18 |
| `AV1_VAAPI` | `global_quality`（0〜255） | 35 |
| `H265_NVENC` | `qp`（0〜51） | 10 |
| `AV1_NVENC` | `qp`（0〜255） | 20 |

量子化した値が壊れないよう、必要な Encoder 設定（フルレンジ指定など）は rebake が自動で付けます。動画ファイルの中身がどうなっているかは[中間フォーマット](intermediate-format_ja.md#動画)へ。

## export の既定値

`rebake-cli export` は `--video-config` なしのとき、コーデックごとにスループット寄りの値で上書きします。表に無い項目は YAML の既定と同じです。

| `--codec` | crf | gop | その他 |
|---|---|---|---|
| `av1` | 34 | 20 | |
| `h264` | 15 | 2 | `preset` `fast` |
| `h265` | 18 | 100 | `preset` `superfast`、`frame-threads` 6 |
| `av1_vaapi` | | 100 | 品質 124（YAML 既定の 110 と異なります） |
| `h264_vaapi` | | 20 | |
| `h265_vaapi` | | 100 | |
| `h264_nvenc` | | 20 | |
| `h265_nvenc` | | 100 | |
| `av1_nvenc` | | 20 | |

`--qp` は `h264_vaapi` と NVENC 3 種の品質を上書きします（`av1_vaapi` / `h265_vaapi` は固定）。深度は `--depth-qp` が NVENC の `qp` を上書きします。それ以外を変えたいときは `config/export/` のサンプルを `--video-config` で渡してください。

## キーの綴り

動画・コーデック設定は未知のキーを拒否するので、綴りがそのままエラーになります。

| 対象 | 書き方 | 別表記 |
|---|---|---|
| AV1 / VA-API / x265 の複合語キー（`film-grain` `fast-decode` `frame-threads` `b-depth` `async-depth`） | ハイフン | なし（アンダースコアは拒否） |
| NVENC の複合語キー（`b_frames` `rc_lookahead`） | アンダースコア | ハイフンも可 |
| ソフトウェアの `preset` / `tune` の値 | 小文字（`medium` `film`） | 先頭大文字も可 |
| NVENC の `preset` / `tune` の値 | `P1`〜`P7`、`Hq` / `Ll` / `Ull` | 小文字も可 |
| `scaling` の値 | 先頭大文字（`Bicubic`） | なし |

値の範囲は YAML の読み込み時ではなく、ステージの実行時に検査されます。

## ハードウェアの要件

ハードウェアコーデックは環境の `ffmpeg` コマンド経由で動くので、対応する Encoder 入りの ffmpeg と、コンテナから見える GPU デバイスが必要です。

- VA-API: H.264 / H.265 は AMD RDNA 2 以降か Intel Gen 8 以降、AV1 は AMD RDNA 3 以降か Intel Arc。
- NVENC: H.264 / H.265 は Maxwell（GTX 900 番台）以降、AV1 は Ada Lovelace（RTX 4000 番台）以降。

コンテナの準備手順は[ハードウェアアクセラレーション](hardware_ja.md)へ。

## うまくいかないとき

| 症状 | 原因 | 直し方 |
|---|---|---|
| 動画と行がずれる（エラーなし） | `video_config.fps` が同期の `fps` と違う | 同じ値にする |
| 設定のキーが拒否される | 綴り・ハイフンとアンダースコアの取り違え | [キーの綴り](#キーの綴り)の表で確認 |
| 実行の途中でステージが失敗する | 値が範囲外 | 各表の範囲に収める |
| AMD の AV1 で画面端に黒帯 | RDNA 3（VCN 4）の幅パディングの既知問題 | 幅を 64 の倍数に `resize` するか、コーデックを変える |

ハードウェアが見つからない系（`/dev/dri` が無いなど）は[ハードウェアアクセラレーション](hardware_ja.md)へ。
