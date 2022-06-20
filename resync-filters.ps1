curl.exe --url https://raw.githubusercontent.com/abba23/spotify-adblock/main/config.toml --output .\filter.toml
Set-Content -Path .\filter.toml -Value ("# source: https://github.com/abba23/spotify-adblock/blob/main/config.toml`n`n" + (Get-Content .\filter.toml -Raw))
