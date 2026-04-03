#[cfg(windows)]
use crate::windows::{BitBltCapture, WgcCapture, WindowBoxCapture, WindowsCapture};
use crate::{Error, Result, Window, windows::query_capture_handles};

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
    // TODO: Color format? Currently always BGRA
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
pub enum WindowsCaptureKind {
    BitBlt,
    BitBltArea,
    Wgc,
}

#[derive(Debug)]
pub struct Capture {
    window: Window,

    #[cfg(windows)]
    windows: WindowsCapture,
    #[cfg(windows)]
    windows_kind: WindowsCaptureKind,
}

impl Capture {
    pub fn new(window: Window) -> Result<Self> {
        if cfg!(windows) {
            return Ok(Self {
                window,
                windows: WindowsCapture::BitBlt(BitBltCapture::new(window.windows, false)),
                windows_kind: WindowsCaptureKind::BitBlt,
            });
        }

        Err(Error::PlatformNotSupported)
    }

    #[inline]
    pub fn grab(&mut self) -> Result<Frame> {
        if cfg!(windows) {
            return self.windows.grab();
        }

        Err(Error::PlatformNotSupported)
    }

    #[inline]
    pub fn window(&self) -> Result<Window> {
        if cfg!(windows) {
            return match &self.windows {
                WindowsCapture::Wgc(_) | WindowsCapture::BitBlt(_) => Ok(self.window),
                WindowsCapture::BitBltArea(capture) => Ok(capture.handle().into()),
            };
        }

        Err(Error::PlatformNotSupported)
    }

    #[inline]
    pub fn set_window(&mut self, window: Window) -> Result<()> {
        self.window = window;

        if cfg!(windows) {
            return self.windows_capture_kind(self.windows_kind);
        }

        Err(Error::PlatformNotSupported)
    }

    #[cfg(windows)]
    pub fn windows_capture_kind(&mut self, kind: WindowsCaptureKind) -> Result<()> {
        self.windows = match kind {
            WindowsCaptureKind::BitBlt => {
                WindowsCapture::BitBlt(BitBltCapture::new(self.window.windows, false))
            }
            WindowsCaptureKind::BitBltArea => {
                WindowsCapture::BitBltArea(WindowBoxCapture::default())
            }
            WindowsCaptureKind::Wgc => WindowsCapture::Wgc(WgcCapture::new(self.window.windows)?),
        };
        self.windows_kind = kind;

        Ok(())
    }
}

pub fn query_capture_windows() -> Result<Vec<Window>> {
    if cfg!(windows) {
        return Ok(query_capture_handles()
            .into_iter()
            .map(|handle| handle.into())
            .collect::<Vec<_>>());
    }

    Err(Error::PlatformNotSupported)
}
