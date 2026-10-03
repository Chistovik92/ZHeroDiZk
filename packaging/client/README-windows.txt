ZHeroDiZk client for Windows - TEST BUILD
=========================================

This is a test build for people who help check the project. It is NOT a finished product.

What you must know
- Portable: unpack the whole archive into a folder and run zherodizk.exe. There is no installer,
  no service and no auto-start. It is NOT code-signed, so Windows SmartScreen will warn you.
- It does not enforce managed session grants yet: whoever knows the ID and the permanent
  password of a computer can connect to it (the same as the original software it is built on).
  Do not use it on computers with sensitive data or on the public internet.
- It has no default server. Open Settings -> Network and enter the address and the key of YOUR
  ZHeroDiZk server (rendezvous/relay). Without them the client shows "not ready".
- It has not been run on Windows by the author. Expect problems and report them.
- It needs the Microsoft Visual C++ 2015-2022 runtime (x64). Without a service or the installer,
  it cannot control elevated windows (UAC) or the lock screen.
- The configuration is kept separately from other remote-desktop programs.

Licence and source code
- AGPL-3.0-only for the new code; third-party notices are in NOTICE. The complete corresponding
  source code (this repository at the release tag, plus the pinned upstream sources named in
  upstream.lock.json) is at https://github.com/Chistovik92/ZHeroDiZk
- Author: SecretHero (Telegram: https://t.me/SecretHero).
