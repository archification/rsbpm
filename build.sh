#!/bin/bash

RUSTFLAGS="-Zlocation-detail=none" cross build --target x86_64-unknown-linux-musl --release

upx --best --lzma target/x86_64-unknown-linux-musl/release/rsbpm
