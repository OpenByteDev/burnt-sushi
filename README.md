# <img src="https://github.com/OpenByteDev/burnt-sushi/blob/master/icon.png" height="40px" /> BurntSushi 

[![Build](https://github.com/OpenByteDev/burnt-sushi/actions/workflows/build.yml/badge.svg)](https://github.com/OpenByteDev/burnt-sushi/actions/workflows/build.yml) [![Last Release](https://img.shields.io/github/v/release/OpenByteDev/burnt-sushi?include_prereleases)](https://github.com/OpenByteDev/burnt-sushi/releases/latest/) [![License](https://img.shields.io/github/license/OpenByteDev/burnt-sushi)](https://github.com/OpenByteDev/burnt-sushi/blob/master/LICENSE)

A Spotify AdBlocker for Windows that works via DLL injection and function hooking.

## Installation
The latest version can be downloaded [here](https://github.com/OpenByteDev/burnt-sushi/releases/latest). The app is portable and there is no need for an installation.

## FAQ
### How does it work?
BurntSushi works by intercepting network requests and blocking ones that match a set of [filters](https://github.com/OpenByteDev/burnt-sushi/blob/master/filter.toml). This is implemented by injecting a dynamic library into the Spotify process that overrides [`getaddrinfo`](https://docs.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfo) from the Windows API and `cef_urlrequest_create` from [libcef](https://github.com/chromiumembedded/cef).
The status of the Spotify process is determined using [`wineventhook`](https://github.com/OpenByteDev/wineventhook-rs) which is based on [`SetWinEventHook`](https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook).

### Can this be detected by Spotify?
Theoretically yes, but practically it probably won't.

### Does it work on Linux?
No, BurntSushi supports Windows only, but you can check out [spotify-adblock](https://github.com/abba23/spotify-adblock) instead.

## Credits
Inspired by https://github.com/abba23/spotify-adblock

Original icon made by [Freepik](https://www.freepik.com/) from [flaticon.com](https://www.flaticon.com/)
