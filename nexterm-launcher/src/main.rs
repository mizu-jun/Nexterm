#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Nexterm ランチャー
//!
//! `nexterm` コマンド 1本でサーバーを自動起動し、GPU クライアントを開く。
//!
//! 動作フロー:
//! 1. nexterm-server が既に動作しているか確認（IPC ソケット/パイプの存在チェック）
//! 2. 動作していなければバックグラウンドで nexterm-server を起動
//! 3. サーバーの準備完了を待ってから nexterm-client-gpu を起動
//! 4. クライアントが終了してもサーバーは継続動作させる

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn main() {
    if let Err(e) = run() {
        show_error(&format!("nexterm: {}", e));
        std::process::exit(1);
    }
}

/// エラーメッセージを表示する。
/// リリースビルド（Windows）では stderr が無効なため MessageBox を使う。
#[cfg(all(windows, not(debug_assertions)))]
fn show_error(msg: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = OsStr::new(msg)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let title: Vec<u16> = OsStr::new("Nexterm")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: Windows API の正規呼び出し。ポインタはスタック上で有効
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            title.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn show_error(msg: &str) {
    eprintln!("{}", msg);
}

fn run() -> anyhow::Result<()> {
    let exe_dir = exe_dir()?;

    // サーバーが未起動なら起動する
    if !server_is_running() {
        start_server(&exe_dir)?;
        wait_for_server(Duration::from_secs(10))?;
    }

    // GPU クライアントを起動する
    start_client(&exe_dir)?;

    Ok(())
}

// ---- サーバー起動確認 ----

/// IPC エンドポイントの存在でサーバーが動作中かどうかを判定する
fn server_is_running() -> bool {
    #[cfg(unix)]
    {
        let uid = libc_getuid();
        let runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
        std::path::Path::new(&format!("{}/nexterm.sock", runtime_dir)).exists()
    }

    #[cfg(windows)]
    {
        // Named Pipe が存在すればサーバーが起動中
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
        let pipe = format!("\\\\.\\pipe\\nexterm-{}", username);
        named_pipe_exists(&pipe)
    }

    #[cfg(not(any(unix, windows)))]
    false
}

#[cfg(unix)]
fn libc_getuid() -> libc::uid_t {
    // SAFETY: getuid() は常に成功する
    unsafe { libc::getuid() }
}

#[cfg(windows)]
fn named_pipe_exists(pipe_name: &str) -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = OsStr::new(pipe_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: Windows API の正規呼び出し
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return false;
    }

    // SAFETY: 有効なハンドルを閉じる
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(handle);
    }
    true
}

// ---- サーバー起動 ----

fn start_server(exe_dir: &PathBuf) -> anyhow::Result<()> {
    let server = server_exe(exe_dir);

    if !server.exists() {
        return Err(anyhow::anyhow!(
            "nexterm-server が見つかりません: {}",
            server.display()
        ));
    }

    #[cfg(windows)]
    {
        // Windows: CREATE_NEW_CONSOLE で新しいコンソールウィンドウを持たせずバックグラウンド起動
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new(&server)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| anyhow::anyhow!("nexterm-server の起動に失敗: {}", e))?;
    }

    #[cfg(not(windows))]
    {
        Command::new(&server)
            .spawn()
            .map_err(|e| anyhow::anyhow!("nexterm-server の起動に失敗: {}", e))?;
    }

    Ok(())
}

/// サーバーが準備完了するまで最大 `timeout` 待機する
fn wait_for_server(timeout: Duration) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        if server_is_running() {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            return Err(anyhow::anyhow!(
                "nexterm-server の起動がタイムアウトしました（{}秒）",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---- クライアント起動 ----

fn start_client(exe_dir: &PathBuf) -> anyhow::Result<()> {
    let client = client_exe(exe_dir);

    if !client.exists() {
        // GPU クライアントがなければ TUI クライアントで代替
        let tui = tui_exe(exe_dir);
        if tui.exists() {
            eprintln!("nexterm-client-gpu が見つかりません。TUI クライアントを起動します");
            return exec_replace(&tui);
        }
        return Err(anyhow::anyhow!(
            "nexterm-client-gpu も nexterm-client-tui も見つかりません"
        ));
    }

    exec_replace(&client)
}

/// 現在のプロセスを指定の実行ファイルで置き換える（exec）
fn exec_replace(exe: &PathBuf) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(exe).exec();
        Err(anyhow::anyhow!("exec に失敗: {}", err))
    }

    #[cfg(not(unix))]
    {
        // Windows には exec() がないため spawn + wait で代替
        let status = Command::new(exe)
            .status()
            .map_err(|e| anyhow::anyhow!("クライアントの起動に失敗: {}", e))?;
        if !status.success() {
            eprintln!("nexterm-client-gpu が非ゼロで終了しました: {}", status);
        }
        Ok(())
    }
}

// ---- パスヘルパー ----

fn exe_dir() -> anyhow::Result<PathBuf> {
    std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("実行ファイルのパス取得に失敗: {}", e))?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("実行ファイルの親ディレクトリが取得できません"))
}

fn server_exe(dir: &PathBuf) -> PathBuf {
    #[cfg(windows)]
    return dir.join("nexterm-server.exe");
    #[cfg(not(windows))]
    dir.join("nexterm-server")
}

fn client_exe(dir: &PathBuf) -> PathBuf {
    #[cfg(windows)]
    return dir.join("nexterm-client-gpu.exe");
    #[cfg(not(windows))]
    dir.join("nexterm-client-gpu")
}

fn tui_exe(dir: &PathBuf) -> PathBuf {
    #[cfg(windows)]
    return dir.join("nexterm-client-tui.exe");
    #[cfg(not(windows))]
    dir.join("nexterm-client-tui")
}
