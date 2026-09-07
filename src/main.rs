#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::env;
use std::error::Error;
use std::io::{self, BufRead, BufReader, Read};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

mod gui;

#[cfg(target_os = "linux")]
#[path = "platform_linux.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "platform_windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
compile_error!("ratablet supports Linux, Windows, and macOS only");

const REMOTE_SCRIPT: &str = r#"
set -eu
machine=$(cat /sys/devices/soc0/machine 2>/dev/null || cat /proc/device-tree/model 2>/dev/null || printf unknown)
if [ -n "$RAT_DEVICE" ]; then
    device=$RAT_DEVICE
else
    device=
    for name_file in /sys/class/input/event*/device/name; do
        IFS= read -r name < "$name_file" || continue
        case "$name" in
            *marker*|*Marker*|*wacom*|*Wacom*|*digitizer*|*Digitizer*)
                event=${name_file#/sys/class/input/}
                event=${event%%/*}
                device=/dev/input/$event
                break
                ;;
        esac
    done
fi
[ -n "$device" ] && [ -r "$device" ] || { printf 'pen input device not found\n' >&2; exit 1; }
set -- ${SSH_CLIENT:-}
[ "$#" -ge 1 ] || { printf 'SSH client address unavailable\n' >&2; exit 1; }
client=$1
command -v nc >/dev/null || { printf 'netcat unavailable\n' >&2; exit 1; }
case "$(uname -m)" in
    aarch64|x86_64|riscv64) bits=64 ;;
    *) bits=32 ;;
esac
printf 'RAT1\t%s\t%s\t%s\n' "$bits" "$device" "$machine"
lock=ratablet-$$
locked=0
child=
orient_child=
if [ -w /sys/power/wake_lock ]; then
    printf '%s' "$lock" > /sys/power/wake_lock
    locked=1
fi
cleanup() {
    trap - EXIT HUP INT TERM
    [ -z "$child" ] || kill "$child" 2>/dev/null || :
    [ -z "$orient_child" ] || kill "$orient_child" 2>/dev/null || :
    [ "$locked" -eq 0 ] || printf '%s' "$lock" > /sys/power/wake_unlock 2>/dev/null || :
}
trap cleanup EXIT HUP INT TERM
if [ "$RAT_AUTO" = 1 ]; then
    ax=
    ay=
    for candidate in /sys/bus/iio/devices/iio:device*/in_accel_x_raw; do
        [ -r "$candidate" ] || continue
        candidate_y=${candidate%/*}/in_accel_y_raw
        [ -r "$candidate_y" ] || continue
        ax=$candidate
        ay=$candidate_y
        break
    done
    if [ -n "$ax" ]; then
        (
            while :; do
                IFS= read -r x < "$ax" || exit
                IFS= read -r y < "$ay" || exit
                printf 'RATACCEL\t%s\t%s\n' "$x" "$y" >&2
                sleep 0.1
            done
        ) &
        orient_child=$!
    else
        printf 'ratablet: rotation sensor unavailable; keeping landscape\n' >&2
    fi
fi
(
    printf '%s' "$RAT_TOKEN"
    cat "$device"
) | nc "$client" "$RAT_PORT" &
child=$!
# The host keeps SSH stdin open as a session-lifetime pipe. EOF means the
# host or connection is gone, so cleanup can stop the otherwise-blocking reader.
cat >/dev/null
"#;

#[cfg(target_os = "windows")]
const KNOWN_HOSTS_SINK: &str = "NUL";
#[cfg(not(target_os = "windows"))]
const KNOWN_HOSTS_SINK: &str = "/dev/null";

#[derive(Debug, Clone)]
struct Args {
    host: String,
    host_explicit: bool,
    key: Option<PathBuf>,
    device: Option<String>,
    max_x: Option<i32>,
    max_y: Option<i32>,
    max_pressure: Option<i32>,
    rotate: Option<u16>,
    auto_rotate: bool,
    headless: bool,
    verbose: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            host: "root@10.11.99.1".into(),
            host_explicit: false,
            key: None,
            device: None,
            max_x: None,
            max_y: None,
            max_pressure: None,
            rotate: None,
            auto_rotate: false,
            headless: false,
            verbose: false,
        }
    }
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = Self::default();
        let mut words = env::args().skip(1);
        while let Some(word) = words.next() {
            let value = |words: &mut std::iter::Skip<env::Args>, flag: &str| {
                words
                    .next()
                    .ok_or_else(|| format!("{flag} requires a value"))
            };
            match word.as_str() {
                "--host" => {
                    args.host = value(&mut words, "--host")?;
                    args.host_explicit = true;
                }
                "--key" => args.key = Some(value(&mut words, "--key")?.into()),
                "--device" => args.device = Some(value(&mut words, "--device")?),
                "--max-x" => args.max_x = Some(parse_positive(&value(&mut words, "--max-x")?)?),
                "--max-y" => args.max_y = Some(parse_positive(&value(&mut words, "--max-y")?)?),
                "--max-pressure" => {
                    args.max_pressure = Some(parse_positive(&value(&mut words, "--max-pressure")?)?)
                }
                "--rotate" => {
                    let rotate = value(&mut words, "--rotate")?
                        .parse()
                        .map_err(|_| "--rotate must be 0, 90, 180, or 270".to_string())?;
                    if !matches!(rotate, 0 | 90 | 180 | 270) {
                        return Err("--rotate must be 0, 90, 180, or 270".into());
                    }
                    args.rotate = Some(rotate);
                }
                "--auto-rotate" => args.auto_rotate = true,
                "--headless" => args.headless = true,
                "-v" | "--verbose" => args.verbose = true,
                "-h" | "--help" => return Err(usage()),
                _ => return Err(format!("unknown option: {word}\n\n{}", usage())),
            }
        }
        if let Some(device) = &args.device {
            let suffix = device.strip_prefix("/dev/input/event").unwrap_or("");
            if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
                return Err("--device must look like /dev/input/event2".into());
            }
        }
        args.host = normalize_host(&args.host)?;
        if args.auto_rotate && args.rotate.is_some() {
            return Err("--auto-rotate and --rotate cannot be used together".into());
        }
        Ok(args)
    }
}

fn normalize_host(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err("SSH target must not be empty or contain spaces".into());
    }
    let target = if value.contains('@') {
        value.to_owned()
    } else {
        format!("root@{value}")
    };
    let (user, host) = target
        .split_once('@')
        .ok_or_else(|| "SSH target must be an IP, hostname, or user@host".to_string())?;
    if user.is_empty() || host.is_empty() || host.starts_with('-') || host.contains('@') {
        return Err("SSH target must be an IP, hostname, or user@host".into());
    }
    Ok(target)
}

fn parse_positive(value: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("expected a positive integer, got {value:?}"))
}

fn usage() -> String {
    "Usage: ratablet [--host root@10.11.99.1] [--key FILE] [--device /dev/input/eventN]\n\
     \x20                [--max-x N --max-y N --max-pressure N]\n\
     \x20                [--auto-rotate | --rotate 0|90|180|270] [--headless] [-v]"
        .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RotationMode {
    Landscape,
    Auto,
    Fixed(u16),
}

impl RotationMode {
    fn from_args(args: &Args) -> Self {
        if args.auto_rotate {
            Self::Auto
        } else if let Some(degrees) = args.rotate {
            Self::Fixed(degrees)
        } else {
            Self::Landscape
        }
    }

    fn encode(self) -> u16 {
        match self {
            Self::Landscape => 0,
            Self::Auto => 1,
            Self::Fixed(0) => 2,
            Self::Fixed(90) => 3,
            Self::Fixed(180) => 4,
            Self::Fixed(270) => 5,
            Self::Fixed(_) => unreachable!(),
        }
    }

    fn decode(value: u16) -> Self {
        match value {
            1 => Self::Auto,
            2 => Self::Fixed(0),
            3 => Self::Fixed(90),
            4 => Self::Fixed(180),
            5 => Self::Fixed(270),
            _ => Self::Landscape,
        }
    }

    fn rotation(self, info: &DeviceInfo, sensed: u16) -> u16 {
        match self {
            Self::Landscape => info.landscape_rotation(),
            Self::Auto if sensed != u16::MAX => sensed,
            Self::Auto => info.landscape_rotation(),
            Self::Fixed(degrees) => degrees,
        }
    }
}

#[derive(Debug, Clone)]
enum ConnectionStatus {
    Connecting,
    Waiting(String),
    Connected(DeviceInfo),
}

struct SharedState {
    connection: Mutex<ConnectionStatus>,
    rotation_mode: AtomicU16,
    revision: AtomicU64,
    effective_rotation: AtomicU16,
    host: Mutex<String>,
    password: Mutex<Option<String>>,
    stop: AtomicBool,
}

impl SharedState {
    fn new(rotation_mode: RotationMode, host: String, password: Option<String>) -> Self {
        Self {
            connection: Mutex::new(ConnectionStatus::Connecting),
            rotation_mode: AtomicU16::new(rotation_mode.encode()),
            revision: AtomicU64::new(0),
            effective_rotation: AtomicU16::new(u16::MAX),
            host: Mutex::new(host),
            password: Mutex::new(password),
            stop: AtomicBool::new(false),
        }
    }

    fn rotation_mode(&self) -> RotationMode {
        RotationMode::decode(self.rotation_mode.load(Ordering::Relaxed))
    }

    fn set_rotation_mode(&self, mode: RotationMode) {
        if self.rotation_mode.swap(mode.encode(), Ordering::Relaxed) != mode.encode() {
            self.reconnect();
        }
    }

    fn set_password(&self, password: Option<String>) {
        *self.password.lock().unwrap() = password;
        self.reconnect();
    }

    fn set_host(&self, host: String, password: Option<String>) {
        *self.host.lock().unwrap() = host;
        *self.password.lock().unwrap() = password;
        self.reconnect();
    }

    fn reconnect(&self) {
        self.set_connection(ConnectionStatus::Connecting);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn set_connection(&self, status: ConnectionStatus) {
        *self.connection.lock().unwrap() = status;
    }

    fn set_waiting(&self, error: String) {
        let mut connection = self.connection.lock().unwrap();
        if !matches!(&*connection, ConnectionStatus::Waiting(current) if same_error_type(current, &error))
        {
            *connection = ConnectionStatus::Waiting(error);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputEvent {
    kind: u16,
    code: u16,
    value: i32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct PenState {
    x: i32,
    y: i32,
    pressure: i32,
    distance: i32,
    tilt_x: i32,
    tilt_y: i32,
    touch: bool,
    stylus1: bool,
    stylus2: bool,
    tool_pen: bool,
    eraser: bool,
}

impl PenState {
    fn apply(&mut self, event: InputEvent) -> bool {
        match (event.kind, event.code) {
            (3, 0) => self.x = event.value,
            (3, 1) => self.y = event.value,
            (3, 24) => self.pressure = event.value,
            (3, 25) => self.distance = event.value,
            (3, 26) => self.tilt_x = event.value,
            (3, 27) => self.tilt_y = event.value,
            (1, 320) => self.tool_pen = event.value != 0,
            (1, 321) => self.eraser = event.value != 0,
            (1, 330) => self.touch = event.value != 0,
            (1, 331) => self.stylus1 = event.value != 0,
            (1, 332) => self.stylus2 = event.value != 0,
            (0, 0) => return true,
            _ => {}
        }
        false
    }

    fn transformed(
        self,
        max_x: i32,
        max_y: i32,
        output_x: i32,
        output_y: i32,
        rotate: u16,
    ) -> Self {
        let (x, y, rotated_max_x, rotated_max_y) = match rotate {
            0 => (self.x, self.y, max_x, max_y),
            90 => (max_y - self.y, self.x, max_y, max_x),
            180 => (max_x - self.x, max_y - self.y, max_x, max_y),
            270 => (self.y, max_x - self.x, max_y, max_x),
            _ => unreachable!(),
        };
        Self {
            x: scale_axis(x, rotated_max_x, output_x),
            y: scale_axis(y, rotated_max_y, output_y),
            ..self
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn in_range(self) -> bool {
        self.tool_pen || self.eraser
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceInfo {
    model: String,
    path: String,
    event_size: usize,
    max_x: i32,
    max_y: i32,
    max_pressure: i32,
}

impl DeviceInfo {
    fn from_header(header: &str, args: &Args) -> Result<Self, String> {
        let fields: Vec<_> = header
            .trim_end_matches(['\r', '\n'])
            .splitn(4, '\t')
            .collect();
        if fields.len() != 4 || fields[0] != "RAT1" {
            return Err(format!("invalid remote header: {header:?}"));
        }
        let event_size = match fields[1] {
            "32" => 16,
            "64" => 24,
            bits => return Err(format!("unsupported remote word size: {bits}")),
        };
        let model = fields[3].trim_end_matches('\0').to_string();
        let profile = profile_for(&model);
        let max_x = args.max_x.or(profile.map(|p| p.0));
        let max_y = args.max_y.or(profile.map(|p| p.1));
        let max_pressure = args.max_pressure.or(profile.map(|p| p.2));
        let (Some(max_x), Some(max_y), Some(max_pressure)) = (max_x, max_y, max_pressure) else {
            return Err(format!(
                "unknown model {model:?}; pass --max-x, --max-y, and --max-pressure"
            ));
        };
        Ok(Self {
            model,
            path: fields[2].into(),
            event_size,
            max_x,
            max_y,
            max_pressure,
        })
    }

    fn output_ranges(&self, rotate: u16) -> (i32, i32) {
        if matches!(rotate, 90 | 270) {
            (self.max_y, self.max_x)
        } else {
            (self.max_x, self.max_y)
        }
    }

    fn landscape_rotation(&self) -> u16 {
        let model = self.model.to_ascii_lowercase();
        if model.contains("remarkable 1.0") || model.contains("remarkable 2.0") {
            0
        } else {
            90
        }
    }

    fn supports_auto_rotation(&self) -> bool {
        let model = self.model.to_ascii_lowercase();
        model.contains("ferrari")
            || model.contains("chiappa")
            || model.contains("tatsu")
            || model.trim() == "remarkable pro"
    }
}

fn scale_axis(value: i32, source_max: i32, target_max: i32) -> i32 {
    (value.clamp(0, source_max) as i64 * target_max as i64 / source_max as i64) as i32
}

fn sensor_rotation(x: i32, y: i32, current: Option<u16>) -> Option<u16> {
    let x = x as f64;
    let y = y as f64;
    if x.hypot(y) < 3500.0 {
        return None;
    }
    let angle = (y.atan2(x).to_degrees() + 90.0).rem_euclid(360.0);
    if let Some(current) = current {
        let mut delta = (angle - current as f64).abs();
        if delta > 180.0 {
            delta = 360.0 - delta;
        }
        if delta <= 57.0 {
            return None;
        }
    }
    Some(((((angle + 45.0) / 90.0) as u16) % 4) * 90)
}

fn profile_for(model: &str) -> Option<(i32, i32, i32)> {
    let model = model.to_ascii_lowercase();
    if model.contains("chiappa") {
        Some((6760, 11960, 4096))
    } else if model.contains("tatsu") {
        Some((9620, 13000, 4096))
    } else if model.contains("ferrari") || model.trim() == "remarkable pro" {
        Some((11180, 15340, 4096))
    } else if model.contains("remarkable 1.0") || model.contains("remarkable 2.0") {
        Some((20967, 15725, 4095))
    } else {
        None
    }
}

fn read_event(reader: &mut impl Read, record_size: usize) -> io::Result<Option<InputEvent>> {
    let mut bytes = vec![0; record_size];
    let mut filled = 0;
    while filled < bytes.len() {
        let count = reader.read(&mut bytes[filled..])?;
        if count == 0 {
            if filled == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("partial evdev record ({filled}/{record_size} bytes)"),
            ));
        }
        filled += count;
    }
    let offset = record_size - 8;
    Ok(Some(InputEvent {
        kind: u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()),
        code: u16::from_le_bytes(bytes[offset + 2..offset + 4].try_into().unwrap()),
        value: i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()),
    }))
}

struct SshChild(Child);

impl Drop for SshChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_ssh(
    args: &Args,
    auto_rotate: bool,
    password: Option<&str>,
    data_port: u16,
    data_token: &str,
) -> io::Result<SshChild> {
    ssh_command(args, auto_rotate, password, data_port, data_token)?
        .spawn()
        .map(SshChild)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to start OpenSSH client `ssh`: {error}"),
            )
        })
}

fn ssh_command(
    args: &Args,
    auto_rotate: bool,
    password: Option<&str>,
    data_port: u16,
    data_token: &str,
) -> io::Result<Command> {
    let mut command = Command::new("ssh");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.args([
        "-T",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UpdateHostKeys=no",
        "-o",
        "CheckHostIP=no",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "PreferredAuthentications=publickey,keyboard-interactive,password",
        "-o",
        "NumberOfPasswordPrompts=1",
    ]);
    command
        .arg("-o")
        .arg(format!("UserKnownHostsFile={KNOWN_HOSTS_SINK}"))
        .arg("-o")
        .arg(format!("GlobalKnownHostsFile={KNOWN_HOSTS_SINK}"));
    if let Some(key) = &args.key {
        command.arg("-i").arg(key);
    }
    command
        .env("SSH_ASKPASS", env::current_exe()?)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("RATABLET_ASKPASS", "1");
    if let Some(password) = password {
        command.env("RATABLET_PASSWORD", password);
    }
    let device = args.device.as_deref().unwrap_or("");
    let auto = u8::from(auto_rotate);
    command
        .arg("--")
        .arg(&args.host)
        .arg(format!(
            "RAT_DEVICE='{device}' RAT_AUTO='{auto}' RAT_PORT='{data_port}' RAT_TOKEN='{data_token}' /bin/sh -c {}",
            shell_quote(REMOTE_SCRIPT)
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn data_token() -> io::Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(format!("{:032x}", u128::from_ne_bytes(bytes)))
}

fn accept_data_stream(listener: &TcpListener, token: &str) -> io::Result<TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_millis(500)))?;
                let mut received = vec![0; token.len()];
                if stream.read_exact(&mut received).is_ok() && received == token.as_bytes() {
                    stream.set_read_timeout(None)?;
                    return Ok(stream);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "direct pen data connection timed out (check the host firewall)",
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_ssh_stderr(
    stderr: impl Read,
    orientation: Arc<AtomicU16>,
    shared: Arc<SharedState>,
    errors: Arc<Mutex<Vec<String>>>,
    verbose: bool,
) {
    let mut current = None;
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        if let Some(values) = line.strip_prefix("RATACCEL\t") {
            let mut values = values
                .split('\t')
                .filter_map(|value| value.parse::<i32>().ok());
            if let (Some(x), Some(y)) = (values.next(), values.next()) {
                if let Some(next) = sensor_rotation(x, y, current) {
                    if verbose && current != Some(next) {
                        eprintln!("orientation: {next}");
                    }
                    current = Some(next);
                    orientation.store(next, Ordering::Relaxed);
                    if shared.rotation_mode() == RotationMode::Auto {
                        shared.effective_rotation.store(next, Ordering::Relaxed);
                    }
                }
                continue;
            }
        }
        eprintln!("{line}");
        let mut errors = errors.lock().unwrap();
        errors.push(line);
        if errors.len() > 8 {
            errors.remove(0);
        }
    }
}

fn run(args: &Args, shared: &Arc<SharedState>) -> Result<(), Box<dyn Error>> {
    if shared.stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    shared.effective_rotation.store(u16::MAX, Ordering::Relaxed);
    let mode = shared.rotation_mode();
    let revision = shared.revision.load(Ordering::Relaxed);
    let host = shared.host.lock().unwrap().clone();
    let password = shared.password.lock().unwrap().clone();
    let mut ssh_args = args.clone();
    ssh_args.host = host;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    listener.set_nonblocking(true)?;
    let data_port = listener.local_addr()?.port();
    let data_token = data_token()?;
    let mut ssh = spawn_ssh(
        &ssh_args,
        mode == RotationMode::Auto,
        password.as_deref(),
        data_port,
        &data_token,
    )?;
    let orientation = Arc::new(AtomicU16::new(u16::MAX));
    let ssh_errors = Arc::new(Mutex::new(Vec::new()));
    let stderr_thread = if let Some(stderr) = ssh.0.stderr.take() {
        let orientation = Arc::clone(&orientation);
        let shared = Arc::clone(shared);
        let errors = Arc::clone(&ssh_errors);
        let verbose = args.verbose;
        Some(thread::spawn(move || {
            read_ssh_stderr(stderr, orientation, shared, errors, verbose)
        }))
    } else {
        None
    };
    let stdout = ssh
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("ssh stdout was not captured"))?;
    let mut reader = BufReader::new(stdout);
    let mut header = String::new();
    reader.read_line(&mut header)?;
    if header.is_empty() {
        let status = ssh.0.wait()?;
        if let Some(thread) = stderr_thread {
            let _ = thread.join();
        }
        let errors = ssh_errors.lock().unwrap().join("\n");
        return Err(io::Error::other(if errors.is_empty() {
            format!("SSH exited with {status}")
        } else {
            errors
        })
        .into());
    }
    let info = DeviceInfo::from_header(&header, args).map_err(io::Error::other)?;
    if mode == RotationMode::Auto && !info.supports_auto_rotation() {
        return Err(io::Error::other(format!(
            "{} has no supported rotation sensor; use --rotate",
            info.model
        ))
        .into());
    }
    let mut event_reader = BufReader::new(accept_data_stream(&listener, &data_token)?);
    let fixed_rotation = mode.rotation(&info, u16::MAX);
    let output_rotation = fixed_rotation;
    let (output_x, output_y) = info.output_ranges(output_rotation);
    let rotation_label = if mode == RotationMode::Auto {
        "auto".into()
    } else {
        fixed_rotation.to_string()
    };
    eprintln!(
        "connected: {} ({}, {} bytes/event, {}x{}, pressure {}, rotate {})",
        info.model,
        info.path,
        info.event_size,
        output_x,
        output_y,
        info.max_pressure,
        rotation_label
    );
    shared
        .effective_rotation
        .store(fixed_rotation, Ordering::Relaxed);
    shared.set_connection(ConnectionStatus::Connected(info.clone()));
    let mut output = platform::Output::new(output_x, output_y, info.max_pressure)?;
    let mut state = PenState::default();
    let (event_tx, event_rx) = mpsc::channel();
    let event_size = info.event_size;
    thread::spawn(move || loop {
        match read_event(&mut event_reader, event_size) {
            Ok(Some(event)) => {
                if event_tx.send(Ok(event)).is_err() {
                    break;
                }
            }
            Ok(None) => {
                let _ = event_tx.send(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "SSH event stream ended",
                )));
                break;
            }
            Err(error) => {
                let _ = event_tx.send(Err(error));
                break;
            }
        }
    });
    loop {
        if shared.stop.load(Ordering::Relaxed)
            || shared.revision.load(Ordering::Relaxed) != revision
        {
            return Ok(());
        }
        let event = match event_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(event) => event?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("SSH event reader stopped").into())
            }
        };
        if args.verbose {
            eprintln!(
                "event type={} code={} value={}",
                event.kind, event.code, event.value
            );
        }
        if state.apply(event) {
            let sensed = orientation.load(Ordering::Relaxed);
            let rotate = mode.rotation(&info, sensed);
            shared.effective_rotation.store(rotate, Ordering::Relaxed);
            output.push(state.transformed(info.max_x, info.max_y, output_x, output_y, rotate))?;
        }
    }
}

fn connection_loop(args: Args, shared: Arc<SharedState>) {
    while !shared.stop.load(Ordering::Relaxed) {
        if let Err(error) = run(&args, &shared) {
            let error = error.to_string();
            eprintln!("ratablet: {error}");
            shared.set_waiting(error.clone());
            if is_auth_error(&error) {
                let revision = shared.revision.load(Ordering::Relaxed);
                while !shared.stop.load(Ordering::Relaxed)
                    && shared.revision.load(Ordering::Relaxed) == revision
                {
                    thread::sleep(std::time::Duration::from_millis(100));
                }
                continue;
            }
            eprintln!("ratablet: disconnected; waiting to reconnect");
            for _ in 0..10 {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn is_auth_error(error: &str) -> bool {
    error_kind(error) == RetryErrorKind::Authentication
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RetryErrorKind {
    Authentication,
    Timeout,
    Refused,
    Unreachable,
    NameResolution,
    Other,
}

fn error_kind(error: &str) -> RetryErrorKind {
    let error = error.to_ascii_lowercase();
    if error.contains("permission denied") || error.contains("authentication failed") {
        RetryErrorKind::Authentication
    } else if error.contains("timed out") {
        RetryErrorKind::Timeout
    } else if error.contains("connection refused") {
        RetryErrorKind::Refused
    } else if error.contains("no route to host") || error.contains("network is unreachable") {
        RetryErrorKind::Unreachable
    } else if error.contains("could not resolve hostname")
        || error.contains("name or service not known")
    {
        RetryErrorKind::NameResolution
    } else {
        RetryErrorKind::Other
    }
}

fn same_error_type(left: &str, right: &str) -> bool {
    let kind = error_kind(left);
    (kind != RetryErrorKind::Other && kind == error_kind(right)) || left == right
}

fn main() {
    if env::var_os("RATABLET_ASKPASS").is_some() {
        if let Ok(password) = env::var("RATABLET_PASSWORD") {
            println!("{password}");
        }
        return;
    }
    let args = match Args::parse() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(if message.starts_with("Usage:") { 0 } else { 2 });
        }
    };
    if !args.headless {
        if let Err(error) = gui::run(args) {
            eprintln!("ratablet: GUI failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    let shared = Arc::new(SharedState::new(
        RotationMode::from_args(&args),
        args.host.clone(),
        None,
    ));
    connection_loop(args, shared);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Args {
        Args::default()
    }

    #[test]
    fn recognizes_every_supported_model_family() {
        assert_eq!(profile_for("reMarkable 1.0"), Some((20967, 15725, 4095)));
        assert_eq!(profile_for("reMarkable 2.0"), Some((20967, 15725, 4095)));
        assert_eq!(
            profile_for("reMarkable Ferrari"),
            Some((11180, 15340, 4096))
        );
        assert_eq!(profile_for("reMarkable Chiappa"), Some((6760, 11960, 4096)));
        assert_eq!(profile_for("reMarkable Tatsu"), Some((9620, 13000, 4096)));
    }

    #[test]
    fn parses_32_and_64_bit_evdev_records() {
        for (size, offset) in [(16, 8), (24, 16)] {
            let mut bytes = vec![0u8; size];
            bytes[offset..offset + 2].copy_from_slice(&3u16.to_le_bytes());
            bytes[offset + 2..offset + 4].copy_from_slice(&24u16.to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&2048i32.to_le_bytes());
            assert_eq!(
                read_event(&mut bytes.as_slice(), size).unwrap(),
                Some(InputEvent {
                    kind: 3,
                    code: 24,
                    value: 2048
                })
            );
        }
    }

    #[test]
    fn header_allows_unknown_hardware_with_explicit_ranges() {
        let mut args = args();
        args.max_x = Some(100);
        args.max_y = Some(200);
        args.max_pressure = Some(1024);
        let info =
            DeviceInfo::from_header("RAT1\t64\t/dev/input/event9\tfuture model\n", &args).unwrap();
        assert_eq!(
            (info.max_x, info.max_y, info.max_pressure),
            (100, 200, 1024)
        );
    }

    #[test]
    fn rotates_coordinates_and_ranges() {
        let state = PenState {
            x: 25,
            y: 80,
            ..PenState::default()
        };
        let rotated = state.transformed(100, 200, 200, 100, 90);
        assert_eq!((rotated.x, rotated.y), (120, 25));
        let rm2 = DeviceInfo::from_header("RAT1\t32\t/dev/input/event1\treMarkable 2.0\n", &args())
            .unwrap();
        assert_eq!(rm2.landscape_rotation(), 0);
        let rmpp =
            DeviceInfo::from_header("RAT1\t64\t/dev/input/event2\treMarkable Ferrari\n", &args())
                .unwrap();
        assert_eq!(rmpp.landscape_rotation(), 90);
        assert!(rmpp.supports_auto_rotation());
    }

    #[test]
    fn maps_accelerometer_quadrants_and_ignores_flat_readings() {
        assert_eq!(sensor_rotation(0, -10000, None), Some(0));
        assert_eq!(sensor_rotation(10000, 0, None), Some(90));
        assert_eq!(sensor_rotation(0, 10000, None), Some(180));
        assert_eq!(sensor_rotation(-10000, 0, None), Some(270));
        assert_eq!(sensor_rotation(100, 100, None), None);
        assert_eq!(sensor_rotation(7000, -7000, Some(0)), None);
    }

    #[test]
    fn normalizes_device_ip_to_root_ssh_target() {
        assert_eq!(normalize_host("10.11.99.1").unwrap(), "root@10.11.99.1");
        assert_eq!(normalize_host("admin@rm.local").unwrap(), "admin@rm.local");
        assert!(normalize_host("root@bad host").is_err());
        assert!(normalize_host("root@").is_err());
    }

    #[test]
    fn ssh_skips_known_hosts_uses_keys_first_and_internal_askpass() {
        let command = ssh_command(&args(), false, Some("secret"), 12345, "data-token").unwrap();
        let arguments: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy())
            .collect();
        assert!(arguments
            .iter()
            .any(|value| value == "StrictHostKeyChecking=no"));
        for option in ["UpdateHostKeys=no", "CheckHostIP=no"] {
            assert!(arguments.iter().any(|value| value == option));
        }
        assert!(arguments
            .iter()
            .any(|value| value == &format!("UserKnownHostsFile={KNOWN_HOSTS_SINK}")));
        assert!(arguments
            .iter()
            .any(|value| value == &format!("GlobalKnownHostsFile={KNOWN_HOSTS_SINK}")));
        assert!(arguments.iter().any(
            |value| value == "PreferredAuthentications=publickey,keyboard-interactive,password"
        ));
        let environment: Vec<_> = command.get_envs().collect();
        assert!(environment.iter().any(|(key, value)| {
            *key == "SSH_ASKPASS_REQUIRE" && value.is_some_and(|value| value == "force")
        }));
        assert!(environment.iter().any(|(key, value)| {
            *key == "RATABLET_PASSWORD" && value.is_some_and(|value| value == "secret")
        }));
        assert!(arguments
            .iter()
            .any(|value| value.contains("RAT_PORT='12345' RAT_TOKEN='data-token'")));
    }

    #[test]
    fn direct_data_stream_requires_the_session_token() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for token in [b"wrong-token".as_slice(), b"right-token"] {
                let mut stream = TcpStream::connect(address).unwrap();
                std::io::Write::write_all(&mut stream, token).unwrap();
            }
        });
        assert!(accept_data_stream(&listener, "right-token").is_ok());
    }

    #[test]
    fn recognizes_ssh_authentication_failures() {
        assert!(is_auth_error(
            "root@10.11.99.1: Permission denied (publickey,password)."
        ));
        assert!(is_auth_error("Authentication failed."));
        assert!(!is_auth_error("Connection timed out"));
        assert!(same_error_type(
            "connect to host 10.0.0.1: Connection timed out",
            "connect to host 10.0.0.2: Connection timed out"
        ));
        assert!(!same_error_type(
            "Connection timed out",
            "Connection refused"
        ));
    }
}
