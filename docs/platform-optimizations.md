# Platform-Specific Optimization Recommendations

This document covers SIMD guards, buffer size tuning, and latency targets for each supported platform.

---

## Audio Latency Targets

| Platform | Target latency | Buffer size | Notes |
|---|---|---|---|
| Linux (PipeWire) | 5–10 ms | 256–512 frames | PipeWire manages quantum automatically |
| Linux (JACK) | 2–5 ms | 128–256 frames | `jackd -d alsa -p 256 -n 2` |
| Linux (ALSA) | 10–21 ms | 512–1024 frames | Depends on hardware and kernel |
| macOS (CoreAudio) | 5–10 ms | 256–512 frames | IOAudioEngine handles low-latency |
| Windows (WASAPI) | 10–20 ms | 512–1024 frames | WASAPI Exclusive mode for lower latency |
| Raspberry Pi 4 (ALSA) | 10–21 ms | 512–1024 frames | Requires RT scheduling for < 10 ms |
| Raspberry Pi 5 (ALSA) | 5–10 ms | 256–512 frames | Better CPU, lower latency achievable |

---

## SIMD Strategy

SeqTerm's hot path (audio callback) contains no explicit SIMD intrinsics. The compiler generates SIMD code automatically:

| Feature | Rust cfg | Auto-enabled on |
|---|---|---|
| SSE2 | `target_feature = "sse2"` | All x86\_64 targets |
| AVX2 | `RUSTFLAGS=-C target-cpu=native` | x86\_64 with AVX2 support |
| NEON | Enabled by default on ARM64 | `aarch64-*` targets |
| VFPv4 | `target_feature = "neon"` | ARMv7 with FPU |

For maximum performance on x86\_64:
```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

For ARM64 (Raspberry Pi 5):
```bash
RUSTFLAGS="-C target-cpu=cortex-a76" cargo build --release
```

For ARMv7 (Raspberry Pi 4):
```bash
RUSTFLAGS="-C target-cpu=cortex-a72 -C target-feature=+neon,+vfp4" cargo build --release
```

**Warning:** Do not use `target-cpu=native` in distributed binaries — the binary will crash on older CPUs that don't support the required extensions.

### SIMD guard pattern (for future explicit SIMD)

If explicit SIMD is added in the future (e.g., for the mixer sum loop), guard it with feature detection:

```rust
#[cfg(target_arch = "x86_64")]
fn mix_avx2(a: &[f32], b: &[f32], out: &mut [f32]) {
    #[cfg(target_feature = "avx2")]
    unsafe {
        // AVX2 implementation
    }
    #[cfg(not(target_feature = "avx2"))]
    mix_scalar(a, b, out);
}
```

Or use runtime dispatch via `std::is_x86_feature_detected!("avx2")`.

---

## Linux Optimization

### PipeWire

PipeWire selects buffer size automatically based on the quantum setting. To request a specific quantum:

```bash
PIPEWIRE_QUANTUM=256/48000 seqterm   # 256 frames at 48 kHz ≈ 5.3 ms
PIPEWIRE_QUANTUM=512/48000 seqterm   # 512 frames ≈ 10.7 ms
```

SeqTerm reads `PIPEWIRE_QUANTUM` from the environment and passes it to the JACK bridge automatically (via `AudioEngineConfig.pipewire_quantum`).

### JACK

For the lowest possible JACK latency:
```bash
jackd -R -d alsa -d hw:0 -r 48000 -p 128 -n 3
SEQTERM_AUDIO_BACKEND=jack seqterm
```

The `-R` flag enables realtime scheduling. `-p 128 -n 3` gives 3 × 128-frame periods ≈ 8 ms round-trip.

### Real-time scheduling

```bash
# Option 1: chrt (run as root or with CAP_SYS_NICE)
chrt -f 70 seqterm

# Option 2: limits.conf (persistent, no sudo required after reboot)
# Add to /etc/security/limits.d/99-seqterm.conf:
#   @audio - rtprio 95
#   @audio - memlock unlimited
sudo usermod -aG audio $USER
# Log out and back in, then:
chrt -f 70 seqterm
```

---

## macOS Optimization

CoreAudio handles scheduling and buffer sizing. SeqTerm runs at the system-selected buffer size. To reduce it, set in System Preferences → Sound → Output → Use low latency audio (varies by macOS version).

For M1/M2/M3 (Apple Silicon), the `aarch64-apple-darwin` binary uses NEON automatically and runs very efficiently — no additional configuration needed.

---

## Windows Optimization

WASAPI Exclusive Mode gives the lowest latency on Windows (bypasses the audio session layer):

```bash
# SeqTerm uses CPAL which uses WASAPI. Exclusive mode is enabled automatically
# when no other application holds the device.
SEQTERM_BUFFER_SIZE=256 seqterm.exe
```

For lower latency, close all other audio applications before launching SeqTerm.

Windows Defender real-time protection can cause audio dropouts. Add the seqterm binary directory to the exclusion list.

---

## Build Profile Recommendations

### Development (`dev`)

Default cargo profile. Fast compilation, slower runtime. Suitable for development.

### Release (`release`)

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 4
strip = false   # keep debug info for profiling
```

Suitable for testing and benchmarking.

### Distribution (`release-lto`)

```toml
[profile.release-lto]
inherits = "release"
lto = "fat"
codegen-units = 1
strip = true
```

Used for the GitHub Actions release artifacts. Takes longer to build but produces the fastest binary.

### Raspberry Pi (`rpi`)

```toml
[profile.rpi]
inherits = "release"
opt-level = "z"   # size over speed — Pi has limited storage
strip = true
panic = "abort"   # smaller binary, no unwinding
```

The rpi profile reduces binary size by ~30% compared to the standard release profile.

---

## Memory Budget

| Component | Memory usage (approximate) |
|---|---|
| seqterm binary | ~8 MB (stripped release) |
| oxisynth + GeneralUser GS SF2 | ~12 MB |
| oxisynth + FluidR3 SF2 | ~160 MB |
| 32 audio slots (pre-allocated) | ~4 MB |
| rtatui TUI buffers | ~2 MB |
| Total (minimal) | ~26 MB |
| Total (full GM2 SF2) | ~180 MB |

On Raspberry Pi 4 (4 GB RAM), the typical working set is 30–80 MB depending on SF2 file size, leaving plenty of room for the OS and other applications.
