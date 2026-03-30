#Requires -RunAsAdministrator
<#
.SYNOPSIS
    nexterm-server の Windows Service 登録を解除します。

.EXAMPLE
    .\uninstall-service.ps1
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ServiceName = 'NextermServer'

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if (-not $existing) {
    Write-Host "サービス '$ServiceName' は登録されていません。"
    exit 0
}

Write-Host "サービス '$ServiceName' を停止・削除します..."

if ($existing.Status -eq 'Running') {
    Stop-Service -Name $ServiceName -Force
    Start-Sleep -Seconds 2
}

sc.exe delete $ServiceName | Out-Null

Write-Host "✅ '$ServiceName' のアンインストールが完了しました。"
