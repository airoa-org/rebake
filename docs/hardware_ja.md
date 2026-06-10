# ハードウェアアクセラレーション

rebake は既定では CPU でエンコードし（SVT-AV1 / x264 / x265）、GPU は不要です。このページは、VA-API（AMD / Intel）か NVENC（NVIDIA）のコーデックを使うときの、コンテナの準備手順です。どのコーデックを選ぶかは[エンコード](encoding_ja.md#コーデックを選ぶ)へ。

## VA-API（AMD / Intel）

ホストに `/dev/dri/renderD128` があることを確かめてから（`ls /dev/dri/renderD*`）、VA-API 用の compose 設定を重ねて起動します。

```bash
cd docker
docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
docker compose exec rebake-dev bash
```

イメージには AMD / Intel 両方の VA-API ドライバが入っており、既定では libva が `/dev/dri` のデバイスに合わせて自動選択します。自動選択に失敗するときは、デバイスを提供している GPU に合わせて明示します（CPU のメーカーではなく、`/dev/dri` を出している GPU / iGPU で選びます）。

- `radeonsi`: AMD の GPU / iGPU
- `iHD`: 新しめの Intel GPU / iGPU
- `i965`: `iHD` が使えない古い Intel GPU / iGPU

```bash
LIBVA_DRIVER_NAME=radeonsi docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
```

## NVENC（NVIDIA）

必要なものは 3 つです。NVENC 対応の NVIDIA ドライバ（H.264 / H.265 は GTX 900 番台以降、AV1 は RTX 4000 番台以降）、[NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html)、そしてホストで生成した CDI デバイス定義です。

```bash
sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml
nvidia-ctk cdi list
```

ホストに `nvidia-cdi-refresh` の systemd ユニットがある場合（`systemctl list-unit-files | grep nvidia-cdi-refresh` で確認）、ドライバ更新後も定義が追従するよう有効化しておきます。

```bash
sudo systemctl enable --now nvidia-cdi-refresh.path
```

NVENC 用イメージはベースイメージの上に重ねるので、先にベースをビルドしてから NVENC の compose 設定で起動します。

```bash
cd docker
docker compose -f docker-compose.yml build rebake-dev
docker compose -f docker-compose.yml -f docker-compose.nvenc.yml up -d --build
docker compose exec rebake-dev bash
```

コンテナから GPU が見えないときは、ホストで次の 2 つを確かめます。

```bash
nvidia-ctk cdi list
docker run --rm --device nvidia.com/gpu=all rebake:nvenc nvidia-smi
```
