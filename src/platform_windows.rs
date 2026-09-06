use crate::PenState;
use std::io;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO,
};
use windows_sys::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE,
    POINTER_FLAG_PRIMARY, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, PEN_FLAG_BARREL, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PEN_MASK_PRESSURE,
    PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

pub struct Output {
    device: HSYNTHETICPOINTERDEVICE,
    max_x: i32,
    max_y: i32,
    max_pressure: i32,
    screen_x: i32,
    screen_y: i32,
    screen_w: i32,
    screen_h: i32,
    was_in_range: bool,
    was_touching: bool,
    last: PenState,
}

impl Output {
    pub fn new(max_x: i32, max_y: i32, max_pressure: i32) -> io::Result<Self> {
        let device = unsafe { CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_NONE) };
        if device.is_null() {
            return Err(io::Error::last_os_error());
        }
        let (screen_x, screen_y, screen_w, screen_h) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        if screen_w <= 0 || screen_h <= 0 {
            unsafe { DestroySyntheticPointerDevice(device) };
            return Err(io::Error::other(
                "Windows reported an empty virtual desktop",
            ));
        }
        Ok(Self {
            device,
            max_x,
            max_y,
            max_pressure,
            screen_x,
            screen_y,
            screen_w,
            screen_h,
            was_in_range: false,
            was_touching: false,
            last: PenState::default(),
        })
    }

    pub fn push(&mut self, state: PenState) -> io::Result<()> {
        let in_range = state.in_range();
        if !in_range && !self.was_in_range {
            self.last = state;
            return Ok(());
        }
        let flags = if in_range && state.touch && !self.was_touching {
            POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_PRIMARY
        } else if in_range && state.touch {
            POINTER_FLAG_UPDATE
                | POINTER_FLAG_INRANGE
                | POINTER_FLAG_INCONTACT
                | POINTER_FLAG_PRIMARY
        } else if in_range && self.was_touching {
            POINTER_FLAG_UP | POINTER_FLAG_INRANGE | POINTER_FLAG_PRIMARY
        } else if in_range {
            POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_PRIMARY
        } else {
            POINTER_FLAG_UP | POINTER_FLAG_PRIMARY
        };

        let point = POINT {
            x: self.screen_x + scale(state.x, self.max_x, self.screen_w - 1),
            y: self.screen_y + scale(state.y, self.max_y, self.screen_h - 1),
        };
        let mut pen = POINTER_PEN_INFO::default();
        pen.pointerInfo.pointerType = PT_PEN;
        pen.pointerInfo.pointerId = 1;
        pen.pointerInfo.pointerFlags = flags;
        pen.pointerInfo.ptPixelLocation = point;
        pen.pointerInfo.ptPixelLocationRaw = point;
        pen.penMask = PEN_MASK_PRESSURE | PEN_MASK_TILT_X | PEN_MASK_TILT_Y;
        pen.pressure = scale(state.pressure, self.max_pressure, 1024) as u32;
        pen.tiltX = (state.tilt_x / 100).clamp(-90, 90);
        pen.tiltY = (state.tilt_y / 100).clamp(-90, 90);
        if state.stylus1 {
            pen.penFlags |= PEN_FLAG_BARREL;
        }
        if state.eraser {
            pen.penFlags |= PEN_FLAG_INVERTED | PEN_FLAG_ERASER;
        }
        let info = POINTER_TYPE_INFO {
            r#type: PT_PEN,
            Anonymous: windows_sys::Win32::UI::Controls::POINTER_TYPE_INFO_0 { penInfo: pen },
        };
        if unsafe { InjectSyntheticPointerInput(self.device, &info, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.was_in_range = in_range;
        self.was_touching = state.touch;
        self.last = state;
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if self.was_in_range {
            let mut released = self.last;
            released.tool_pen = false;
            released.eraser = false;
            released.touch = false;
            let _ = self.push(released);
        }
        unsafe { DestroySyntheticPointerDevice(self.device) };
    }
}

fn scale(value: i32, source_max: i32, target_max: i32) -> i32 {
    (value.clamp(0, source_max) as i64 * target_max as i64 / source_max as i64) as i32
}
