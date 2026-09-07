# ratablet

[简体中文][zh]

[zh]: README.zh-CN.md

Use a reMarkable tablet as a pen tablet on Linux, Windows, or macOS.

ratablet runs in the system tray by default. Select the tray icon to open the control panel. The panel shows the device model, connection state, rotation, pen mapping, and settings. Its language follows the system locale and can also be selected manually.

The settings page accepts a device IP address or SSH target. An IP address or hostname is expanded to the `root` user. The initial target is `root@10.11.99.1`. Authentication starts with SSH keys. Saved passwords use Linux Secret Service, Windows Credential Manager, or macOS Keychain. An authentication failure pauses reconnection and opens the password dialog.

SSH authenticates the device and starts the remote reader. Pen events return over a temporary TCP connection authenticated by a one-time session token, avoiding SSH's high-frequency buffering. The device stays unchanged and the remote reader exits with the SSH session. A wakelock keeps the tablet awake while connected. Reconnection continues after a disconnect until the process receives Ctrl+C. The event stream is not encrypted, so use USB or a trusted network.

## Supported devices

- reMarkable 1 and 2
- reMarkable Paper Pro — Ferrari
- reMarkable Paper Pro Move — Chiappa
- reMarkable Paper Pro Pure — Tatsu

Input devices are discovered by name. Explicit calibration values support additional models. RM1 and RM2 use native landscape coordinates. The Paper Pro family uses a 90-degree rotation. `--rotate` selects a fixed orientation and `--auto-rotate` follows the Paper Pro rotation sensor.

## Run

Short Version: Download → Connect Remarkable Tablet → Input Password → Done

Long Version:

Enable SSH on the reMarkable and install an OpenSSH client on the host.

```sh
ratablet
ratablet --key ~/.ssh/remarkable
ratablet --host root@192.168.1.50 --rotate 270
ratablet --auto-rotate
ratablet --headless
```

SSH uses an isolated known-hosts configuration and automatically accepts the device host key. This favors easy reconnection after a device reset over protection from a machine-in-the-middle attack.

Linux requires write access to `/dev/uinput`, commonly granted through a distribution uinput group or udev rule. The tray uses StatusNotifierItem. Desktops without tray support open the control panel directly. KDE Plasma Wayland uses its native applet popup behavior.

Windows uses the Synthetic Pen API included with Windows 10 version 1809 and later. It supplies position, pressure, tilt, hover, and eraser data to Windows Ink applications.

macOS uses CoreGraphics event injection. Grant ratablet access under System Settings → Privacy & Security → Accessibility. Pen input maps to the primary display.

Override input discovery when needed:

```sh
ratablet --device /dev/input/event2
```

Provide calibration values for an additional model:

```sh
ratablet --max-x 11180 --max-y 15340 --max-pressure 4096
```

Run `ratablet --help` for every option.

## Build

```sh
cargo build --release
```

Cross-compile for Windows with the Rust GNU target and MinGW-w64:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

Cross-compile for macOS with osxcross and its `target/bin` directory on `PATH`:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./package-macos.sh
```

The script uses the standard osxcross aliases `oa64-clang`, `o64-clang`, and `lipo`. Linker environment variables can override the first two. The output is `dist/ratablet.app`, a universal ARM64 and x86_64 application bundle.

## License

GPL-3.0-or-later. Commercial use is allowed. Distributors must provide the corresponding source under the same license. Private use does not trigger a source distribution requirement.
