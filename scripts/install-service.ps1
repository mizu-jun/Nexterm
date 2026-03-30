#Requires -RunAsAdministrator
<#
.SYNOPSIS
    nexterm-server を Windows Service として登録します。

.DESCRIPTION
    nexterm-server.exe をログイン不要で自動起動する Windows Service として登録します。
    サービス名: NextermServer
    起動種別: 自動（OS 起動時に自動開始）

.PARAMETER InstallDir
    nexterm の実行ファイルが置かれたディレクトリ。
    省略時はこのスクリプトと同じディレクトリを使用します。

.EXAMPLE
    .\install-service.ps1
    .\install-service.ps1 -InstallDir "C:\Program Files\Nexterm"
#>

param(
    [string]$InstallDir = $PSScriptRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ServiceName    = 'NextermServer'
$DisplayName    = 'Nexterm Terminal Server'
$Description    = 'Nexterm multiplexed terminal server — manages PTY sessions and IPC.'
$ServerExe      = Join-Path $InstallDir 'nexterm-server.exe'

# ---- 前提チェック ----

if (-not (Test-Path $ServerExe)) {
    Write-Error "nexterm-server.exe が見つかりません: $ServerExe`nインストール先を -InstallDir で指定してください。"
    exit 1
}

# ---- 既存サービスの確認 ----

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "既存のサービス '$ServiceName' を停止・削除します..."
    if ($existing.Status -eq 'Running') {
        Stop-Service -Name $ServiceName -Force
        Start-Sleep -Seconds 2
    }
    sc.exe delete $ServiceName | Out-Null
    Start-Sleep -Seconds 1
}

# ---- サービス登録 ----

Write-Host "サービス '$ServiceName' を登録します..."
Write-Host "  実行ファイル: $ServerExe"

New-Service `
    -Name        $ServiceName `
    -BinaryPathName "`"$ServerExe`"" `
    -DisplayName $DisplayName `
    -Description $Description `
    -StartupType Automatic | Out-Null

# ---- ログオンアカウントをローカルサービスに設定 ----
# nexterm-server は Named Pipe を使うため LocalSystem でなく LocalService で十分
sc.exe config $ServiceName obj= "NT AUTHORITY\LocalService" | Out-Null

# ---- 失敗時の自動再起動設定 ----
sc.exe failure $ServiceName reset= 60 actions= restart/5000/restart/10000/restart/30000 | Out-Null

# ---- サービス起動 ----

Write-Host "サービスを起動します..."
Start-Service -Name $ServiceName
$svc = Get-Service -Name $ServiceName
Write-Host "サービス状態: $($svc.Status)"

Write-Host ""
Write-Host "✅ '$DisplayName' のインストールが完了しました。"
Write-Host "   OS 起動時に自動で開始されます。"
Write-Host ""
Write-Host "管理コマンド:"
Write-Host "  Stop-Service $ServiceName      # 停止"
Write-Host "  Start-Service $ServiceName     # 起動"
Write-Host "  Restart-Service $ServiceName   # 再起動"
Write-Host "  .\uninstall-service.ps1        # アンインストール"
