//! Thanks https://github.com/NiiightmareXD/windows-capture
//! Thanks https://github.com/obsproject/obs-studio/blob/cfb23a51ff8acad13dc739c31854d9f451e05298/libobs-d3d11/d3d11-subsystem.cpp#L587
//! Thanks https://github.com/obsproject/obs-studio/blob/cfb23a51ff8acad13dc739c31854d9f451e05298/libobs-winrt/winrt-capture.cpp#L244

use std::{
    cmp::min,
    mem, slice,
    sync::{Arc, Mutex, mpsc},
};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    System::{DispatcherQueue, DispatcherQueueController, DispatcherQueueHandler},
    Win32::{
        Foundation::{HMODULE, HWND, POINT, RECT},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_11_0,
            },
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Dxgi::{
                Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC},
                IDXGIDevice,
            },
            Gdi::ClientToScreen,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RoGetActivationFactory,
        },
        UI::WindowsAndMessaging::GetClientRect,
    },
    core::{HSTRING, Interface, RuntimeName},
};

use super::{Handle, HandleCell};
use crate::{Error, Result, capture::Frame};

#[derive(Debug)]
struct SendWrapper<T> {
    inner: T,
}

impl<T> SendWrapper<T> {
    fn as_inner(&self) -> &T {
        &self.inner
    }
}

impl<T: Clone> Clone for SendWrapper<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

unsafe impl<T> Send for SendWrapper<T> {}

#[derive(Debug)]
struct WgcCaptureInner {
    handle: SendWrapper<HWND>,
    d3d11_device: ID3D11Device,
    d3d11_context: ID3D11DeviceContext,
    d3d11_texture: Option<ID3D11Texture2D>,
    item: GraphicsCaptureItem,
    item_closed_token: i64,
    d3d_device: SendWrapper<IDirect3DDevice>,
    session: GraphicsCaptureSession,
    queue: DispatcherQueue,
    frame_format: DirectXPixelFormat,
    frame_pool: Direct3D11CaptureFramePool,
    frame_last_content_size: SizeInt32,
    frame_arrived_token: i64,
    frame_rx: mpsc::Receiver<Message>,
}

impl WgcCaptureInner {
    fn grab(&mut self) -> Result<Frame> {
        let handle = *self.handle.as_inner();
        let message = self.frame_rx.recv().unwrap();

        let frame = match message {
            Message::FrameArrived(frame) => frame,
            Message::ItemClosed => return Err(Error::WindowNotFound),
        };
        let frame_content_size = frame.ContentSize().unwrap();
        let surface = frame.Surface()?;
        let surface_texture = surface.cast::<IDirect3DDxgiInterfaceAccess>()?;
        let surface_texture = unsafe { surface_texture.GetInterface::<ID3D11Texture2D>()? };
        let mut surface_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { surface_texture.GetDesc(&mut surface_desc) };

        let texture_rect = get_client_rect(handle, surface_desc.Width, surface_desc.Height)?;
        let texture_width = texture_rect.right - texture_rect.left;
        let texture_height = texture_rect.bottom - texture_rect.top;
        if self.d3d11_texture.as_ref().is_none_or(|texture| {
            let mut texture_desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&raw mut texture_desc);
            };
            texture_desc.Width != texture_width || texture_desc.Height != texture_height
        }) {
            self.d3d11_texture = Some(create_texture_2d(
                &self.d3d11_device,
                texture_width,
                texture_height,
                surface_desc.Format,
            )?);
        }

        let texture = self.d3d11_texture.as_ref().unwrap();
        let mut resource = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.d3d11_context.CopySubresourceRegion(
                texture,
                0,
                0,
                0,
                0,
                &surface_texture,
                0,
                Some(&raw const texture_rect),
            );
        }
        unsafe {
            self.d3d11_context
                .Map(texture, 0, D3D11_MAP_READ, 0, Some(&mut resource))?;
        };
        let buffer = unsafe {
            slice::from_raw_parts::<u8>(
                resource.pData.cast(),
                (texture_height * resource.RowPitch) as usize,
            )
        };
        let vec = if texture_height * 4 != resource.RowPitch {
            let capacity = (texture_width * texture_height * 4) as usize;
            let dst_stride = (texture_width * 4) as usize;
            let mut vec = vec![0u8; capacity];
            for i in 0..texture_height as usize {
                let src_offset = resource.RowPitch as usize * i;
                let dst_offset = dst_stride * i;

                vec[dst_offset..dst_offset + dst_stride]
                    .copy_from_slice(&buffer[src_offset..src_offset + dst_stride]);
            }

            vec
        } else {
            buffer.to_vec()
        };

        unsafe {
            self.d3d11_context.Unmap(texture, 0);
        };

        if frame_content_size != self.frame_last_content_size {
            self.frame_format = DirectXPixelFormat(surface_desc.Format.0);
            self.frame_last_content_size = frame_content_size;

            let (recreated_tx, recreated_rx) = mpsc::channel::<()>();
            let frame_pool = self.frame_pool.clone();
            let frame_format = self.frame_format;
            let d3d_device = self.d3d_device.clone();
            let _ = self.queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
                let _ =
                    frame_pool.Recreate(d3d_device.as_inner(), frame_format, 1, frame_content_size);
                let _ = recreated_tx.send(());
                Ok(())
            }));

            if recreated_rx.recv().is_err() {
                return Err(Error::WindowNotFound);
            }
        }

        Ok(Frame {
            width: texture_width as i32,
            height: texture_height as i32,
            data: vec,
        })
    }
}

impl Drop for WgcCaptureInner {
    fn drop(&mut self) {
        let _ = self.item.RemoveClosed(self.item_closed_token);
        let _ = self.frame_pool.RemoveFrameArrived(self.frame_arrived_token);
        let _ = self.frame_pool.Close();
        let _ = self.session.Close();
    }
}

#[derive(Debug)]
enum Message {
    FrameArrived(Direct3D11CaptureFrame),
    ItemClosed,
}

#[derive(Debug)]
pub struct WgcCapture {
    handle: HandleCell,
    d3d11_device: ID3D11Device,
    d3d11_context: ID3D11DeviceContext,
    d3d_device: IDirect3DDevice,
    queue_controller: DispatcherQueueController,
    inner: Arc<Mutex<Option<WgcCaptureInner>>>,
}

impl WgcCapture {
    pub fn new(handle: Handle) -> Result<Self> {
        let (d3d11_device, d3d11_context) = create_d3d11_device()?;
        let d3d_device = create_d3d_device(&d3d11_device)?;
        let queue_controller = DispatcherQueueController::CreateOnDedicatedThread()?;

        Ok(Self {
            handle: HandleCell::new(handle),
            d3d11_device,
            d3d11_context,
            d3d_device,
            queue_controller,
            inner: Arc::new(Mutex::new(None)),
        })
    }

    pub fn grab(&mut self) -> Result<Frame> {
        if self.inner.lock().unwrap().is_none()
            && let Some(handle) = self.handle.as_inner()
        {
            self.start_capture(handle)?;
        }

        let mut guard = self.inner.lock().unwrap();
        let inner = guard.as_mut().ok_or(Error::WindowNotFound)?;
        let result = inner.grab();
        if let Err(Error::WindowNotFound) = result.as_ref() {
            drop(guard);
            self.stop_capture();
        }

        result
    }

    pub fn stop_capture(&mut self) {
        let _ = self.inner.lock().unwrap().take();
    }

    fn start_capture(&mut self, handle: HWND) -> Result<()> {
        let queue = self.queue_controller.DispatcherQueue()?;
        let queue_clone = queue.clone();
        let inner_arc = self.inner.clone();
        let handle = SendWrapper { inner: handle };
        let d3d11_device = self.d3d11_device.clone();
        let d3d11_context = self.d3d11_context.clone();
        let d3d_device = SendWrapper {
            inner: self.d3d_device.clone(),
        };
        let (pending_tx, pending_rx) = mpsc::channel::<()>();

        let _ = queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
            let (tx, rx) = mpsc::channel::<Message>();
            let frame_format = DirectXPixelFormat::B8G8R8A8UIntNormalized;

            let Ok(item) = create_graphics_capture_item(*handle.as_inner()) else {
                return Ok(()); // Avoids crash when open game after bot
            };
            let item_closed_tx = tx.clone();
            let item_closed_token = item.Closed(&TypedEventHandler::new(move |_, _| {
                item_closed_tx.send(Message::ItemClosed).unwrap();
                Ok(())
            }))?;

            let frame_last_content_size = item.Size()?;
            let (session, frame_pool) =
                create_capture_session(d3d_device.as_inner(), &item, frame_format)?;
            let frame_arrived_token = frame_pool.FrameArrived(&TypedEventHandler::<
                Direct3D11CaptureFramePool,
                _,
            >::new(
                move |frame_pool, _| {
                    tx.send(Message::FrameArrived(
                        frame_pool.as_ref().unwrap().TryGetNextFrame().unwrap(),
                    ))
                    .unwrap();
                    Ok(())
                },
            ))?;
            session.StartCapture()?;
            let _ = session.SetIsBorderRequired(false);

            let inner = WgcCaptureInner {
                handle: handle.clone(),
                d3d11_device: d3d11_device.clone(),
                d3d11_context: d3d11_context.clone(),
                d3d_device: d3d_device.clone(),
                d3d11_texture: None,
                item,
                item_closed_token,
                session,
                queue: queue_clone.clone(),
                frame_format,
                frame_pool,
                frame_last_content_size,
                frame_arrived_token,
                frame_rx: rx,
            };
            *inner_arc.lock().unwrap() = Some(inner);
            let _ = pending_tx.send(());

            Ok(())
        }));

        let _ = pending_rx.recv();
        Ok(())
    }
}

impl Drop for WgcCapture {
    fn drop(&mut self) {
        let _ = self.inner;
        let _ = self.d3d_device.Close();
        let _ = self.queue_controller.ShutdownQueueAsync().unwrap().get();
    }
}

#[inline]
fn get_client_rect(handle: HWND, width: u32, height: u32) -> Result<D3D11_BOX> {
    let mut window_rect = RECT::default();
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(handle, &mut client_rect)? };
    unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut window_rect).cast(),
            mem::size_of::<RECT>() as u32,
        )?
    };

    let rect_width = (client_rect.right - client_rect.left) as u32;
    let rect_height = (client_rect.bottom - client_rect.top) as u32;
    if rect_width == 0 || rect_height == 0 {
        return Err(Error::WindowInvalidSize);
    }
    let mut upper_left = POINT::default();
    unsafe { ClientToScreen(handle, &mut upper_left).ok()? };

    let left = (upper_left.x as u32).saturating_sub(window_rect.left as u32);
    let top = (upper_left.y as u32).saturating_sub(window_rect.top as u32);
    let texture_width = if width > left {
        min(width - left, client_rect.right as u32)
    } else {
        1
    };
    let texture_height = if height > top {
        min(height - top, client_rect.bottom as u32)
    } else {
        1
    };
    Ok(D3D11_BOX {
        left,
        top,
        right: left + texture_width,
        bottom: top + texture_height,
        front: 0,
        back: 1,
    })
}

#[inline]
fn create_texture_2d(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> Result<ID3D11Texture2D> {
    let mut texture = None;
    let texture_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    unsafe {
        device.CreateTexture2D(&texture_desc, None, Some(&mut texture))?;
    }
    Ok(texture.unwrap())
}

#[inline]
fn create_capture_session(
    device: &IDirect3DDevice,
    item: &GraphicsCaptureItem,
    format: DirectXPixelFormat,
) -> windows::core::Result<(GraphicsCaptureSession, Direct3D11CaptureFramePool)> {
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(device, format, 1, item.Size()?)?;
    let session = pool.CreateCaptureSession(item)?;
    Ok((session, pool))
}

#[inline]
fn create_graphics_capture_item(handle: HWND) -> windows::core::Result<GraphicsCaptureItem> {
    let factory = unsafe {
        RoGetActivationFactory::<IGraphicsCaptureItemInterop>(&HSTRING::from(
            GraphicsCaptureItem::NAME,
        ))?
    };
    Ok(unsafe { factory.CreateForWindow(handle)? })
}

#[inline]
fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let feature_flags = [
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let mut d3d_device = None;
    let mut feature_level = D3D_FEATURE_LEVEL_10_0;
    let mut d3d_device_context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_flags),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            Some(&mut feature_level),
            Some(&mut d3d_device_context),
        )?
    };
    Ok((d3d_device.unwrap(), d3d_device_context.unwrap()))
}

#[inline]
fn create_d3d_device(d3d11_device: &ID3D11Device) -> Result<IDirect3DDevice> {
    let dxgi_device = d3d11_device.cast::<IDXGIDevice>()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    let d3d_device = inspectable.cast()?;
    Ok(d3d_device)
}
