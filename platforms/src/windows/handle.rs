use std::{cell::Cell, ffi::OsString, os::windows::ffi::OsStringExt, ptr, str};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT},
        Graphics::Gdi::{
            ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowRect, GetWindowTextW},
    },
    core::BOOL,
};

use crate::{ConvertedCoordinates, Error, Result};

#[derive(Clone, Debug)]
pub struct HandleCell {
    inner: Handle,
    inner_cell: Cell<Option<HWND>>,
}

impl HandleCell {
    pub fn new(handle: Handle) -> Self {
        Self {
            inner: handle,
            inner_cell: Cell::new(None),
        }
    }

    #[inline]
    pub fn as_inner(&self) -> Option<HWND> {
        match self.inner.kind {
            HandleKind::Fixed(handle) => Some(handle),
            HandleKind::Dynamic(class) => {
                if self.inner_cell.get().is_none() {
                    self.inner_cell.set(query_handle(class));
                }

                let handle = self.inner_cell.get()?;
                if is_class_matched(handle, class) {
                    Some(handle)
                } else {
                    self.inner_cell.set(None);
                    None
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Fixed(HWND),
    Dynamic(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle {
    kind: HandleKind,
}

impl Handle {
    pub fn new(kind: HandleKind) -> Self {
        Self { kind }
    }

    pub fn name(&self) -> Result<String> {
        let handle = self.as_inner().ok_or(Error::WindowNotFound)?;
        let mut buf = [0u16; 256];
        let count = unsafe { GetWindowTextW(handle, &mut buf) } as usize;
        if count == 0 {
            return Err(Error::from_last_win_error());
        }

        Ok(OsString::from_wide(&buf[..count])
            .to_str()
            .unwrap_or_default()
            .to_string())
    }

    pub fn convert_coordinate(
        &self,
        x: i32,
        y: i32,
        monitor_coordinate: bool,
    ) -> Result<ConvertedCoordinates> {
        let handle = self.as_inner().ok_or(Error::WindowNotFound)?;
        let mut point = POINT { x, y };
        unsafe { ClientToScreen(handle, &raw mut point).ok()? };

        if !monitor_coordinate {
            let mut rect = RECT::default();
            unsafe { GetWindowRect(handle, &raw mut rect)? };

            let x = point.x - rect.left;
            let y = point.y - rect.top;
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;

            return Ok(ConvertedCoordinates {
                width,
                height,
                x,
                y,
            });
        }

        // Get monitor from window
        let monitor = unsafe { MonitorFromWindow(handle, MONITOR_DEFAULTTONULL) };
        if monitor.is_invalid() {
            return Err(Error::WindowNotFound);
        }

        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        unsafe { GetMonitorInfoW(monitor, &mut mi).ok()? };
        let width = mi.rcMonitor.right - mi.rcMonitor.left;
        let height = mi.rcMonitor.bottom - mi.rcMonitor.top;

        let x = point.x - mi.rcMonitor.left;
        let y = point.y - mi.rcMonitor.top;

        Ok(ConvertedCoordinates {
            width,
            height,
            x,
            y,
        })
    }

    pub fn as_inner(&self) -> Option<HWND> {
        match self.kind {
            HandleKind::Fixed(handle) => Some(handle),
            HandleKind::Dynamic(class) => query_handle(class),
        }
    }
}

#[inline]
fn query_handle(class: &'static str) -> Option<HWND> {
    struct Params {
        class: &'static str,
        handle_out: *mut HWND,
    }

    unsafe extern "system" fn callback(handle: HWND, params: LPARAM) -> BOOL {
        let params = unsafe { ptr::read::<Params>(params.0 as *const _) };
        if is_class_matched(handle, params.class) {
            unsafe { ptr::write(params.handle_out, handle) };
            false.into()
        } else {
            true.into()
        }
    }

    let mut handle = HWND::default();
    let params = Params {
        class,
        handle_out: &raw mut handle,
    };
    let _ = unsafe { EnumWindows(Some(callback), LPARAM(&raw const params as isize)) };

    if handle.is_invalid() {
        None
    } else {
        Some(handle)
    }
}

#[inline]
fn is_class_matched(handle: HWND, class: &'static str) -> bool {
    let mut buf = [0u16; 256];
    let count = unsafe { GetClassNameW(handle, &mut buf) as usize };
    if count == 0 {
        return false;
    }

    OsString::from_wide(&buf[..count])
        .to_str()
        .map(|s| s.starts_with(class))
        .unwrap_or(false)
}
