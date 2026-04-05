class Nexterm < Formula
  desc "GPU-accelerated terminal multiplexer with SSH, SFTP, Lua scripting"
  homepage "https://github.com/mizu-jun/Nexterm"
  version "0.5.5"

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
    # `nexterm` が唯一の起動口。サーバーを自動起動して GPU クライアントを開く。
    bin.install "nexterm"
    # 補助バイナリ（直接利用する上級者向け）
    bin.install "nexterm-server"
    bin.install "nexterm-client-gpu"
    bin.install "nexterm-client-tui"
    bin.install "nexterm-ctl"
    # macOS: .app バンドルがあれば /Applications にも配置する
    if OS.mac? && File.directory?("Nexterm.app")
      prefix.install "Nexterm.app"
      system "ln", "-sf", "#{prefix}/Nexterm.app", "/Applications/Nexterm.app"
    end
  end

  def caveats
    if OS.mac?
      <<~EOS
        Nexterm をインストールしました。

        【起動方法 — どれか 1 つを選ぶ】
          1. Finder から: /Applications/Nexterm.app をダブルクリック
          2. Terminal から: nexterm

        nexterm コマンド 1 本でサーバーの自動起動と GPU クライアントの
        起動を行います。個別バイナリを手動で起動する必要はありません。

        ──────────────────────────────────────────
        Nexterm has been installed.

        [How to launch — choose one]
          1. From Finder: double-click /Applications/Nexterm.app
          2. From Terminal: nexterm

        The `nexterm` command starts the server automatically and opens
        the GPU client. You do not need to start individual binaries.
      EOS
    else
      <<~EOS
        Nexterm をインストールしました。

        【起動方法】
          nexterm

        nexterm コマンド 1 本でサーバーの自動起動と GPU クライアントの
        起動を行います。

        ──────────────────────────────────────────
        Nexterm has been installed.

        [How to launch]
          nexterm

        The `nexterm` command starts the server automatically and opens
        the GPU client.
      EOS
    end
  end

  test do
    system "#{bin}/nexterm-ctl", "--help"
  end
end
