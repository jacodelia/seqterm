class Seqterm < Formula
  desc "Terminal-based modular step sequencer / DAW"
  homepage "https://github.com/jacodelia/seqterm"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_MACOS_ARM64"
    else
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_MACOS_X86_64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"
    else
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_X86_64"
    end
  end

  def install
    bin.install "seqterm"
    man1.install "seqterm.1" if File.exist?("seqterm.1")
  end

  def caveats
    <<~EOS
      SeqTerm uses ALSA (Linux) or CoreAudio (macOS) for audio output.
      For low-latency operation on Linux, consider installing JACK/PipeWire.

      Quick start:
        seqterm                   # launch with default settings
        seqterm --help            # show options

      SF2 SoundFonts:
        Place .sf2 files anywhere and open them from within SeqTerm
        using the SF2 browser (Ctrl+O on a matrix clip).
    EOS
  end

  test do
    system "#{bin}/seqterm", "--version"
  end
end
