class Seqterm < Formula
  desc "Terminal-based modular sequencer / DAW"
  homepage "https://github.com/jacodelia/seqterm"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_MACOS_ARM64"
    else
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_MACOS_X86"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"
    else
      url "https://github.com/jacodelia/seqterm/releases/download/v#{version}/seqterm-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_X86"
    end
  end

  # Optional: build from source instead of using pre-built binaries.
  # head do
  #   url "https://github.com/jacodelia/seqterm.git", branch: "main"
  #   depends_on "rust" => :build
  # end

  # Runtime optional: FluidSynth for higher-quality SF2 synthesis.
  depends_on "fluid-synth" => :optional

  def install
    bin.install "seqterm"
    man1.install "seqterm.1" if File.exist? "seqterm.1"
  end

  def post_install
    # Create default config directory.
    (var/"seqterm").mkpath
  end

  test do
    # Smoke test: print version and exit.
    assert_match version.to_s, shell_output("#{bin}/seqterm --version 2>&1")
  end
end
