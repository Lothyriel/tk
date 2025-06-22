#!/bin/bash
cargo build --bin client --target=x86_64-pc-windows-gnu

cargo run --bin server &
mv_win ./target/x86_64-pc-windows-gnu/debug/client.exe
