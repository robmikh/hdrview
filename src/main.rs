mod window;
mod handle;
mod dispatcher_queue;
mod d3d;
mod composition;

use composition::{create_composition_graphics_device, draw_to_surface};
use d3d::create_d3d_device;
use dispatcher_queue::{create_dispatcher_queue_controller_for_current_thread, shutdown_dispatcher_queue_controller_and_wait};
use window::Window;
use windows::{core::{ComInterface, Result, HSTRING}, Foundation::Numerics::Vector2, Graphics::{DirectX::{DirectXAlphaMode, DirectXPixelFormat}, SizeInt32}, Win32::{Graphics::{Direct3D11::{ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT}, Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC}, Imaging::{CLSID_WICImagingFactory, GUID_WICPixelFormat64bppRGBAHalf, IWICBitmap, IWICBitmapDecoder, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand}}, System::{Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, STGM_READ}, WinRT::{Composition::ICompositionDrawingSurfaceInterop, RoInitialize, RO_INIT_SINGLETHREADED}}, UI::{Shell::{SHCreateMemStream, SHCreateStreamOnFileW}, WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG}}}, UI::{Color, Composition::{CompositionStretch, Compositor}}};

const DEFAULT_IMAGE_BYTES: &[u8] = include_bytes!("../assets/hdr-image.jxr");

fn main() -> Result<()> {
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
    let controller = create_dispatcher_queue_controller_for_current_thread()?;

    // Init D3D11
    let d3d_device = create_d3d_device()?;
    let d3d_context = unsafe { d3d_device.GetImmediateContext()? };

    // Init Composition
    let compositor = Compositor::new()?;
    let root = compositor.CreateSpriteVisual()?;
    root.SetRelativeSizeAdjustment(Vector2::new(1.0, 1.0))?;
    root.SetBrush(&compositor.CreateColorBrushWithColor(Color { A: 255, R: 0, G: 0, B: 0 })?)?;

    // Create our window and hook up our visual tree
    let window = Window::new("hdrview", 800, 600)?;
    let target = window.create_window_target(&compositor, false)?;
    target.SetRoot(&root)?;

    // Create a CompositionGraphicsDevice for our surface
    let comp_graphics = create_composition_graphics_device(&compositor, &d3d_device)?;

    // Init WIC
    let wic_factory: IWICImagingFactory = unsafe {
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?
    };
    let decoder = create_wic_decoder_from_args(&wic_factory)?;
    
    // Get our image from the decoder and make sure it's in the FP16 format we need
    let frame = unsafe { decoder.GetFrame(0)? };
    let converter = unsafe { wic_factory.CreateFormatConverter()? };
    let (width, height) = unsafe {
        converter.Initialize(
            &frame, 
            &GUID_WICPixelFormat64bppRGBAHalf, 
            WICBitmapDitherTypeNone, 
            None, 
            0.0, 
            WICBitmapPaletteTypeMedianCut
        )?;
        let mut width = 0;
        let mut height = 0;
        converter.GetSize(&mut width, &mut height)?;
        (width, height)
    };
    let stride = 8 * width;
    let buffer_size = stride * height;
    let mut bytes = vec![0u8; buffer_size as usize];
    unsafe {
        converter.CopyPixels(std::ptr::null(), stride, &mut bytes)?
    };

    // Create a texture we'll use to copy to the surface
    let texture = {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };

        let init_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: bytes.as_ptr() as *const _,
            SysMemPitch: stride,
            SysMemSlicePitch: buffer_size,
        };

        unsafe {
            let mut texture = None;
            d3d_device.CreateTexture2D(&desc, Some(&init_data), Some(&mut texture))?;
            texture.unwrap()
        }
    };

    // Create our surface and copy our texture to it
    let surface = comp_graphics.CreateDrawingSurface2(SizeInt32 { Width: width as i32, Height: height as i32}, DirectXPixelFormat::R16G16B16A16Float, DirectXAlphaMode::Premultiplied)?;
    draw_to_surface::<ID3D11Texture2D, _>(&surface, None, |surface_texture, point| -> Result<()> {
        unsafe {
            d3d_context.CopySubresourceRegion(
                surface_texture, 
                0, 
                point.x as u32, 
                point.y as u32, 0, 
                &texture, 
                0, 
                None
            );
        }
        Ok(())
    })?;

    // Hookup our surface into the visual tree
    let content = compositor.CreateSpriteVisual()?;
    content.SetRelativeSizeAdjustment(Vector2::one())?;
    let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
    brush.SetStretch(CompositionStretch::Uniform)?;
    brush.SetHorizontalAlignmentRatio(0.5)?;
    brush.SetVerticalAlignmentRatio(0.5)?;
    content.SetBrush(&brush)?;
    root.Children()?.InsertAtTop(&content)?;

    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    // TODO: Do something with the error code.
    let _ = shutdown_dispatcher_queue_controller_and_wait(&controller, message.wParam.0 as i32)?;

    Ok(())
}

fn create_wic_decoder_from_args(wic_factory: &IWICImagingFactory) -> Result<IWICBitmapDecoder> {
    // First check to see if we were passed a path
    let args: Vec<_> = std::env::args().skip(1).collect();
    let stream = if let Some(path) = args.get(0) {
        // TODO: Validate JXR images only
        let path = HSTRING::from(path);
        unsafe {
            SHCreateStreamOnFileW(&path, STGM_READ.0)?
        }
    } else {
        unsafe { SHCreateMemStream(Some(DEFAULT_IMAGE_BYTES)).unwrap() }
    };

    let decoder = unsafe {
        wic_factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?
    };
    Ok(decoder)
}