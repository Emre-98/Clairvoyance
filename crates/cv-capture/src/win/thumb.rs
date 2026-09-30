//! Thumbnails from finished recordings: Media Foundation decodes one frame near a given time
//! (hardware decoding where available), the Windows Imaging Component scales it and saves a
//! small JPEG. Runs after a game on a background thread, never while recording.

use anyhow::{bail, Context, Result};
use std::mem::ManuallyDrop;
use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::VT_I8;

const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Decoded frame: top-down BGRX rows.
struct Frame {
    w: u32,
    h: u32,
    stride: u32,
    px: Vec<u8>,
}

/// Saves a JPEG (`width` pixels wide, 16:9 kept) of the frame at about `at_secs` in `video`.
pub fn video_thumbnail(video: &Path, at_secs: f64, out: &Path, width: u32) -> Result<()> {
    let frame = decode_frame(video, at_secs).with_context(|| format!("decoding a frame of {}", video.display()))?;
    save_jpeg(&frame, out, width)
}

fn decode_frame(video: &Path, at_secs: f64) -> Result<Frame> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let r = decode_inner(video, at_secs);
        let _ = MFShutdown();
        r
    }
}

unsafe fn decode_inner(video: &Path, at_secs: f64) -> Result<Frame> {
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 2)?;
        let attrs = attrs.context("no attributes")?;
        // Lets the reader convert the decoder's NV12 to RGB32 for us.
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(video.as_os_str()), &attrs)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(FIRST_VIDEO, true)?;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(FIRST_VIDEO, None, &mt)?;

        if at_secs > 0.0 {
            // PROPVARIANT holding a plain i64 (100 ns units): nothing to free, but keep it out of
            // Drop anyway (see audio.rs for why PROPVARIANTs are handled with care here).
            let mut pv = ManuallyDrop::new(PROPVARIANT::default());
            {
                let inner = &mut *pv.Anonymous.Anonymous;
                inner.vt = VT_I8;
                inner.Anonymous.hVal = (at_secs * 10_000_000.0) as i64;
            }
            // Seeking lands on the keyframe before `at_secs` (keyframes every 1-2 s): close enough.
            if let Err(e) = reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &*pv) {
                log::debug!("thumbnail seek failed ({e}); using the first frame");
            }
        }

        let mut sample: Option<IMFSample> = None;
        for _ in 0..240 {
            let mut flags = 0u32;
            let mut s: Option<IMFSample> = None;
            reader.ReadSample(FIRST_VIDEO, 0, None, Some(&mut flags), None, Some(&mut s))?;
            if s.is_some() {
                sample = s;
                break;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        let sample = sample.context("the video has no frame at that point")?;

        let cur = reader.GetCurrentMediaType(FIRST_VIDEO)?;
        let size = cur.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (fw, fh) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
        // H.264 decodes 1080p as 1088 lines; the display aperture says what to show.
        let (mut w, mut h) = (fw, fh);
        let mut area = [0u8; 16];
        let mut got = 0u32;
        if cur.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, Some(&mut got)).is_ok() && got >= 16 {
            let cx = i32::from_le_bytes(area[8..12].try_into().unwrap());
            let cy = i32::from_le_bytes(area[12..16].try_into().unwrap());
            if cx > 0 && cy > 0 && cx as u32 <= fw && cy as u32 <= fh {
                (w, h) = (cx as u32, cy as u32);
            }
        }
        if w < 16 || h < 16 {
            bail!("unexpected frame size {fw}x{fh}");
        }

        let buf = sample.GetBufferByIndex(0)?;
        let mut px = vec![0u8; (w * h * 4) as usize];
        let row = (w * 4) as usize;
        if let Ok(b2) = buf.cast::<IMF2DBuffer>() {
            // Lock2D gives the real pitch (negative for bottom-up images) and the first row.
            let mut scan0: *mut u8 = std::ptr::null_mut();
            let mut pitch: i32 = 0;
            b2.Lock2D(&mut scan0, &mut pitch)?;
            for y in 0..h as isize {
                let src = scan0.offset(y * pitch as isize);
                std::ptr::copy_nonoverlapping(src, px.as_mut_ptr().add(y as usize * row), row);
            }
            let _ = b2.Unlock2D();
        } else {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            buf.Lock(&mut data, None, Some(&mut len))?;
            let stride = if fh > 0 { (len / fh).max(fw * 4) } else { fw * 4 } as usize;
            if (stride * (h as usize - 1) + row) > len as usize {
                let _ = buf.Unlock();
                bail!("frame buffer too small");
            }
            for y in 0..h as usize {
                std::ptr::copy_nonoverlapping(data.add(y * stride), px.as_mut_ptr().add(y * row), row);
            }
            let _ = buf.Unlock();
        }
        Ok(Frame { w, h, stride: w * 4, px })
    }
}

fn save_jpeg(f: &Frame, out: &Path, width: u32) -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmapFromMemory(f.w, f.h, &GUID_WICPixelFormat32bppBGR, f.stride, &f.px)?;
        let tw = width.min(f.w).max(16);
        let th = ((f.h as u64 * tw as u64) / f.w.max(1) as u64).max(16) as u32;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&bmp, tw, th, WICBitmapInterpolationModeFant)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write to a temp file first so a half-written thumbnail is never shown.
        let tmp = out.with_extension("jpg.tmp");
        {
            let stream = wic.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(tmp.as_os_str()), 0x4000_0000)?; // GENERIC_WRITE
            let enc = wic.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())?;
            enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            let mut props = None;
            enc.CreateNewFrame(&mut frame, &mut props)?;
            let frame = frame.context("no JPEG frame")?;
            frame.Initialize(props.as_ref())?;
            frame.SetSize(tw, th)?;
            let mut fmt = GUID_WICPixelFormat24bppBGR;
            frame.SetPixelFormat(&mut fmt)?;
            frame.WriteSource(&conv, std::ptr::null())?;
            frame.Commit()?;
            enc.Commit()?;
        }
        std::fs::rename(&tmp, out)?;
        Ok(())
    }
}
