# Build artifacts only: never package an existing environment or installed service binaries.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'

node scripts/release-version.mjs check
if ($LASTEXITCODE -ne 0) { throw 'Release version check failed.' }
$version = (Get-Content -Raw package.json | ConvertFrom-Json).version
$prefix = "Open-Larager_${version}_windows-x64"
$executable = 'src-tauri/target/release/open-larager.exe'
if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'Build the desktop application first.' }
$installers = @(Get-ChildItem -LiteralPath 'src-tauri/target/release/bundle/nsis' -Filter '*-setup.exe' -File)
if ($installers.Count -ne 1) { throw 'Expected exactly one NSIS installer from a clean build.' }
if ($installers[0].Name -ne "Open Larager_${version}_x64-setup.exe") {
  throw 'The NSIS installer does not match the application version and x64 architecture.'
}

# Do not overwrite assets from a previous local build; choose a clean checkout or move them first.
$output = New-Item -ItemType Directory -Path 'release-assets'
$staging = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) "open-larager-release-$([guid]::NewGuid())")
Copy-Item -LiteralPath $installers[0].FullName -Destination (Join-Path $output.FullName "$prefix-setup.exe")
Copy-Item -LiteralPath $executable -Destination (Join-Path $staging.FullName 'open-larager.exe')
[IO.File]::WriteAllText((Join-Path $staging.FullName 'larager.portable'), '')
[IO.File]::WriteAllText((Join-Path $staging.FullName 'README.txt'), @'
Open Larager - Windows 11 x64

Extract this entire ZIP to a writable folder before opening open-larager.exe.
Keep larager.portable beside the executable to store your environment in larager-data.
WebView2 Runtime must already be installed. Some optional packages require Visual C++ x64 Runtime.
To update, stop the app and services, back up your environment, and replace only the executable.
Keep larager-data, larager-webview and larager-environment.json when updating.
Credentials encrypted with Windows DPAPI remain tied to the Windows account and computer.
'@)
Compress-Archive -Path (Join-Path $staging.FullName '*') -DestinationPath (Join-Path $output.FullName "$prefix-portable.zip")

$checksums = Get-ChildItem -LiteralPath $output.FullName -File | Sort-Object Name | ForEach-Object {
  $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  "$hash  $($_.Name)"
}
[IO.File]::WriteAllText((Join-Path $output.FullName 'SHA256SUMS.txt'), ($checksums -join "`n") + "`n")
Copy-Item -LiteralPath '.github/RELEASE_TEMPLATE.md' -Destination (Join-Path $output.FullName 'RELEASE-NOTES.md')

# The staging folder contains only files created above; resolve and check its boundary before cleanup.
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
$stagingPath = [IO.Path]::GetFullPath($staging.FullName)
if (!$stagingPath.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
    !(Split-Path -Leaf $stagingPath).StartsWith('open-larager-release-')) {
  throw 'Unexpected staging path; temporary files were preserved.'
}
Remove-Item -LiteralPath $stagingPath -Recurse -Force
Write-Output "Release downloads and SHA-256 checksums ready in $($output.FullName)"
