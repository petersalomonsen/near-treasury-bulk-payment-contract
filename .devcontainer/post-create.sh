#!/bin/bash

# Add wasm32 target for NEAR contract compilation
rustup target add wasm32-unknown-unknown

# Install cargo-near using the official installer
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/near/cargo-near/releases/latest/download/cargo-near-installer.sh | sh

sudo apt update
sudo apt install -y pkg-config
