use crate::PenState;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const ABS_X: usize = 0;
const ABS_Y: usize = 1;
const ABS_PRESSURE: usize = 24;
const ABS_DISTANCE: usize = 25;
const ABS_TILT_X: usize = 26;
const ABS_TILT_Y: usize = 27;
const BTN_TOOL_PEN: u16 = 320;
const BTN_TOOL_RUBBER: u16 = 321;
const BTN_TOUCH: u16 = 330;
const BTN_STYLUS: u16 = 331;
const BTN_STYLUS2: u16 = 332;
const BUS_USB: u16 = 3;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
const UI_ABS_SETUP: libc::c_ulong = 0x401c_5504;
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;

#[repr(C)]
#[derive(Default)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UInputSetup {
    id: InputId,
    name: [libc::c_char; 80],
    ff_effects_max: u32,
}

impl Default for UInputSetup {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Default)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
#[derive(Default)]
struct UInputAbsSetup {
    code: u16,
    absinfo: InputAbsInfo,
}

#[repr(C)]
struct KernelInputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

pub struct Output {
    file: File,
}

impl Output {
    pub fn new(max_x: i32, max_y: i32, max_pressure: i32) -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot open /dev/uinput (check permissions): {error}"),
                )
            })?;
        let fd = file.as_raw_fd();
        ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_ulong)?;
        for code in [
            BTN_TOOL_PEN,
            BTN_TOOL_RUBBER,
            BTN_TOUCH,
            BTN_STYLUS,
            BTN_STYLUS2,
        ] {
            ioctl(fd, UI_SET_KEYBIT, code as libc::c_ulong)?;
        }
        ioctl(fd, UI_SET_EVBIT, EV_ABS as libc::c_ulong)?;
        setup_abs(fd, ABS_X, 0, max_x, 100)?;
        setup_abs(fd, ABS_Y, 0, max_y, 100)?;
        setup_abs(fd, ABS_PRESSURE, 0, max_pressure, 0)?;
        setup_abs(fd, ABS_DISTANCE, 0, u16::MAX as i32, 0)?;
        setup_abs(fd, ABS_TILT_X, -9000, 9000, 5074)?;
        setup_abs(fd, ABS_TILT_Y, -9000, 9000, 5074)?;

        let mut setup = UInputSetup::default();
        for (slot, byte) in setup.name.iter_mut().zip(b"ratablet reMarkable Pen\0") {
            *slot = *byte as libc::c_char;
        }
        setup.id = InputId {
            bustype: BUS_USB,
            vendor: 0x056a,
            product: 0x0001,
            version: 1,
        };
        ioctl_ptr(fd, UI_DEV_SETUP, &setup)?;
        ioctl(fd, UI_DEV_CREATE, 0)?;
        Ok(Self { file })
    }

    pub fn push(&mut self, state: PenState) -> io::Result<()> {
        self.emit(EV_ABS, ABS_X as u16, state.x)?;
        self.emit(EV_ABS, ABS_Y as u16, state.y)?;
        self.emit(EV_ABS, ABS_PRESSURE as u16, state.pressure)?;
        self.emit(EV_ABS, ABS_DISTANCE as u16, state.distance)?;
        self.emit(EV_ABS, ABS_TILT_X as u16, state.tilt_x)?;
        self.emit(EV_ABS, ABS_TILT_Y as u16, state.tilt_y)?;
        self.emit(EV_KEY, BTN_TOOL_PEN, state.tool_pen as i32)?;
        self.emit(EV_KEY, BTN_TOOL_RUBBER, state.eraser as i32)?;
        self.emit(EV_KEY, BTN_TOUCH, state.touch as i32)?;
        self.emit(EV_KEY, BTN_STYLUS, state.stylus1 as i32)?;
        self.emit(EV_KEY, BTN_STYLUS2, state.stylus2 as i32)?;
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    fn emit(&mut self, kind: u16, code: u16, value: i32) -> io::Result<()> {
        let event = KernelInputEvent {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            kind,
            code,
            value,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &event as *const KernelInputEvent as *const u8,
                std::mem::size_of::<KernelInputEvent>(),
            )
        };
        self.file.write_all(bytes)
    }
}

fn setup_abs(
    fd: libc::c_int,
    code: usize,
    minimum: i32,
    maximum: i32,
    resolution: i32,
) -> io::Result<()> {
    let setup = UInputAbsSetup {
        code: code as u16,
        absinfo: InputAbsInfo {
            minimum,
            maximum,
            resolution,
            ..InputAbsInfo::default()
        },
    };
    ioctl_ptr(fd, UI_ABS_SETUP, &setup)
}

fn ioctl_ptr<T>(fd: libc::c_int, request: libc::c_ulong, value: &T) -> io::Result<()> {
    if unsafe { libc::ioctl(fd, request, value) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY) };
    }
}

fn ioctl(fd: libc::c_int, request: libc::c_ulong, value: libc::c_ulong) -> io::Result<()> {
    if unsafe { libc::ioctl(fd, request, value) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uinput_structs_match_linux_abi() {
        assert_eq!(std::mem::size_of::<UInputSetup>(), 92);
        assert_eq!(std::mem::size_of::<InputAbsInfo>(), 24);
        assert_eq!(std::mem::size_of::<UInputAbsSetup>(), 28);
    }
}
