class Nexterm < Formula
  desc "GPU-accelerated terminal multiplexer with SSH, SFTP, Lua scripting"
  homepage "https://github.com/mizu-jun/Nexterm"
  version "0.5.4"

  on_macos do
    on_arm do
      url "https://github.com/mizu-jun/Nexterm/releases/download/v#{version}/nexterm-v#{version}-macos-arm64.tar.gz"
      sha256 "PLACEHOLDER_ARM64_SHA256"
    end
    on_intel do
      url "https://github.com/mizu-jun/Nexterm/releases/download/v#{version}/nexterm-v#{version}-macos-x86_64.tar.gz"
      sha256 "PLACEHOLDER_X86_64_SHA256"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/mizu-jun/Nexterm/releases/download/v#{version}/nexterm-v#{version}-linux-x86_64.tar.gz"
      sha256 "PLACEHOLDER_LINUX_SHA256"
    end
  end

  def install
    bin.install "nexterm-server"
    bin.install "nexterm-client-gpu"
    bin.install "nexterm-client-tui"
    bin.install "nexterm-ctl"
    bin.install "nexterm" if File.exist?("nexterm")
  end

  def caveats
    <<~EOS
      Start the nexterm server:
        nexterm-server &
      Or use the launcher (starts server automatically):
        nexterm
    EOS
  end

  test do
    system "#{bin}/nexterm-ctl", "--help"
  end
end
