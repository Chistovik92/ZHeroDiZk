ZHeroDiZk client for Linux - TEST BUILD
=======================================

This is a test build for people who help check the project. It is NOT a finished product.

What you must know
- It does not enforce managed session grants yet: whoever knows the ID and the permanent
  password of a computer can connect to it (the same as the original software it is built on).
  Do not use it on computers with sensitive data or on the public internet.
- It has no default server. Open Settings -> Network and enter the address and the key of YOUR
  ZHeroDiZk server (rendezvous/relay). Without them the client shows "not ready".
- It has not been run on the target computers by the author. Expect problems and report them.
- Needs an X11 session (Wayland is not supported for remote control) and libxdo for the mouse:
  run  zherodizk --doctor  (or  python3 zherodizk.py --doctor  in the tarball) to check.
  On Simply Linux / ALT:  su -  &&  apt-get install xdotool
- The configuration lives in ~/.config/zherodizk/ and does not touch other remote-desktop programs.

Start
  deb/rpm package:  zherodizk
  tarball:          python3 zherodizk.py     (from the unpacked directory)

Licence and source code
- AGPL-3.0-only for the new code; third-party notices are in NOTICE. The complete corresponding
  source code (this repository at the release tag, plus the pinned upstream sources named in
  upstream.lock.json) is at https://github.com/Chistovik92/ZHeroDiZk
- Author: SecretHero (Telegram @SecretHero).
