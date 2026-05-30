# ハードウェアアクセラレーションによるエンコード

[English](hardware.md)

rebake は既定で動画を CPU エンコード (SVT-AV1 / x264 / x265) し、GPU は不要です。以下の
codec は任意のアクセラレータであり、対応ハードウェアがある場合のみ有効化してください。

## VA-API ハードウェアエンコーディング (AMD / Intel)

GPU アクセラレーションによる動画エンコード (AMD/Intel) を使用するには、VA-API override ファイルを指定します:

```bash
cd docker
docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
docker compose exec rebake-dev bash
```

前提条件:
- ホストに `/dev/dri/renderD128` が存在すること (`ls /dev/dri/renderD*`)

Docker image には AMD/Intel 用の VA-API driver を含めています。既定では `LIBVA_DRIVER_NAME` を設定せず、libva がマウントされた `/dev/dri` デバイスから適切な driver を自動選択します。自動判定が外れる場合だけ、起動時に driver 名を明示してください。

driver は CPU ベンダーだけではなく、`/dev/dri` を提供している GPU/iGPU に合わせて選びます:

- `radeonsi`: AMD GPU/iGPU
- `iHD`: 新しめの Intel GPU/iGPU
- `i965`: `iHD` が対応しない古い Intel GPU/iGPU

```bash
LIBVA_DRIVER_NAME=radeonsi docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
LIBVA_DRIVER_NAME=iHD docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
```

## NVIDIA NVENC ハードウェアエンコーディング

NVIDIA GPU でアクセラレーションによる動画エンコードを使用するには、次の前提条件を満たしておく必要があります:

- NVENC をサポートする NVIDIA driver。H.264 と H.265 NVENC は Maxwell 世代以降のほとんどの NVIDIA GPU で動作します（GTX 900 シリーズ以降）。AV1 NVENC を利用する場合は、加えて Ada Lovelace 世代以降の GPU が必要です（RTX 4000 シリーズ以降）
- [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) がインストールされていること
- host 側で NVIDIA CDI devices が生成されていること:

```bash
sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml
nvidia-ctk cdi list
```

host に `nvidia-cdi-refresh` の systemd unit がある場合（`systemctl list-unit-files | grep nvidia-cdi-refresh` で確認）、driver/toolkit 更新後に CDI spec が自動更新されるよう path unit を有効化してください:

```bash
sudo systemctl enable --now nvidia-cdi-refresh.path
```

これらが整ったら、NVENC の compose override で dev container を起動します。`Dockerfile.nvenc` はベースイメージ `rebake:latest` の上に layer を重ねる構造のため、まずベースイメージを build してから override を起動します:

```bash
cd docker
docker compose -f docker-compose.yml build rebake-dev
docker compose -f docker-compose.yml -f docker-compose.nvenc.yml up -d --build
docker compose exec rebake-dev bash
```

dev container から GPU が見えない場合は、host 側で次を実行して CDI と build 済みイメージが正しく組み合わさっているかを確認できます:

```bash
nvidia-ctk cdi list
docker run --rm --device nvidia.com/gpu=all rebake:nvenc nvidia-smi
```
