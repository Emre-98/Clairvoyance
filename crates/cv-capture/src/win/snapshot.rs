//! Saves a captured frame as a JPEG (thumbnails), using the Windows Imaging Component.

use super::d3d::Gpu;
use anyhow::Result;
use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_BIND_FLAG, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

/// `tex` is a BGRA texture of `w`x`h`; the JPEG is scaled to `width` pixels wide.
pub fn save_jpeg(gpu: &Gpu, tex: &ID3D11Texture2D, w: u32, h: u32, path: &Path, width: u32) -> Result<()> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        let staging = gpu.texture(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, D3D11_BIND_FLAG(0), true)?;
        gpu.context.CopyResource(&staging, tex);
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
        let stride = m.RowPitch;
        let bytes = std::slice::from_raw_parts(m.pData as *const u8, (stride * h) as usize).to_vec();
        gpu.context.Unmap(&staging, 0);

        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppBGRA, stride, &bytes)?;
        let tw = width.min(w).max(16);
        let th = ((h as u64 * tw as u64) / w.max(1) as u64).max(16) as u32;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&bmp, tw, th, WICBitmapInterpolationModeFant)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let stream = wic.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), 0x4000_0000)?; // GENERIC_WRITE
        let enc = wic.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        let mut props = None;
        enc.CreateNewFrame(&mut frame, &mut props)?;
        let frame = frame.unwrap();
        frame.Initialize(props.as_ref())?;
        frame.SetSize(tw, th)?;
        let mut fmt = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut fmt)?;
        frame.WriteSource(&conv, std::ptr::null())?;
        frame.Commit()?;
        enc.Commit()?;
        Ok(())
    }
}
