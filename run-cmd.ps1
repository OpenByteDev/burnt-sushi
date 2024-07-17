param ([string] $command)

$PSNativeCommandUseErrorActionPreference = $true
$ErrorActionPreference = 'Stop'

$IncludeTarget = $true
if ($command -eq "fmt") {
    $IncludeTarget = $false
}

cargo +nightly $command --manifest-path=shared/Cargo.toml $(if ($IncludeTarget) { "--target" } else { "" }) $(if ($IncludeTarget) { "i686-pc-windows-msvc" } else { "" }) $args 
cargo +nightly $command --manifest-path=shared/Cargo.toml $(if ($IncludeTarget) { "--target" } else { "" }) $(if ($IncludeTarget) { "x86_64-pc-windows-msvc" } else { "" }) $args 
cargo +nightly $command --manifest-path=burnt-sushi-blocker/Cargo.toml $(if ($IncludeTarget) { "--target" } else { "" }) $(if ($IncludeTarget) { "i686-pc-windows-msvc" } else { "" }) $args 
cargo +nightly $command --manifest-path=burnt-sushi/Cargo.toml $(if ($IncludeTarget) { "--target" } else { "" }) $(if ($IncludeTarget) { "x86_64-pc-windows-msvc" } else { "" }) $args 

