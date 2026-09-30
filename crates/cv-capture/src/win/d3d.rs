//! Direct3D 11 device on the main GPU, shared by capture, conversion and the encoder.

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub vendor_id: u32,
    pub name: String,
    pub luid: i64,
}

pub fn vendor_name(id: u32) -> &'static str {
    match id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        _ => "Unknown",
    }
}

pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub info: GpuInfo,
}

// The device is created multithread-protected, so it may be used from several threads.
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

/// Adapters, most dedicated video memory first (the gaming GPU on laptops too).
pub fn adapters() -> Result<Vec<(IDXGIAdapter1, GpuInfo)>> {
    let mut out = Vec::new();
    unsafe {
        let f: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut i = 0;
        while let Ok(a) = f.EnumAdapters1(i) {
            i += 1;
            let d = a.GetDesc1()?;
            if (d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }
            let end = d.Description.iter().position(|c| *c == 0).unwrap_or(d.Description.len());
            let luid = ((d.AdapterLuid.HighPart as i64) << 32) | d.AdapterLuid.LowPart as i64;
            out.push((a, GpuInfo { vendor_id: d.VendorId, name: String::from_utf16_lossy(&d.Description[..end]), luid }, d.DedicatedVideoMemory));
        }
    }
    out.sort_by(|a, b| b.2.cmp(&a.2));
    Ok(out.into_iter().map(|(a, i, _)| (a, i)).collect())
}

impl Gpu {
    pub fn create() -> Result<Gpu> {
        let (adapter, info) = adapters()?.into_iter().next().context("no graphics card found")?;
        unsafe {
            let mut device = None;
            let mut context = None;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("creating the Direct3D 11 device")?;
            let device: ID3D11Device = device.unwrap();
            let context = context.unwrap();
            let mt: ID3D11Multithread = device.cast()?;
            let _ = mt.SetMultithreadProtected(true);
            Ok(Gpu { device, context, info })
        }
    }

    pub fn dxgi(&self) -> Result<IDXGIDevice> {
        Ok(self.device.cast()?)
    }

    pub fn texture(&self, w: u32, h: u32, format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT, bind: D3D11_BIND_FLAG, staging: bool) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: if staging { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
            BindFlags: bind.0 as u32,
            CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
            MiscFlags: 0,
        };
        let mut t = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut t))? };
        Ok(t.unwrap())
    }
}

/// Current time in 100 ns units on the QPC clock (the same clock as Windows Graphics
/// Capture's `SystemRelativeTime` and WASAPI's QPC positions).
pub fn qpc_hns() -> i64 {
    unsafe {
        let mut c = 0i64;
        let mut f = 0i64;
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
        if f == 0 {
            return 0;
        }
        ((c as i128 * 10_000_000) / f as i128) as i64
    }
}
