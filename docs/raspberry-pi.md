# SeqTerm on Raspberry Pi

SeqTerm is written in pure Rust with no x86-specific SIMD in the hot path, so it runs well on Pi 4 and Pi 5. This guide covers setup, audio latency tuning, and performance tips.

---

## Supported Models

| Model | OS | Arch | Status |
|-------|----|------|--------|
| Pi 5 (4 GB+) | Pi OS 64-bit (Bookworm) | `aarch64` | Recommended — plenty of headroom |
| Pi 4 (4 GB+) | Pi OS 64-bit (Bookworm) | `aarch64` | Good — 8-16 tracks at 512-frame buffer |
| Pi 4 (2 GB) | Pi OS 64-bit | `aarch64` | OK — limit SF2 polyphony, use small SoundFonts |
| Pi 4 / Pi 3 | Pi OS 32-bit | `armhf` | Works — tighter memory budget |
| Pi Zero 2 W | Pi OS 64-bit | `aarch64` | Marginal — single-core; use ≥ 1024-frame buffer |

---

## Install

### From binary (recommended)

```bash
# Pi OS 64-bit
wget https://github.com/your-org/seqterm/releases/latest/download/seqterm_arm64.deb
sudo dpkg -i seqterm_arm64.deb
seqterm

# Pi OS 32-bit
wget https://github.com/your-org/seqterm/releases/latest/download/seqterm_armhf.deb
sudo dpkg -i seqterm_armhf.deb
seqterm
```

### From source on the Pi

```bash
# Install dependencies
sudo apt update
sudo apt install build-essential libasound2-dev pkg-config curl

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Build (takes ~10 min on Pi 4)
git clone https://github.com/your-org/seqterm
cd seqterm/seqterm-rs
cargo build --release -p seqterm-app

# Run
./target/release/seqterm
```

### Cross-compile from x86\_64 (faster)

```bash
# Install cross-compilation toolchain on your dev machine
cargo install cross --git https://github.com/cross-rs/cross

# Build for Pi 64-bit
cross build --release --target aarch64-unknown-linux-gnu -p seqterm-app

# Copy and run
scp target/aarch64-unknown-linux-gnu/release/seqterm pi@raspberrypi.local:~/
ssh pi@raspberrypi.local ./seqterm
```

---

## Audio Setup

SeqTerm uses ALSA directly on the Pi (no PipeWire or JACK required, though both work).

### ALSA configuration

Create or edit `/etc/asound.conf`:

```
# /etc/asound.conf — optimised for SeqTerm on Pi
pcm.!default {
    type hw
    card 0
    device 0
}
ctl.!default {
    type hw
    card 0
}
```

For USB audio interfaces (recommended for low latency):

```
# Replace card 0 with your USB interface index (check: aplay -l)
pcm.!default {
    type hw
    card 1          # adjust for your device
    device 0
    rate 48000
    format S16_LE
}
```

### Buffer size tuning

Start with a 512-frame buffer and reduce if you need lower latency:

```bash
SEQTERM_BUFFER_SIZE=512 SEQTERM_SAMPLE_RATE=48000 seqterm
```

| Buffer (frames) | Latency @48kHz | Suitable for |
|----------------|----------------|-------------|
| 1024 | 21 ms | Safe default; headless sequencer, Pi 4 2 GB |
| 512 | 10.7 ms | Live performance; Pi 4 4 GB |
| 256 | 5.3 ms | Low-latency; Pi 5 only; may xrun on heavy projects |
| 128 | 2.7 ms | Experimental; Pi 5 + USB interface + `chrt` |

The status bar shows `XRUN` counts. If you see frequent xruns, increase the buffer size.

---

## Real-Time Scheduling (optional)

For the lowest latency, give SeqTerm a real-time scheduling priority:

```bash
# Add your user to the audio group (if not already)
sudo usermod -aG audio $USER
# Log out and back in

# Run with FIFO real-time priority 50
sudo chrt -f 50 seqterm
# Or without sudo if /etc/security/limits.d/audio.conf is configured:
chrt -f 50 seqterm
```

To make this automatic, add to `/etc/security/limits.d/99-realtime.conf`:

```
@audio   -  rtprio     95
@audio   -  memlock    unlimited
```

### rtirq (optional)

`rtirq` boosts the priority of audio IRQ threads:

```bash
sudo apt install rtirq-init
sudo systemctl enable rtirq
sudo systemctl start rtirq
```

---

## Performance Tips

### SF2 / SoundFont selection

| SoundFont | Size | Notes |
|-----------|------|-------|
| GeneralUser GS | 1.4 MB | Excellent for Pi — fits in RAM easily |
| TimGM6mb | 6 MB | Good quality, small footprint |
| FluidR3_GM | 141 MB | Pi 4 (4 GB) only; swap disabled recommended |
| Arachno | 148 MB | Pi 5 only |

Keep polyphony low:
```
# In Config view → Audio → Max voices: 64 (Pi 4) / 128 (Pi 5)
```

The audio engine is configured with 256 max voices by default. Reduce this if you see CPU spikes.

### Project complexity guidelines (Pi 4, 512-frame buffer)

| Component | Safe limit |
|-----------|-----------|
| MIDI tracks | 16 |
| Audio slots (SF2 + AudioFile) | 8 |
| Active FX per slot | 3–4 |
| Granular voices | 16 |
| Pattern steps | 64 per pattern |

### Headless operation

SeqTerm works over SSH with a terminal multiplexer:

```bash
# On the Pi: start a persistent session
tmux new-session -s seqterm

# In the tmux session
seqterm

# Detach: Ctrl+B then D
# Reattach from another SSH session:
tmux attach -t seqterm
```

For headless performance without a monitor, use a terminal multiplexer rather than a VNC session — VNC adds latency and consumes GPU memory.

### Disable unnecessary services

```bash
# Disable Bluetooth (saves ~20 MB RAM and reduces IRQ contention)
sudo systemctl disable bluetooth
sudo systemctl stop bluetooth

# Disable Wi-Fi if using ethernet
sudo ifconfig wlan0 down

# Disable GPU memory split if no display
echo "gpu_mem=16" | sudo tee -a /boot/config.txt
```

---

## MIDI on Pi

SeqTerm uses ALSA MIDI natively. List MIDI devices:

```bash
aconnect -l
aplaymidi -l
```

USB MIDI interfaces are plug-and-play. For DIN-5 MIDI, a USB-MIDI adapter works reliably (e.g., Roland UM-ONE).

### Virtual MIDI ports

SeqTerm creates one virtual ALSA port per pattern key on startup. Connect to them from other applications:

```bash
# Connect SeqTerm's port A to a hardware synth on MIDI out
aconnect "SeqTerm:A" "UM-ONE:0"
```

---

## Troubleshooting

**`ALSA: unable to open device`**
```bash
# Check the device name
aplay -l
# Set explicitly
SEQTERM_AUDIO_BACKEND=alsa SEQTERM_ALSA_DEVICE="hw:0,0" seqterm
```

**Xruns / audio glitches**
- Increase buffer size (`SEQTERM_BUFFER_SIZE=1024`)
- Reduce polyphony (Config → Audio → Max voices)
- Use a smaller SoundFont
- Enable `chrt` scheduling (see above)
- Disable Wi-Fi and Bluetooth

**`Error: ALSA: PCM open failed`**
```bash
# Ensure no other application holds the device
fuser /dev/snd/*
# Kill conflicting process or use PulseAudio passthrough
```

**High CPU on Pi 4 with SF2**
- oxisynth (the SF2 engine) is single-threaded. Limit to 1–2 simultaneous SF2 files.
- Use audio file clips for percussive one-shots — they're cheaper than SF2 synthesis.
