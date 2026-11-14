param(
  [Parameter(Mandatory = $true)][string]$OutputDirectory,
  [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$projectDirectory = Split-Path $PSScriptRoot -Parent
$nativeDirectory = Join-Path $projectDirectory 'src-tauri'
$outputPath = [IO.Path]::GetFullPath($OutputDirectory)
if ($outputPath.TrimEnd('\', '/') -eq ([IO.Path]::GetPathRoot($outputPath)).TrimEnd('\', '/')) {
  throw 'OutputDirectory must be an application folder, not a drive root.'
}
if ($outputPath.StartsWith($projectDirectory + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
    $outputPath -eq $projectDirectory) {
  throw 'Choose an output folder outside the source project.'
}
Push-Location $projectDirectory
try {
  if (!$SkipBuild) {
    & node node_modules/react-scripts/bin/react-scripts.js build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed.' }
    & cargo build --manifest-path src-tauri/Cargo.toml --release --offline
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed.' }
  }
  $metadataText = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 --offline
  if ($LASTEXITCODE -ne 0) { throw 'Cannot locate Cargo build output.' }
  $metadata = $metadataText | ConvertFrom-Json
  $executable = Join-Path $metadata.target_directory 'release/ukiyo.exe'
  if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'Release executable is missing.' }
  $configuration = Get-Content (Join-Path $nativeDirectory 'tauri.conf.json') -Raw | ConvertFrom-Json
  # Expand the same resource list used by Tauri; preserve paths beside the executable.
  $resources = @($configuration.tauri.bundle.resources | ForEach-Object {
    Get-ChildItem -Path (Join-Path $nativeDirectory $_) -File
  } | Sort-Object FullName -Unique)
  New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
  Copy-Item -LiteralPath $executable -Destination (Join-Path $outputPath 'ukiyo.exe') -Force
  $packagedFiles = @('ukiyo.exe')
  foreach ($resource in $resources) {
    $relative = $resource.FullName.Substring($nativeDirectory.Length + 1)
    $destination = Join-Path $outputPath $relative
    New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $resource.FullName -Destination $destination -Force
    $packagedFiles += $relative
  }
  # Update the GIMI runtime without touching user data (Mods / User / generated state).
  $gimiSource = Join-Path $nativeDirectory 'assets/gimi'
  if (Test-Path -LiteralPath $gimiSource) {
    $gimiDestination = Join-Path $outputPath 'gimi'
    New-Item -ItemType Directory -Path $gimiDestination -Force | Out-Null
    $gimiFiles = @((Get-Item -LiteralPath (Join-Path $gimiSource '3dmloader.dll')))
    foreach ($folder in @('Licenses', 'Modules', 'Profiles/XXMI')) {
      $source = Join-Path $gimiSource $folder
      if (Test-Path -LiteralPath $source) {
        $gimiFiles += @(Get-ChildItem -LiteralPath $source -Recurse -File | Where-Object {
          $_.Name -notin @('d3dx.ini', 'd3dx_user.ini', 'mod-catalog.ini')
        })
      }
    }
    $gimiHashes = [ordered]@{}
    foreach ($file in $gimiFiles) {
      $relative = $file.FullName.Substring($gimiSource.Length + 1)
      $target = Join-Path $gimiDestination $relative
      New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force | Out-Null
      Copy-Item -LiteralPath $file.FullName -Destination $target -Force
      $gimiHashes[$relative.Replace('\', '/')] = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
      $packagedFiles += ('gimi\' + $relative)
    }
    # Describe exactly the immutable runtime files shipped here. Mods and User
    # are user data; the old standalone launcher's inventory is not this package.
    $runtimeManifest = [ordered]@{
      product = 'Ukiyo XXMI runtime'
      upstream_version = '1.0.14'
      integration_version = $configuration.package.version
      cores = @('XXMI')
      supported_target_versions = @('3.4', '4.7')
      files = $gimiHashes
    }
    $runtimeManifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $gimiDestination 'portable-manifest.json') -Encoding UTF8
    $packagedFiles += 'gimi\portable-manifest.json'
  }

  $manifest = @($packagedFiles | ForEach-Object {
    $file = Get-Item -LiteralPath (Join-Path $outputPath $_)
    [ordered]@{ path = $_; bytes = $file.Length; sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash }
  })
  [ordered]@{ version = $configuration.package.version; files = $manifest } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputPath 'release-manifest.json') -Encoding UTF8
  Write-Host "Portable release: $outputPath"
  Write-Host "Executable: $((Get-Item -LiteralPath (Join-Path $outputPath 'ukiyo.exe')).Length) bytes"
} finally {
  Pop-Location
}