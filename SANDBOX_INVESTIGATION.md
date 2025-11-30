# Sandbox Investigation Report

## Problem Statement
The NEAR sandbox fails to initialize in Docker containers running on Apple Silicon (M1/M2/M3) with the error:
```
Error: Failed to start sandbox
Caused by:
    0: Error while performing r/w on config file: No such file or directory (os error 2)
```

## Investigation Process

### Step 1: Initial Diagnosis
- Suspected file system permission issues
- Tested with various directory configurations
- Verified `/data` directory was writable (created core dumps)

### Step 2: Deep Inspection with strace
Ran the sandbox with system call tracing:
```bash
docker run --platform linux/amd64 -v sandbox_debug:/data near-treasury-sandbox-debug \
  /bin/bash -c "strace -e trace=open,openat,stat,access /usr/local/bin/sandbox-init"
```

**Key Finding**: The strace output revealed repeated mode switches:
```
[ Process PID=11 runs in 64 bit mode. ]
[ Process PID=11 runs in x32 mode. ]
[ Process PID=11 runs in 64 bit mode. ]
[ Process PID=11 runs in x32 mode. ]
```

### Step 3: Exit Code Analysis
The sandbox process exits with status 132, which translates to:
- Signal 4 (SIGILL - Illegal Instruction)
- This means the CPU/emulator encountered an instruction it doesn't support

### Step 4: Root Cause Identification

The `near-sandbox` crate downloads a precompiled binary from NEAR's infrastructure:
```
https://s3-us-west-1.amazonaws.com/build.nearprotocol.com/nearcore/Linux-x86_64/2.9.0/near-sandbox.tar.gz
```

This binary is compiled with **x32 ABI** (x86_64 architecture with 32-bit pointers for smaller memory footprint).

**x32 ABI Details**:
- Uses x86-64 instruction set but with 32-bit pointers
- Optimized for memory efficiency
- **Not supported by Rosetta translation layer**

**Rosetta Translator Limitations**:
- Colima's Rosetta implementation translates ARM64 → x86_64
- It only supports standard x86_64 ABI (64-bit pointers)
- When the binary tries to execute x32-specific instructions, Rosetta cannot translate them
- Result: SIGILL (Illegal Instruction) signal → Process termination

## Conclusion

**This is NOT a Docker configuration issue**, but a **fundamental platform incompatibility**:

1. ✅ The Docker image builds correctly for linux/amd64
2. ✅ The application binaries (Bulk Payment API, Sputnik Indexer) run fine
3. ✅ File I/O operations work correctly
4. ❌ The NEAR sandbox binary cannot execute under Rosetta due to x32 ABI instructions

## Solution: Native ARM64 Support

NEAR provides a Linux ARM64 binary at:
```
https://s3-us-west-1.amazonaws.com/build.nearprotocol.com/nearcore/Linux-aarch64/2.9.0/near-sandbox.tar.gz
```

The `near-sandbox-utils` crate supports overriding the download URL via the `SANDBOX_ARTIFACT_URL` environment variable. We created a wrapper script (`start-sandbox.sh`) that:
1. Detects the CPU architecture at runtime
2. Sets `SANDBOX_ARTIFACT_URL` to the appropriate binary (Linux-aarch64 or Linux-x86_64)
3. Runs the sandbox-init binary

This allows the Docker image to work natively on both:
- **Apple Silicon (M1/M2/M3)**: Uses `linux/arm64` image with Linux-aarch64 sandbox binary
- **Intel/AMD**: Uses `linux/amd64` image with Linux-x86_64 sandbox binary

## Testing Commands

Build and run natively on Apple Silicon:
```bash
# Build for native ARM64 (no emulation needed)
docker build -f sandbox/Dockerfile -t near-treasury-sandbox .

# Run the container
docker run -d \
  --name near-treasury-sandbox \
  -p 3030:3030 \
  -p 8080:8080 \
  -p 5001:5001 \
  -v sandbox_data:/data \
  near-treasury-sandbox
```
