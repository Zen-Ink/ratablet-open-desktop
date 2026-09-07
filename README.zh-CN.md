# ratablet

[English][en]

[en]: README.md

将 reMarkable 用作 Linux、Windows 或 macOS 数位板。

ratablet 默认以系统托盘应用运行。点击托盘图标可打开控制面板。面板显示设备型号、连接状态、旋转方向、笔映射和设置。界面语言跟随系统，也可手动选择简体中文或英文。

设置页接受设备 IP 地址或 SSH 目标。输入 IP 地址或主机名时会自动补全 `root` 用户。初始目标为 `root@10.11.99.1`。身份验证优先使用 SSH 密钥。保存的密码进入 Linux Secret Service、Windows Credential Manager 或 macOS Keychain。身份验证失败会暂停重连并打开密码对话框。

SSH 负责验证设备并启动远端读取进程。笔事件通过带一次性会话令牌的临时 TCP 连接返回，避开 SSH 对高频数据的批量缓冲。设备端保持原样，远端读取进程随 SSH 会话退出。连接期间 wakelock 会让平板保持唤醒。连接断开后程序持续重连，按 Ctrl+C 可结束进程。事件流本身不加密，请使用 USB 或可信网络。

## 支持设备

- reMarkable 1 和 2
- reMarkable Paper Pro — Ferrari
- reMarkable Paper Pro Move — Chiappa
- reMarkable Paper Pro Pure — Tatsu

输入节点按名称自动发现。显式校准值可支持更多型号。RM1 和 RM2 使用原生横屏坐标。Paper Pro 系列采用 90 度旋转。`--rotate` 可选择固定方向，`--auto-rotate` 可跟随 Paper Pro 旋转传感器。

## 运行

简短版本：下载 → 连接 reMarkable 平板 → 输入密码 → 完成

详细版本：

在 reMarkable 上开启 SSH，并在宿主机安装 OpenSSH 客户端。

```sh
ratablet
ratablet --key ~/.ssh/remarkable
ratablet --host root@192.168.1.50 --rotate 270
ratablet --auto-rotate
ratablet --headless
```

SSH 使用独立的 known-hosts 配置，并自动接受设备主机密钥。此配置侧重设备重置后的连接便利性，主机密钥验证功能处于关闭状态。

Linux 需要当前用户拥有 `/dev/uinput` 写权限，可通过发行版的 uinput 用户组或 udev 规则授权。托盘使用 StatusNotifierItem。缺少托盘支持的桌面会直接打开控制面板。KDE Plasma Wayland 使用原生 applet popup 行为。

Windows 使用 Windows 10 1809 及后续版本自带的 Synthetic Pen API，可向 Windows Ink 应用提供位置、压力、倾斜、悬停和橡皮擦数据。

macOS 使用 CoreGraphics 事件注入。请在系统设置 → 隐私与安全性 → 辅助功能中授权 ratablet。笔输入映射到主显示器。

输入发现失败时可指定节点：

```sh
ratablet --device /dev/input/event2
```

新型号可使用显式校准值：

```sh
ratablet --max-x 11180 --max-y 15340 --max-pressure 4096
```

运行 `ratablet --help` 可查看全部参数。

## 构建

```sh
cargo build --release
```

Windows 交叉构建需要 Rust GNU target 和 MinGW-w64：

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

macOS 交叉构建需要 osxcross，并将其 `target/bin` 目录加入 `PATH`：

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./package-macos.sh
```

脚本使用 osxcross 的稳定别名 `oa64-clang`、`o64-clang` 和 `lipo`。前两个工具可通过 linker 环境变量覆盖。输出为 `dist/ratablet.app`，其中包含 ARM64 和 x86_64 通用应用包。

## 许可证

项目采用 GPL-3.0-or-later。商业使用受到许可。发布软件时须按同一许可证提供对应源代码。私人使用无需发布源代码。
