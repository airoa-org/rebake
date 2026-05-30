FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    autoconf \
    automake \
    build-essential \
    ca-certificates \
    clang \
    curl \
    git \
    libclang-dev \
    libtool \
    libnuma-dev \
    meson \
    libdav1d-dev \
    libxml2-dev \
    nasm \
    ninja-build \
    pkg-config \
    python3 \
    wget \
    xz-utils \
    yasm \
    cmake \
    vim \
    # VA-API hardware acceleration support
    libva-dev \
    libva-drm2 \
    libdrm-dev \
    mesa-va-drivers \
    intel-media-va-driver \
    vainfo \
    && rm -rf /var/lib/apt/lists/*

# Build and install libvmaf
RUN VMAF_VER="v3.0.0" && \
    git clone --depth 1 --branch ${VMAF_VER} https://github.com/Netflix/vmaf.git /tmp/vmaf && \
    cd /tmp/vmaf/libvmaf && \
    meson setup build --buildtype=release --prefix=/usr \
      -Denable_tests=false \
      -Denable_docs=false \
      -Denable_asm=true \
      -Dbuilt_in_models=true && \
    ninja -C build -j"$(nproc)" && \
    ninja -C build install && \
    ldconfig && \
    mkdir -p /usr/share/model && \
    cp -a /tmp/vmaf/model/*.json /usr/share/model/ && \
    rm -rf /tmp/vmaf

## Build and install x264 from source (fast H.264; keep asm)
RUN git clone --depth 1 https://code.videolan.org/videolan/x264.git /tmp/x264 && \
    cd /tmp/x264 && \
    ./configure \
      --prefix=/usr \
      --enable-shared \
      --enable-pic \
      --disable-cli && \
    make -j"$(nproc)" && \
    make install && \
    ldconfig && \
    rm -rf /tmp/x264

## Build and install x265 from source (8-bit only, Release, assembly on, NUMA on)
RUN X265_VER="4.1" && \
    curl -L "https://bitbucket.org/multicoreware/x265_git/downloads/x265_${X265_VER}.tar.gz" | tar xz -C /tmp && \
    cd /tmp/x265_${X265_VER}/build/linux && \
    cmake -G Ninja ../../source \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DENABLE_SHARED=ON \
      -DENABLE_CLI=OFF \
      -DEXPORT_C_API=ON \
      -DENABLE_ASSEMBLY=ON \
      -DENABLE_PIC=ON \
      -DENABLE_LIBNUMA=ON \
      -DHIGH_BIT_DEPTH=OFF \
      -DMAIN12=OFF \
      -DCMAKE_C_FLAGS="-O3 -pipe" \
      -DCMAKE_CXX_FLAGS="-O3 -pipe" && \
    ninja -j"$(nproc)" && ninja install && \
    ldconfig && \
    rm -rf /tmp/x265_${X265_VER}

# Build and install SVT-AV1
RUN SVT_VER="v3.1.2" && \
    curl -L "https://gitlab.com/AOMediaCodec/SVT-AV1/-/archive/${SVT_VER}/SVT-AV1-${SVT_VER}.tar.gz" | tar xz -C /tmp && \
    cd /tmp/SVT-AV1-${SVT_VER} && \
    cmake -G Ninja -B build -S . \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DBUILD_SHARED_LIBS=ON && \
    cmake --build build --target install && \
    ldconfig && \
    rm -rf /tmp/SVT-AV1-${SVT_VER}

# Build and install FFmpeg with multi-codec support (AV1, H.264, H.265) and VA-API
RUN FF_VER="n8.0" && \
    curl -L "https://github.com/FFmpeg/FFmpeg/archive/refs/tags/${FF_VER}.tar.gz" | tar xz -C /tmp && \
    cd /tmp/FFmpeg-${FF_VER} && \
    ./configure \
    --prefix=/usr \
    --enable-shared \
    --disable-static \
    --disable-doc \
    --disable-ffplay \
    --enable-gpl \
    --enable-version3 \
    --enable-libvmaf \
    --enable-libsvtav1 \
    --enable-libx264 \
    --enable-libx265 \
    --enable-libdav1d \
    --enable-vaapi \
    --enable-libdrm \
    --extra-cflags="-O3 -pipe" && \
    make -j"$(nproc)" && \
    make install && \
    ldconfig && \
    rm -rf /tmp/FFmpeg-${FF_VER}

# Install Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path
ENV PATH="/root/.cargo/bin:${PATH}"

# Install just (task runner)
RUN cargo install just

# Install uv
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:${PATH}"

ENV LIBCLANG_PATH="/usr/lib/llvm-18/lib"

CMD ["tail", "-f", "/dev/null"]
