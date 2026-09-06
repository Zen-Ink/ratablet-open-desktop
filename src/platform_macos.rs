use crate::PenState;
use std::ffi::c_void;
use std::io;
use std::ptr;

type CGEventRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

const CG_HID_EVENT_TAP: i32 = 0;
const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const CG_EVENT_MOUSE_MOVED: u32 = 5;
const CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
const CG_EVENT_TABLET_PROXIMITY: u32 = 24;

const MOUSE_EVENT_CLICK_STATE: u32 = 1;
const MOUSE_EVENT_PRESSURE: u32 = 2;
const MOUSE_EVENT_SUBTYPE: u32 = 7;
const TABLET_EVENT_POINT_X: u32 = 15;
const TABLET_EVENT_POINT_Y: u32 = 16;
const TABLET_EVENT_POINT_BUTTONS: u32 = 18;
const TABLET_EVENT_POINT_PRESSURE: u32 = 19;
const TABLET_EVENT_TILT_X: u32 = 20;
const TABLET_EVENT_TILT_Y: u32 = 21;
const TABLET_EVENT_DEVICE_ID: u32 = 24;
const TABLET_PROXIMITY_VENDOR_ID: u32 = 28;
const TABLET_PROXIMITY_TABLET_ID: u32 = 29;
const TABLET_PROXIMITY_POINTER_ID: u32 = 30;
const TABLET_PROXIMITY_DEVICE_ID: u32 = 31;
const TABLET_PROXIMITY_SYSTEM_TABLET_ID: u32 = 32;
const TABLET_PROXIMITY_VENDOR_POINTER_TYPE: u32 = 33;
const TABLET_PROXIMITY_CAPABILITY_MASK: u32 = 36;
const TABLET_PROXIMITY_POINTER_TYPE: u32 = 37;
const TABLET_PROXIMITY_ENTER: u32 = 38;

const MOUSE_SUBTYPE_TABLET_POINT: i64 = 1;
const POINTER_PEN: i64 = 1;
const POINTER_ERASER: i64 = 3;
const DEVICE_ID: i64 = 0x5241_5441_424c_4554;
const VENDOR_ID: i64 = 0x056a;
const TABLET_ID: i64 = 1;
const VENDOR_POINTER_STYLUS: i64 = 0x0802;
const CAPABILITIES: i64 = 0x0001 | 0x0002 | 0x0004 | 0x0040 | 0x0080 | 0x0100 | 0x0400;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        mouse_type: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    fn CGEventSetType(event: CGEventRef, event_type: u32);
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventSetDoubleValueField(event: CGEventRef, field: u32, value: f64);
    fn CGEventPost(tap: i32, event: CGEventRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

pub struct Output {
    bounds: CGRect,
    max_x: i32,
    max_y: i32,
    max_pressure: i32,
    was_in_range: bool,
    was_touching: bool,
    was_eraser: bool,
    last: PenState,
}

impl Output {
    pub fn new(max_x: i32, max_y: i32, max_pressure: i32) -> io::Result<Self> {
        if unsafe { !CGPreflightPostEventAccess() && !CGRequestPostEventAccess() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "grant ratablet Accessibility access in System Settings > Privacy & Security",
            ));
        }
        // ponytail: map the main display only; add display selection when it is requested.
        let bounds = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
            return Err(io::Error::other("macOS reported an empty main display"));
        }
        Ok(Self {
            bounds,
            max_x,
            max_y,
            max_pressure,
            was_in_range: false,
            was_touching: false,
            was_eraser: false,
            last: PenState::default(),
        })
    }

    pub fn push(&mut self, state: PenState) -> io::Result<()> {
        let in_range = state.in_range();
        if in_range {
            if !self.was_in_range || state.eraser != self.was_eraser {
                self.post_proximity(true, state.eraser)?;
            }
            self.post_pointer(state)?;
        } else if self.was_in_range {
            if self.was_touching {
                let mut released = self.last;
                released.touch = false;
                released.pressure = 0;
                self.post_pointer(released)?;
            }
            self.post_proximity(false, self.was_eraser)?;
        }
        self.was_in_range = in_range;
        self.was_touching = in_range && state.touch;
        self.was_eraser = state.eraser;
        self.last = state;
        Ok(())
    }

    fn post_pointer(&self, state: PenState) -> io::Result<()> {
        let event_type = if state.touch && !self.was_touching {
            CG_EVENT_LEFT_MOUSE_DOWN
        } else if state.touch {
            CG_EVENT_LEFT_MOUSE_DRAGGED
        } else if self.was_touching {
            CG_EVENT_LEFT_MOUSE_UP
        } else {
            CG_EVENT_MOUSE_MOVED
        };
        let click_state = i64::from(event_type != CG_EVENT_MOUSE_MOVED);
        let point = CGPoint {
            x: self.bounds.origin.x + scale(state.x, self.max_x, self.bounds.size.width),
            y: self.bounds.origin.y + scale(state.y, self.max_y, self.bounds.size.height),
        };
        let event = unsafe { CGEventCreateMouseEvent(ptr::null(), event_type, point, 0) };
        if event.is_null() {
            return Err(io::Error::other("CGEventCreateMouseEvent failed"));
        }
        let pressure = state.pressure.clamp(0, self.max_pressure) as f64 / self.max_pressure as f64;
        let buttons = i64::from(state.touch)
            | (i64::from(state.stylus1) << 1)
            | (i64::from(state.stylus2) << 2);
        unsafe {
            // CoreGraphics requires the tablet subtype before any tablet fields.
            CGEventSetIntegerValueField(event, MOUSE_EVENT_SUBTYPE, MOUSE_SUBTYPE_TABLET_POINT);
            CGEventSetIntegerValueField(event, MOUSE_EVENT_CLICK_STATE, click_state);
            CGEventSetDoubleValueField(event, MOUSE_EVENT_PRESSURE, pressure);
            CGEventSetIntegerValueField(event, TABLET_EVENT_POINT_X, state.x as i64);
            CGEventSetIntegerValueField(event, TABLET_EVENT_POINT_Y, state.y as i64);
            CGEventSetIntegerValueField(event, TABLET_EVENT_POINT_BUTTONS, buttons);
            CGEventSetDoubleValueField(event, TABLET_EVENT_POINT_PRESSURE, pressure);
            CGEventSetDoubleValueField(event, TABLET_EVENT_TILT_X, state.tilt_x as f64 / 9000.0);
            CGEventSetDoubleValueField(event, TABLET_EVENT_TILT_Y, -state.tilt_y as f64 / 9000.0);
            CGEventSetIntegerValueField(event, TABLET_EVENT_DEVICE_ID, DEVICE_ID);
        }
        post(event);
        Ok(())
    }

    fn post_proximity(&self, entering: bool, eraser: bool) -> io::Result<()> {
        let event = unsafe { CGEventCreate(ptr::null()) };
        if event.is_null() {
            return Err(io::Error::other("CGEventCreate failed"));
        }
        unsafe {
            CGEventSetType(event, CG_EVENT_TABLET_PROXIMITY);
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_VENDOR_ID, VENDOR_ID);
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_TABLET_ID, TABLET_ID);
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_POINTER_ID, 1);
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_DEVICE_ID, DEVICE_ID);
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_SYSTEM_TABLET_ID, DEVICE_ID);
            CGEventSetIntegerValueField(
                event,
                TABLET_PROXIMITY_VENDOR_POINTER_TYPE,
                VENDOR_POINTER_STYLUS,
            );
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_CAPABILITY_MASK, CAPABILITIES);
            CGEventSetIntegerValueField(
                event,
                TABLET_PROXIMITY_POINTER_TYPE,
                if eraser { POINTER_ERASER } else { POINTER_PEN },
            );
            CGEventSetIntegerValueField(event, TABLET_PROXIMITY_ENTER, i64::from(entering));
        }
        post(event);
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if self.was_in_range {
            if self.was_touching {
                let mut released = self.last;
                released.touch = false;
                released.pressure = 0;
                let _ = self.post_pointer(released);
            }
            let _ = self.post_proximity(false, self.was_eraser);
        }
    }
}

fn post(event: CGEventRef) {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        CGEventPost(CG_HID_EVENT_TAP, event);
        objc_autoreleasePoolPop(pool);
        CFRelease(event.cast_const());
    }
}

fn scale(value: i32, source_max: i32, target_size: f64) -> f64 {
    value.clamp(0, source_max) as f64 * (target_size - 1.0) / source_max as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_tablet_axis_to_display() {
        assert_eq!(scale(0, 100, 1000.0), 0.0);
        assert_eq!(scale(50, 100, 1000.0), 499.5);
        assert_eq!(scale(100, 100, 1000.0), 999.0);
    }
}
