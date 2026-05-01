#!/bin/bash

RUSTFLAGS="-Zlocation-detail=none" cross build --target x86_64-unknown-linux-musl --release
cargo xwin build --release --target x86_64-pc-windows-msvc

upx --best --lzma target/x86_64-unknown-linux-musl/release/rsbpm
strip target/x86_64-pc-windows-msvc/release/rsbpm.exe
upx --best --lzma target/x86_64-pc-windows-msvc/release/rsbpm.exe
