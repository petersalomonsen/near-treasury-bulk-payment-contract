#!/bin/bash
# Start sandbox with appropriate binary for the architecture

ARCH=$(uname -m)
VERSION="2.9.0"

case "$ARCH" in
    aarch64|arm64)
        export SANDBOX_ARTIFACT_URL="https://s3-us-west-1.amazonaws.com/build.nearprotocol.com/nearcore/Linux-aarch64/${VERSION}/near-sandbox.tar.gz"
        echo "Using Linux ARM64 sandbox binary"
        ;;
    x86_64)
        export SANDBOX_ARTIFACT_URL="https://s3-us-west-1.amazonaws.com/build.nearprotocol.com/nearcore/Linux-x86_64/${VERSION}/near-sandbox.tar.gz"
        echo "Using Linux x86_64 sandbox binary"
        ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

exec /usr/local/bin/sandbox-init "$@"
