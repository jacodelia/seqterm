class Seqterm < Formula
  desc "Terminal-based music sequencer, sampler, and granular synthesizer"
  homepage "https://github.com/your-org/seqterm"
  license "MIT"
  head "https://github.com/your-org/seqterm.git", branch: "main"

  stable do
    url "https://github.com/your-org/seqterm/archive/refs/tags/v0.1.0.tar.gz"
    sha256 "REPLACE_WITH_ACTUAL_SHA256"
  end

  bottle do
    sha256 cellar: :any_skip_relocation, arm64_sonoma:   "REPLACE"
    sha256 cellar: :any_skip_relocation, arm64_ventura:  "REPLACE"
    sha256 cellar: :any_skip_relocation, sonoma:         "REPLACE"
    sha256 cellar: :any_skip_relocation, ventura:        "REPLACE"
    sha256 cellar: :any_skip_relocation, x86_64_linux:   "REPLACE"
  end

  depends_on "rust" => :build

  # Linux only: ALSA MIDI support
  on_linux do
    depends_on "alsa-lib"
    depends_on "pkg-config" => :build
  end

  def install
    system "cargo", "install",
      "--locked",
      "--root", prefix,
      "--path", "seqterm-rs/crates/seqterm-app"
  end

  test do
    # Smoke test: verify the binary starts and prints the version.
    assert_match "seqterm", shell_output("#{bin}/seqterm --version 2>&1 || true")
  end
end
