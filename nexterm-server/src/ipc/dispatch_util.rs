//! IPC ディスパッチ層の共通ヘルパ — 録画パス検証など

/// 録音出力パスのバリデーション（ディレクトリトラバーサル攻撃を防ぐ）
pub(super) fn validate_recording_path(output_path: &str) -> anyhow::Result<()> {
    use std::path::{Component, Path};
    if output_path.is_empty() {
        return Err(anyhow::anyhow!("出力パスが空です"));
    }
    if Path::new(output_path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(anyhow::anyhow!(
            "セキュリティエラー: パスに '..' を含めることはできません: {}",
            output_path
        ));
    }

    let allowed = allowed_recording_dirs();
    let input_path = Path::new(output_path);

    if input_path.is_absolute() {
        let parent = input_path.parent().unwrap_or(input_path);
        let is_allowed = allowed.iter().any(|dir| parent.starts_with(dir));
        if !is_allowed {
            let first_allowed = &allowed[0];
            std::fs::create_dir_all(first_allowed).ok();
            return Err(anyhow::anyhow!(
                "セキュリティエラー: 録音ファイルは {} または {} 内に保存してください (指定パス: {})",
                allowed[0].display(),
                allowed
                    .get(1)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                output_path
            ));
        }
        std::fs::create_dir_all(parent)?;
    }

    Ok(())
}

/// 録音ファイルを保存できる許可ディレクトリ一覧を返す
pub(super) fn allowed_recording_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let rec_dir = std::path::PathBuf::from(home)
            .join("nexterm")
            .join("recordings");
        std::fs::create_dir_all(&rec_dir).ok();
        dirs.push(rec_dir);
    }

    let tmp_base = std::env::var_os("TMPDIR")
        .or_else(|| std::env::var_os("TEMP"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let tmp_dir = tmp_base.join("nexterm");
    std::fs::create_dir_all(&tmp_dir).ok();
    dirs.push(tmp_dir);

    #[cfg(unix)]
    {
        let unix_tmp = std::path::PathBuf::from("/tmp/nexterm");
        std::fs::create_dir_all(&unix_tmp).ok();
        dirs.push(unix_tmp);
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn パストラバーサルを含むパスは拒否される() {
        assert!(validate_recording_path("../../etc/passwd").is_err());
        assert!(validate_recording_path("../secret.txt").is_err());
        assert!(validate_recording_path("foo/../bar.txt").is_err());
    }

    #[test]
    fn 正常なパスは通過する() {
        assert!(validate_recording_path("recording.txt").is_ok());
        #[cfg(unix)]
        assert!(validate_recording_path("/tmp/nexterm/session.rec").is_ok());
        #[cfg(windows)]
        {
            let tmp = std::env::var("TEMP")
                .or_else(|_| std::env::var("TMP"))
                .unwrap_or_else(|_| "C:\\Temp".to_string());
            let allowed = format!("{}\\nexterm\\session.rec", tmp);
            assert!(validate_recording_path(&allowed).is_ok());
        }
    }

    #[test]
    fn 許可外の絶対パスは拒否される() {
        #[cfg(unix)]
        {
            assert!(validate_recording_path("/home/user/recording.txt").is_err());
            assert!(validate_recording_path("/etc/passwd").is_err());
        }
        #[cfg(windows)]
        {
            assert!(validate_recording_path("D:\\secret\\recording.txt").is_err());
            assert!(validate_recording_path("C:\\Windows\\System32\\recording.txt").is_err());
        }
    }

    #[test]
    fn 空パスは拒否される() {
        assert!(validate_recording_path("").is_err());
    }

    #[test]
    fn 単一のドットは許可される() {
        // カレントディレクトリ参照は許可
        assert!(validate_recording_path("./recording.txt").is_ok());
    }

    #[test]
    fn 複数階層の有効なパス() {
        assert!(validate_recording_path("2024/01/session.log").is_ok());
        assert!(validate_recording_path("recordings/subdir/file.txt").is_ok());
    }

    #[test]
    fn 隠しファイル名は許可される() {
        // Unixの隠しファイル
        assert!(validate_recording_path(".hidden").is_ok());
        assert!(validate_recording_path("dir/.hidden").is_ok());
    }

    #[test]
    fn 特殊文字を含むパスは許可される() {
        assert!(validate_recording_path("file-with-dashes.txt").is_ok());
        assert!(validate_recording_path("file_with_underscores.txt").is_ok());
    }

    #[test]
    fn allowed_recording_dirs_returns_temp() {
        let dirs = allowed_recording_dirs();
        assert!(!dirs.is_empty());
        // Unixでは /tmp が含まれる
        #[cfg(unix)]
        assert!(dirs.iter().any(|d| d.to_str().unwrap().contains("tmp")));
    }
}
