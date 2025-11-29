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

## Solutions

1. **Recommended for local development on Apple Silicon**: Use Fly.io deployment
2. **Alternative**: Run only the Bulk Payment API against testnet
3. **Full testing**: Use a native x86_64 Linux environment or Intel Mac

## Testing Command Used

Created `sandbox/Dockerfile.debug` to isolate the sandbox for investigation.
